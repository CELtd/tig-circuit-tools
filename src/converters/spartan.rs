use crate::dag::{DAG, OpType};
use curve25519_dalek::scalar::Scalar;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Sparse R1CS matrix in COO (Coordinate) format
/// Each entry is (row_index, column_index, value_bytes)
pub type R1CSMatrix = Vec<(usize, usize, [u8; 32])>;

/// Errors from the R1CS witness solver.
#[derive(Debug)]
pub enum WitnessError {
    /// `circuit_inputs` length doesn't match `num_inputs - num_outputs`.
    InvalidInputs { expected: usize, got: usize },
    /// Fixed-point iteration stalled before all variables were determined.
    /// The circuit is underconstrained or malformed.
    SolverStuck { solved: usize, total: usize },
}

/// Spartan R1CS instance representation
///
/// Variable layout follows libspartan's z-vector convention:
///   z = [vars(0..num_vars-1), 1, inputs(0..num_inputs-1)]
/// where:
///   - vars are private intermediate variables
///   - 1 is the implicit constant (at index num_vars, auto-inserted by libspartan)
///   - inputs are public I/O ordered as [outputs..., circuit_inputs...]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_snake_case)]
pub struct SpartanInstance {
    /// Number of constraints
    pub num_cons: usize,
    /// Number of private variables
    pub num_vars: usize,
    /// Number of public inputs (outputs + circuit inputs)
    pub num_inputs: usize,
    /// Left coefficient matrix (A)
    pub A: R1CSMatrix,
    /// Right coefficient matrix (B)
    pub B: R1CSMatrix,
    /// Output coefficient matrix (C)
    pub C: R1CSMatrix,
}

// ---------------------------------------------------------------------------
// Column assignment (shared between dag_to_spartan and compute_witness)
// ---------------------------------------------------------------------------

/// Maps DAG node IDs to z-vector column indices.
///
/// Produced by `assign_columns` and consumed by both `dag_to_spartan`
/// (to build R1CS matrices) and `compute_witness` (to place values).
pub(crate) struct ColumnAssignment {
    /// node_id → column index in the z-vector
    pub node_to_col: HashMap<usize, usize>,
    /// For Pow5 nodes: node_id → (sq_col, qd_col, source_node_id)
    /// sq and qd are private intermediate variables for x^5 unrolling.
    pub pow5_intermediates: HashMap<usize, (usize, usize, usize)>,
    /// Count of private variable columns (= SpartanInstance.num_vars)
    pub num_private_vars: usize,
    /// Count of public I/O columns (= SpartanInstance.num_inputs)
    pub num_public_inputs: usize,
    /// Column index for the constant 1 (= num_private_vars)
    pub col_const_one: usize,
    /// Output node IDs in order: [0, 1, ..., num_outputs-1]
    pub output_node_order: Vec<usize>,
    /// Input node IDs in order (excludes any that overlap with output nodes)
    pub input_node_order: Vec<usize>,
}

/// Assigns z-vector column indices to all DAG nodes.
///
/// Layout:
///   columns 0..num_private-1          → private intermediate variables
///   column  num_private               → constant 1 (implicit in libspartan)
///   columns num_private+1..           → public I/O [outputs..., inputs...]
pub(crate) fn assign_columns(dag: &DAG) -> ColumnAssignment {
    // Step 1: Identify public I/O nodes
    let output_node_order: Vec<usize> = (0..dag.num_outputs).collect();
    let output_set: HashSet<usize> = output_node_order.iter().cloned().collect();

    // Input nodes (OpType::Input) that are NOT already output nodes
    let input_node_order: Vec<usize> = dag
        .nodes
        .iter()
        .filter(|n| n.is_input() && !output_set.contains(&n.id))
        .map(|n| n.id)
        .collect();

    let public_set: HashSet<usize> = output_node_order
        .iter()
        .chain(input_node_order.iter())
        .cloned()
        .collect();

    // Step 2: Assign private variable columns (0, 1, 2, ...)
    let mut node_to_col: HashMap<usize, usize> = HashMap::new();
    let mut pow5_intermediates: HashMap<usize, (usize, usize, usize)> = HashMap::new();
    let mut next_private = 0usize;

    for node in &dag.nodes {
        let is_public = public_set.contains(&node.id);

        if !is_public {
            // This node gets a private variable column
            node_to_col.insert(node.id, next_private);
            next_private += 1;
        }

        // Pow5 intermediates (sq, qd) are always private, even if the output is public
        if let OpType::Pow5(src) = node.op {
            let sq_col = next_private;
            next_private += 1;
            let qd_col = next_private;
            next_private += 1;
            pow5_intermediates.insert(node.id, (sq_col, qd_col, src));
        }
    }

    let num_private_vars = next_private;
    let col_const_one = num_private_vars; // libspartan auto-inserts z[num_vars] = 1

    // Step 3: Assign public I/O columns (after the implicit constant 1)
    // Order: outputs first, then inputs
    let num_public_inputs = output_node_order.len() + input_node_order.len();
    let mut public_offset = 0usize;
    for &node_id in output_node_order.iter().chain(input_node_order.iter()) {
        node_to_col.insert(node_id, num_private_vars + 1 + public_offset);
        public_offset += 1;
    }

    ColumnAssignment {
        node_to_col,
        pow5_intermediates,
        num_private_vars,
        num_public_inputs,
        col_const_one,
        output_node_order,
        input_node_order,
    }
}

// ---------------------------------------------------------------------------
// dag_to_spartan
// ---------------------------------------------------------------------------

/// Converts a DAG to Spartan R1CS matrices with correct libspartan variable layout.
///
/// Each constraint has the form: <A,z> * <B,z> = <C,z>
pub fn dag_to_spartan(dag: &DAG) -> SpartanInstance {
    let cols = assign_columns(dag);

    let mut a_mat: R1CSMatrix = Vec::new();
    let mut b_mat: R1CSMatrix = Vec::new();
    let mut c_mat: R1CSMatrix = Vec::new();
    let mut current_row: usize = 0;

    for node in &dag.nodes {
        match node.op {
            OpType::Input => {} // No constraints for input nodes

            OpType::Alias(src) => {
                // node = src  →  node * 1 = src
                let node_col = cols.node_to_col[&node.id];
                let src_col = cols.node_to_col[&src];
                push_entry(&mut a_mat, current_row, node_col, Scalar::ONE);
                push_entry(&mut b_mat, current_row, cols.col_const_one, Scalar::ONE);
                push_entry(&mut c_mat, current_row, src_col, Scalar::ONE);
                current_row += 1;
            }

            OpType::Add(l, r) => {
                // node = l + r  →  (l + r) * 1 = node
                let l_col = cols.node_to_col[&l];
                let r_col = cols.node_to_col[&r];
                let out_col = cols.node_to_col[&node.id];
                push_entry(&mut a_mat, current_row, l_col, Scalar::ONE);
                push_entry(&mut a_mat, current_row, r_col, Scalar::ONE);
                push_entry(&mut b_mat, current_row, cols.col_const_one, Scalar::ONE);
                push_entry(&mut c_mat, current_row, out_col, Scalar::ONE);
                current_row += 1;
            }

            OpType::Mul(l, r) => {
                // node = l * r
                let l_col = cols.node_to_col[&l];
                let r_col = cols.node_to_col[&r];
                let out_col = cols.node_to_col[&node.id];
                push_entry(&mut a_mat, current_row, l_col, Scalar::ONE);
                push_entry(&mut b_mat, current_row, r_col, Scalar::ONE);
                push_entry(&mut c_mat, current_row, out_col, Scalar::ONE);
                current_row += 1;
            }

            OpType::Scale(src, k) => {
                // node = k * src  →  (k * src) * 1 = node
                let src_col = cols.node_to_col[&src];
                let out_col = cols.node_to_col[&node.id];
                push_entry(&mut a_mat, current_row, src_col, Scalar::from(k));
                push_entry(&mut b_mat, current_row, cols.col_const_one, Scalar::ONE);
                push_entry(&mut c_mat, current_row, out_col, Scalar::ONE);
                current_row += 1;
            }

            OpType::Pow5(_) => {
                // x^5 = three constraints using intermediates sq, qd:
                //   1. sq  = src * src
                //   2. qd  = sq  * sq
                //   3. out = qd  * src
                let &(sq_col, qd_col, src_id) =
                    cols.pow5_intermediates.get(&node.id).unwrap();
                let src_col = cols.node_to_col[&src_id];
                let out_col = cols.node_to_col[&node.id];

                // Constraint 1: src * src = sq
                push_entry(&mut a_mat, current_row, src_col, Scalar::ONE);
                push_entry(&mut b_mat, current_row, src_col, Scalar::ONE);
                push_entry(&mut c_mat, current_row, sq_col, Scalar::ONE);
                current_row += 1;

                // Constraint 2: sq * sq = qd
                push_entry(&mut a_mat, current_row, sq_col, Scalar::ONE);
                push_entry(&mut b_mat, current_row, sq_col, Scalar::ONE);
                push_entry(&mut c_mat, current_row, qd_col, Scalar::ONE);
                current_row += 1;

                // Constraint 3: qd * src = out
                push_entry(&mut a_mat, current_row, qd_col, Scalar::ONE);
                push_entry(&mut b_mat, current_row, src_col, Scalar::ONE);
                push_entry(&mut c_mat, current_row, out_col, Scalar::ONE);
                current_row += 1;
            }

            OpType::Output | OpType::Undefined => {} // Should not exist in finalized DAG
        }
    }

    SpartanInstance {
        num_cons: current_row,
        num_vars: cols.num_private_vars,
        num_inputs: cols.num_public_inputs,
        A: a_mat,
        B: b_mat,
        C: c_mat,
    }
}

/// Pushes a non-zero entry to an R1CS matrix.
#[inline]
fn push_entry(matrix: &mut R1CSMatrix, row: usize, col: usize, val: Scalar) {
    if val != Scalar::ZERO {
        matrix.push((row, col, val.to_bytes()));
    }
}

// ---------------------------------------------------------------------------
// compute_witness
// ---------------------------------------------------------------------------

/// Computes the full witness for a DAG given circuit input values.
///
/// Returns `(vars, public_io)` where:
/// - `vars`: private intermediate variable values, length = `SpartanInstance.num_vars`
/// - `public_io`: public I/O values, length = `SpartanInstance.num_inputs`,
///   ordered as `[output_0, ..., output_n, input_0, ..., input_m]`
///
/// `input_values` must have one entry per circuit input node (same order as
/// `assign_columns`'s `input_node_order`).
pub fn compute_witness(dag: &DAG, input_values: &[Scalar]) -> (Vec<Scalar>, Vec<Scalar>) {
    let cols = assign_columns(dag);

    assert_eq!(
        input_values.len(),
        cols.input_node_order.len(),
        "Expected {} input values, got {}",
        cols.input_node_order.len(),
        input_values.len()
    );

    // 1. Create per-node value storage
    let mut node_values: Vec<Option<Scalar>> = vec![None; dag.nodes.len()];

    // 2. Assign input values to Input nodes
    for (i, &node_id) in cols.input_node_order.iter().enumerate() {
        node_values[node_id] = Some(input_values[i]);
    }

    // 3. Evaluate nodes in REVERSE ID order (topological order for this DAG,
    //    since dependencies point from lower ID → higher ID)
    for node in dag.nodes.iter().rev() {
        match node.op {
            OpType::Input => {} // Already assigned

            OpType::Add(l, r) => {
                let val = node_values[l].unwrap() + node_values[r].unwrap();
                node_values[node.id] = Some(val);
            }

            OpType::Mul(l, r) => {
                let val = node_values[l].unwrap() * node_values[r].unwrap();
                node_values[node.id] = Some(val);
            }

            OpType::Alias(src) => {
                node_values[node.id] = node_values[src];
            }

            OpType::Scale(src, k) => {
                let val = Scalar::from(k) * node_values[src].unwrap();
                node_values[node.id] = Some(val);
            }

            OpType::Pow5(src) => {
                let x = node_values[src].unwrap();
                let sq = x * x;
                let qd = sq * sq;
                node_values[node.id] = Some(qd * x);
            }

            _ => {} // Output/Undefined should not exist in finalized DAG
        }
    }

    // 4. Build private vars vector (in column order)
    let mut vars = vec![Scalar::ZERO; cols.num_private_vars];

    for (&node_id, &col) in &cols.node_to_col {
        if col < cols.num_private_vars {
            vars[col] = node_values[node_id].expect("private node value not computed");
        }
    }

    // Pow5 intermediate values
    for (_, &(sq_col, qd_col, src_id)) in &cols.pow5_intermediates {
        let x = node_values[src_id].unwrap();
        let sq = x * x;
        let qd = sq * sq;
        vars[sq_col] = sq;
        vars[qd_col] = qd;
    }

    // 5. Build public I/O vector: [outputs..., inputs...]
    let mut public_io = Vec::with_capacity(cols.num_public_inputs);
    for &node_id in &cols.output_node_order {
        public_io.push(node_values[node_id].expect("output node value not computed"));
    }
    for &node_id in &cols.input_node_order {
        public_io.push(node_values[node_id].unwrap());
    }

    (vars, public_io)
}

// ---------------------------------------------------------------------------
// solve_witness_from_r1cs
// ---------------------------------------------------------------------------

/// Solves for the full witness (private variables + public outputs) given only
/// the R1CS matrices and circuit input values, without needing the DAG.
///
/// Uses fixed-point iteration: repeatedly scans constraints, solving any row
/// that has exactly one unknown variable. Converges when the R1CS represents
/// a deterministic forward computation (guaranteed for DAG-generated circuits
/// and well-formed optimized circuits).
///
/// # Arguments
/// - `instance` — Sparse R1CS matrices (A, B, C) and dimensions
/// - `num_outputs` — How many of the `num_inputs` public I/O slots are outputs
///   (placed first in the public I/O segment, these are unknown and solved)
/// - `circuit_inputs` — Known input scalars (e.g. x_eval), placed after outputs
///
/// # Returns
/// `(vars, public_io)` — same layout as `compute_witness`:
/// - `vars`: private variable values, length `num_vars`
/// - `public_io`: `[outputs..., circuit_inputs...]`, length `num_inputs`
pub fn solve_witness_from_r1cs(
    instance: &SpartanInstance,
    num_outputs: usize,
    circuit_inputs: &[Scalar],
) -> Result<(Vec<Scalar>, Vec<Scalar>), WitnessError> {
    let expected_inputs = instance.num_inputs - num_outputs;
    if circuit_inputs.len() != expected_inputs {
        return Err(WitnessError::InvalidInputs {
            expected: expected_inputs,
            got: circuit_inputs.len(),
        });
    }

    // --- Pre-process: group matrix entries by row, converting bytes to Scalars ---
    let mut a_rows: Vec<Vec<(usize, Scalar)>> = vec![Vec::new(); instance.num_cons];
    let mut b_rows: Vec<Vec<(usize, Scalar)>> = vec![Vec::new(); instance.num_cons];
    let mut c_rows: Vec<Vec<(usize, Scalar)>> = vec![Vec::new(); instance.num_cons];

    for &(row, col, ref bytes) in &instance.A {
        a_rows[row].push((col, Scalar::from_canonical_bytes(*bytes).unwrap()));
    }
    for &(row, col, ref bytes) in &instance.B {
        b_rows[row].push((col, Scalar::from_canonical_bytes(*bytes).unwrap()));
    }
    for &(row, col, ref bytes) in &instance.C {
        c_rows[row].push((col, Scalar::from_canonical_bytes(*bytes).unwrap()));
    }

    // --- Allocate z-vector and solved mask ---
    let z_len = instance.num_vars + 1 + instance.num_inputs;
    let mut z = vec![Scalar::ZERO; z_len];
    let mut solved = vec![false; z_len];

    // Constant 1 at position num_vars
    z[instance.num_vars] = Scalar::ONE;
    solved[instance.num_vars] = true;

    // Circuit inputs at positions num_vars + 1 + num_outputs ..
    for (i, &val) in circuit_inputs.iter().enumerate() {
        let idx = instance.num_vars + 1 + num_outputs + i;
        z[idx] = val;
        solved[idx] = true;
    }

    // --- Fixed-point iteration ---
    loop {
        let mut progress = false;

        for row in 0..instance.num_cons {
            // Find the unique unsolved column (if exactly one)
            let mut unsolved_col: Option<usize> = None;
            let mut multi = false;

            for &(col, _) in a_rows[row].iter().chain(b_rows[row].iter()).chain(c_rows[row].iter()) {
                if !solved[col] {
                    match unsolved_col {
                        None => unsolved_col = Some(col),
                        Some(prev) if prev == col => {} // same column
                        Some(_) => { multi = true; break; }
                    }
                }
            }

            if multi || unsolved_col.is_none() {
                continue;
            }
            let j = unsolved_col.unwrap();

            // Compute known sums and unknown coefficients for A, B, C
            let mut a_known = Scalar::ZERO;
            let mut a_j = Scalar::ZERO;
            for &(col, val) in &a_rows[row] {
                if col == j { a_j = a_j + val; } else { a_known = a_known + val * z[col]; }
            }

            let mut b_known = Scalar::ZERO;
            let mut b_j = Scalar::ZERO;
            for &(col, val) in &b_rows[row] {
                if col == j { b_j = b_j + val; } else { b_known = b_known + val * z[col]; }
            }

            let mut c_known = Scalar::ZERO;
            let mut c_j = Scalar::ZERO;
            for &(col, val) in &c_rows[row] {
                if col == j { c_j = c_j + val; } else { c_known = c_known + val * z[col]; }
            }

            // Universal formula: x = (c_known - a_known * b_known) / (a_j * b_known + b_j * a_known - c_j)
            let denom = a_j * b_known + b_j * a_known - c_j;
            if denom == Scalar::ZERO {
                continue; // degenerate constraint, doesn't determine this variable
            }

            let numer = c_known - a_known * b_known;
            z[j] = numer * denom.invert();
            solved[j] = true;
            progress = true;
        }

        if !progress {
            break;
        }
    }

    // --- Check convergence ---
    let num_need_solved = instance.num_vars + num_outputs;
    let mut num_solved = 0;
    for i in 0..instance.num_vars {
        if solved[i] { num_solved += 1; }
    }
    for i in 0..num_outputs {
        if solved[instance.num_vars + 1 + i] { num_solved += 1; }
    }
    if num_solved < num_need_solved {
        return Err(WitnessError::SolverStuck {
            solved: num_solved,
            total: num_need_solved,
        });
    }

    // --- Extract results ---
    let vars = z[..instance.num_vars].to_vec();
    let public_io = z[instance.num_vars + 1..instance.num_vars + 1 + instance.num_inputs].to_vec();

    Ok((vars, public_io))
}

// ---------------------------------------------------------------------------
// remove_aliases — baseline optimizer
// ---------------------------------------------------------------------------

/// Removes alias constraints from an R1CS instance via variable substitution.
///
/// An alias constraint has the form `out * 1 = src` in R1CS:
///   - A: single entry `(col_out, 1)` where col_out is a private variable
///   - B: single entry `(col_const, 1)` at the constant column
///   - C: single entry `(col_src, 1)`
///
/// The function substitutes `col_out → col_src` everywhere, removes the alias
/// rows, and compacts private variable columns so `num_vars` shrinks.
///
/// This is a pure function: if no aliases are found, returns a clone.
pub fn remove_aliases(instance: &SpartanInstance) -> SpartanInstance {
    let num_vars = instance.num_vars;
    let const_col = num_vars; // constant 1 sits at index num_vars
    let one_bytes = Scalar::ONE.to_bytes();

    // --- Step 1: Build per-row views ---
    let mut a_rows: Vec<Vec<(usize, [u8; 32])>> = vec![Vec::new(); instance.num_cons];
    let mut b_rows: Vec<Vec<(usize, [u8; 32])>> = vec![Vec::new(); instance.num_cons];
    let mut c_rows: Vec<Vec<(usize, [u8; 32])>> = vec![Vec::new(); instance.num_cons];

    for &(row, col, bytes) in &instance.A {
        a_rows[row].push((col, bytes));
    }
    for &(row, col, bytes) in &instance.B {
        b_rows[row].push((col, bytes));
    }
    for &(row, col, bytes) in &instance.C {
        c_rows[row].push((col, bytes));
    }

    // --- Step 2: Detect alias rows and build substitution map ---
    // substitution[col_out] = col_src
    let mut substitution: HashMap<usize, usize> = HashMap::new();
    let mut removed_rows: Vec<bool> = vec![false; instance.num_cons];

    for row in 0..instance.num_cons {
        // Check alias pattern:
        //   A: exactly 1 entry, coeff = 1, column < num_vars (private)
        //   B: exactly 1 entry, column = const_col, coeff = 1
        //   C: exactly 1 entry, coeff = 1
        if a_rows[row].len() != 1 || b_rows[row].len() != 1 || c_rows[row].len() != 1 {
            continue;
        }

        let (a_col, a_val) = a_rows[row][0];
        let (b_col, b_val) = b_rows[row][0];
        let (c_col, c_val) = c_rows[row][0];

        if a_val != one_bytes || b_val != one_bytes || c_val != one_bytes {
            continue;
        }
        if b_col != const_col {
            continue;
        }

        // This is an alias: a_col = c_col. Substitute away whichever is private.
        // If a_col is private → substitute a_col → c_col
        // If c_col is private (and a_col isn't) → substitute c_col → a_col
        // If neither is private → can't remove this row
        if a_col < num_vars {
            substitution.insert(a_col, c_col);
            removed_rows[row] = true;
        } else if c_col < num_vars {
            substitution.insert(c_col, a_col);
            removed_rows[row] = true;
        }
    }

    if substitution.is_empty() {
        return instance.clone();
    }

    // --- Step 3: Resolve chains ---
    // If a → b → c, flatten to a → c
    let mut changed = true;
    while changed {
        changed = false;
        let snapshot: Vec<(usize, usize)> = substitution.iter().map(|(&k, &v)| (k, v)).collect();
        for (key, target) in snapshot {
            if let Some(&further) = substitution.get(&target) {
                substitution.insert(key, further);
                changed = true;
            }
        }
    }

    // --- Step 4: Apply substitutions to surviving rows ---
    // Helper: apply substitution to a single column
    let remap_col = |col: usize| -> usize {
        substitution.get(&col).copied().unwrap_or(col)
    };

    // Build new flat matrices with substituted columns, skipping removed rows
    // We also need to merge entries that land on the same (row, col) after substitution
    let mut new_a: R1CSMatrix = Vec::new();
    let mut new_b: R1CSMatrix = Vec::new();
    let mut new_c: R1CSMatrix = Vec::new();
    let mut new_row = 0usize;

    for row in 0..instance.num_cons {
        if removed_rows[row] {
            continue;
        }

        // Process each matrix for this row, merging duplicate columns
        fn emit_row(
            src: &[(usize, [u8; 32])],
            dest: &mut R1CSMatrix,
            new_row: usize,
            remap: &dyn Fn(usize) -> usize,
        ) {
            // Collect entries with remapped columns, merge duplicates
            let mut merged: HashMap<usize, Scalar> = HashMap::new();
            for &(col, bytes) in src {
                let new_col = remap(col);
                let val = Scalar::from_canonical_bytes(bytes).unwrap();
                let entry = merged.entry(new_col).or_insert(Scalar::ZERO);
                *entry = *entry + val;
            }
            for (col, val) in merged {
                if val != Scalar::ZERO {
                    dest.push((new_row, col, val.to_bytes()));
                }
            }
        }

        emit_row(&a_rows[row], &mut new_a, new_row, &remap_col);
        emit_row(&b_rows[row], &mut new_b, new_row, &remap_col);
        emit_row(&c_rows[row], &mut new_c, new_row, &remap_col);
        new_row += 1;
    }

    let new_num_cons = new_row;

    // --- Step 5: Column compaction (private variables only) ---
    // Collect all private variable columns still referenced
    let mut live_private_cols: HashSet<usize> = HashSet::new();
    for &(_, col, _) in new_a.iter().chain(new_b.iter()).chain(new_c.iter()) {
        if col < num_vars {
            live_private_cols.insert(col);
        }
    }

    // Sort and assign new sequential indices
    let mut sorted_live: Vec<usize> = live_private_cols.into_iter().collect();
    sorted_live.sort();
    let new_num_vars = sorted_live.len();

    // Build full column remap: old_col → new_col
    let mut col_remap: HashMap<usize, usize> = HashMap::new();
    for (new_idx, &old_col) in sorted_live.iter().enumerate() {
        col_remap.insert(old_col, new_idx);
    }
    // Constant column: old num_vars → new_num_vars
    col_remap.insert(const_col, new_num_vars);
    // Public I/O columns: shift from old base to new base
    for i in 0..instance.num_inputs {
        let old_io_col = num_vars + 1 + i;
        let new_io_col = new_num_vars + 1 + i;
        col_remap.insert(old_io_col, new_io_col);
    }

    // Apply column remap to all entries
    fn remap_matrix(mat: &mut R1CSMatrix, col_remap: &HashMap<usize, usize>) {
        for entry in mat.iter_mut() {
            entry.1 = col_remap[&entry.1];
        }
    }

    remap_matrix(&mut new_a, &col_remap);
    remap_matrix(&mut new_b, &col_remap);
    remap_matrix(&mut new_c, &col_remap);

    SpartanInstance {
        num_cons: new_num_cons,
        num_vars: new_num_vars,
        num_inputs: instance.num_inputs, // public I/O count unchanged
        A: new_a,
        B: new_b,
        C: new_c,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{CircuitConfig, generate_dag};

    #[test]
    fn test_spartan_generation() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);
        let spartan = dag_to_spartan(&dag);

        assert!(spartan.num_cons > 0);
        assert!(spartan.num_vars > 0);
        assert!(spartan.num_inputs > 0);
    }

    #[test]
    fn test_constraint_count_matches_dag() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);
        let spartan = dag_to_spartan(&dag);

        let expected_constraints = dag.total_constraints();
        assert_eq!(spartan.num_cons, expected_constraints);
    }

    #[test]
    fn test_column_bounds() {
        // Verify all matrix entries reference valid z-vector columns
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("bounds_test", &config);
        let si = dag_to_spartan(&dag);

        let max_col = si.num_vars + 1 + si.num_inputs;
        for &(row, col, _) in si.A.iter().chain(si.B.iter()).chain(si.C.iter()) {
            assert!(row < si.num_cons, "row {} >= num_cons {}", row, si.num_cons);
            assert!(col < max_col, "col {} >= max_col {}", col, max_col);
        }
    }

    #[test]
    fn test_compute_witness_basic() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("witness_test", &config);
        let si = dag_to_spartan(&dag);

        // Count input nodes (excluding overlap with output nodes)
        let output_set: HashSet<usize> = (0..dag.num_outputs).collect();
        let actual_inputs = dag
            .nodes
            .iter()
            .filter(|n| n.is_input() && !output_set.contains(&n.id))
            .count();

        let input_values: Vec<Scalar> = (0..actual_inputs)
            .map(|i| Scalar::from((i + 1) as u64))
            .collect();

        let (vars, public_io) = compute_witness(&dag, &input_values);

        assert_eq!(vars.len(), si.num_vars);
        assert_eq!(public_io.len(), si.num_inputs);
    }
}

#[cfg(test)]
mod satisfiability_tests {
    use super::*;
    use crate::dag::{CircuitConfig, generate_dag};
    use libspartan::{InputsAssignment, Instance, VarsAssignment};
    use std::collections::HashSet;

    /// The critical integration test: verify that dag_to_spartan + compute_witness
    /// produce a valid R1CS instance that passes libspartan's is_sat() check.
    #[test]
    fn test_circuit_satisfiability() {
        let seeds = ["test1", "test2", "test3", "validation", "edge_case"];
        let difficulties = [1u32, 2, 4];

        for seed in &seeds {
            for &diff in &difficulties {
                let config = CircuitConfig::from_difficulty(diff);
                let dag = generate_dag(seed, &config);
                let si = dag_to_spartan(&dag);

                // Compute input count (excluding overlap with output nodes)
                let output_set: HashSet<usize> = (0..dag.num_outputs).collect();
                let num_actual_inputs = dag
                    .nodes
                    .iter()
                    .filter(|n| n.is_input() && !output_set.contains(&n.id))
                    .count();

                let input_values: Vec<Scalar> = (0..num_actual_inputs)
                    .map(|i| Scalar::from((i + 1) as u64))
                    .collect();

                let (vars, public_io) = compute_witness(&dag, &input_values);

                // Create libspartan Instance
                let inst = Instance::new(
                    si.num_cons,
                    si.num_vars,
                    si.num_inputs,
                    &si.A,
                    &si.B,
                    &si.C,
                )
                .unwrap_or_else(|e| {
                    panic!(
                        "Instance::new failed for seed='{}' diff={}: {:?}",
                        seed, diff, e
                    )
                });

                // Convert to byte arrays
                let vars_bytes: Vec<[u8; 32]> =
                    vars.iter().map(|s| s.to_bytes()).collect();
                let io_bytes: Vec<[u8; 32]> =
                    public_io.iter().map(|s| s.to_bytes()).collect();

                let assignment_vars = VarsAssignment::new(&vars_bytes).unwrap();
                let assignment_inputs = InputsAssignment::new(&io_bytes).unwrap();

                // THE CRITICAL CHECK
                let sat = inst
                    .is_sat(&assignment_vars, &assignment_inputs)
                    .unwrap_or_else(|e| {
                        panic!(
                            "is_sat error for seed='{}' diff={}: {:?}",
                            seed, diff, e
                        )
                    });
                assert!(
                    sat,
                    "Circuit NOT satisfiable for seed='{}' difficulty={}",
                    seed, diff
                );
            }
        }
    }

    /// Ground-truth test: solve_witness_from_r1cs must produce the same result
    /// as compute_witness (which uses the DAG) across multiple seeds and difficulties.
    #[test]
    fn test_solve_witness_from_r1cs() {
        let seeds = ["test1", "test2", "test3", "validation", "edge_case"];
        let difficulties = [1u32, 2, 4];

        for seed in &seeds {
            for &diff in &difficulties {
                let config = CircuitConfig::from_difficulty(diff);
                let dag = generate_dag(seed, &config);
                let si = dag_to_spartan(&dag);

                // Compute input count (excluding overlap with output nodes)
                let output_set: HashSet<usize> = (0..dag.num_outputs).collect();
                let num_actual_inputs = dag
                    .nodes
                    .iter()
                    .filter(|n| n.is_input() && !output_set.contains(&n.id))
                    .count();

                let input_values: Vec<Scalar> = (0..num_actual_inputs)
                    .map(|i| Scalar::from((i + 1) as u64))
                    .collect();

                // Ground truth from DAG
                let (vars_dag, public_io_dag) = compute_witness(&dag, &input_values);

                // Solver from R1CS only
                let (vars_solver, public_io_solver) =
                    solve_witness_from_r1cs(&si, dag.num_outputs, &input_values)
                        .unwrap_or_else(|e| {
                            panic!(
                                "solve_witness_from_r1cs failed for seed='{}' diff={}: {:?}",
                                seed, diff, e
                            )
                        });

                // Must be byte-identical
                assert_eq!(
                    vars_dag.len(),
                    vars_solver.len(),
                    "vars length mismatch for seed='{}' diff={}",
                    seed, diff
                );
                for (i, (a, b)) in vars_dag.iter().zip(vars_solver.iter()).enumerate() {
                    assert_eq!(
                        a.to_bytes(),
                        b.to_bytes(),
                        "vars[{}] mismatch for seed='{}' diff={}",
                        i, seed, diff
                    );
                }

                assert_eq!(
                    public_io_dag.len(),
                    public_io_solver.len(),
                    "public_io length mismatch for seed='{}' diff={}",
                    seed, diff
                );
                for (i, (a, b)) in public_io_dag.iter().zip(public_io_solver.iter()).enumerate() {
                    assert_eq!(
                        a.to_bytes(),
                        b.to_bytes(),
                        "public_io[{}] mismatch for seed='{}' diff={}",
                        i, seed, diff
                    );
                }
            }
        }
    }

    /// Test that remove_aliases correctly detects and removes alias constraints,
    /// and the optimized circuit is still satisfiable.
    #[test]
    fn test_remove_aliases_reduces_constraints() {
        use crate::analysis::analyze_dag;

        let seeds = ["test1", "test2", "test3", "validation", "edge_case"];
        let difficulties = [1u32, 2, 4];

        for seed in &seeds {
            for &diff in &difficulties {
                let config = CircuitConfig::from_difficulty(diff);
                let dag = generate_dag(seed, &config);
                let si = dag_to_spartan(&dag);
                let analysis = analyze_dag(&dag);

                let optimized = remove_aliases(&si);

                // Constraint count must decrease by exactly alias_removable
                assert_eq!(
                    optimized.num_cons,
                    si.num_cons - analysis.alias_removable,
                    "Wrong constraint count for seed='{}' diff={}: expected {} - {} = {}, got {}",
                    seed, diff, si.num_cons, analysis.alias_removable,
                    si.num_cons - analysis.alias_removable, optimized.num_cons
                );

                // num_vars must have decreased (alias columns removed)
                assert!(
                    optimized.num_vars <= si.num_vars,
                    "num_vars should not increase for seed='{}' diff={}",
                    seed, diff
                );

                // num_inputs must be unchanged
                assert_eq!(
                    optimized.num_inputs, si.num_inputs,
                    "num_inputs changed for seed='{}' diff={}",
                    seed, diff
                );

                // Compute input count
                let output_set: HashSet<usize> = (0..dag.num_outputs).collect();
                let num_actual_inputs = dag
                    .nodes
                    .iter()
                    .filter(|n| n.is_input() && !output_set.contains(&n.id))
                    .count();

                let input_values: Vec<Scalar> = (0..num_actual_inputs)
                    .map(|i| Scalar::from((i + 1) as u64))
                    .collect();

                // Witness solver must converge on the optimized circuit
                let (vars_opt, public_io_opt) =
                    solve_witness_from_r1cs(&optimized, dag.num_outputs, &input_values)
                        .unwrap_or_else(|e| {
                            panic!(
                                "solve_witness_from_r1cs failed on optimized circuit \
                                 for seed='{}' diff={}: {:?}",
                                seed, diff, e
                            )
                        });

                // Optimized circuit must be satisfiable
                let inst = Instance::new(
                    optimized.num_cons,
                    optimized.num_vars,
                    optimized.num_inputs,
                    &optimized.A,
                    &optimized.B,
                    &optimized.C,
                )
                .unwrap_or_else(|e| {
                    panic!(
                        "Instance::new failed on optimized circuit for seed='{}' diff={}: {:?}",
                        seed, diff, e
                    )
                });

                let vars_bytes: Vec<[u8; 32]> =
                    vars_opt.iter().map(|s| s.to_bytes()).collect();
                let io_bytes: Vec<[u8; 32]> =
                    public_io_opt.iter().map(|s| s.to_bytes()).collect();

                let assignment_vars = VarsAssignment::new(&vars_bytes).unwrap();
                let assignment_inputs = InputsAssignment::new(&io_bytes).unwrap();

                let sat = inst
                    .is_sat(&assignment_vars, &assignment_inputs)
                    .unwrap_or_else(|e| {
                        panic!(
                            "is_sat error on optimized circuit for seed='{}' diff={}: {:?}",
                            seed, diff, e
                        )
                    });
                assert!(
                    sat,
                    "Optimized circuit NOT satisfiable for seed='{}' difficulty={}",
                    seed, diff
                );

                // Public I/O outputs must match the original circuit
                let (_, public_io_orig) = compute_witness(&dag, &input_values);
                for i in 0..dag.num_outputs {
                    assert_eq!(
                        public_io_orig[i].to_bytes(),
                        public_io_opt[i].to_bytes(),
                        "Output {} mismatch for seed='{}' diff={}",
                        i, seed, diff
                    );
                }
            }
        }
    }
}

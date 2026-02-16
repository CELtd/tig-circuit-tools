use crate::dag::{DAG, OpType};
use curve25519_dalek::scalar::Scalar;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Sparse R1CS matrix in COO (Coordinate) format
/// Each entry is (row_index, column_index, value_bytes)
pub type R1CSMatrix = Vec<(usize, usize, [u8; 32])>;

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
}

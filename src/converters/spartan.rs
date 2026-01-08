use crate::dag::{DAG, OpType};
use curve25519_dalek::scalar::Scalar;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Sparse R1CS matrix in COO (Coordinate) format
/// Each entry is (row_index, column_index, value_bytes)
pub type R1CSMatrix = Vec<(usize, usize, [u8; 32])>;

/// Spartan R1CS instance representation
#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_snake_case)]
pub struct SpartanInstance {
    /// Number of constraints
    pub num_cons: usize,
    /// Number of variables
    pub num_vars: usize,
    /// Number of public inputs
    pub num_inputs: usize,
    /// Left coefficient matrix (A)
    pub A: R1CSMatrix,
    /// Right coefficient matrix (B)
    pub B: R1CSMatrix,
    /// Output coefficient matrix (C)
    pub C: R1CSMatrix,
}

/// Converts a DAG to Spartan R1CS matrices
///
/// Generates R1CS (Rank-1 Constraint System) matrices in sparse format
/// suitable for Spartan proof systems using the Curve25519 field.
///
/// Each constraint has the form: A × B = C
///
/// # Arguments
/// * `dag` - The DAG to convert
///
/// # Returns
/// A SpartanInstance containing the R1CS matrices
pub fn dag_to_spartan(dag: &DAG) -> SpartanInstance {
    let mut builder = SpartanBuilder::new();

    // Identify and assign input variables first
    let input_ids: Vec<usize> = dag
        .nodes
        .iter()
        .filter(|n| n.is_input())
        .map(|n| n.id)
        .collect();

    for &id in &input_ids {
        let var = builder.next_var_idx;
        builder.next_var_idx += 1;
        builder.node_to_var.insert(id, var);
    }

    // Generate constraints for each node
    for node in &dag.nodes {
        match node.op {
            OpType::Input => {} // Already handled
            OpType::Alias(src) => {
                // A = B  =>  A × 1 = B
                let a_idx = builder.get_var(node.id);
                let b_idx = builder.get_var(src);
                builder.add_cons(
                    vec![(a_idx, Scalar::ONE)],
                    vec![(0, Scalar::ONE)], // Constant 1
                    vec![(b_idx, Scalar::ONE)],
                );
            }
            OpType::Add(l, r) => {
                // Out = L + R  =>  (L + R) × 1 = Out
                let l_idx = builder.get_var(l);
                let r_idx = builder.get_var(r);
                let out_idx = builder.get_var(node.id);

                builder.add_cons(
                    vec![(l_idx, Scalar::ONE), (r_idx, Scalar::ONE)],
                    vec![(0, Scalar::ONE)],
                    vec![(out_idx, Scalar::ONE)],
                );
            }
            OpType::Mul(l, r) => {
                // Out = L × R
                let l_idx = builder.get_var(l);
                let r_idx = builder.get_var(r);
                let out_idx = builder.get_var(node.id);

                builder.add_cons(
                    vec![(l_idx, Scalar::ONE)],
                    vec![(r_idx, Scalar::ONE)],
                    vec![(out_idx, Scalar::ONE)],
                );
            }
            OpType::Scale(src, k) => {
                // Out = k × Src  =>  (k × Src) × 1 = Out
                let k_scalar = Scalar::from(k);
                let src_idx = builder.get_var(src);
                let out_idx = builder.get_var(node.id);

                builder.add_cons(
                    vec![(src_idx, k_scalar)],
                    vec![(0, Scalar::ONE)],
                    vec![(out_idx, Scalar::ONE)],
                );
            }
            OpType::Pow5(src) => {
                // Implement x^5 as three constraints (matching Circom)
                // 1. sq = x × x
                // 2. qd = sq × sq
                // 3. out = qd × x
                let src_idx = builder.get_var(src);
                let out_idx = builder.get_var(node.id);

                // Allocate intermediate variables
                let sq_idx = builder.next_var_idx;
                builder.next_var_idx += 1;
                let qd_idx = builder.next_var_idx;
                builder.next_var_idx += 1;

                // Constraint 1: x × x = sq
                builder.add_cons(
                    vec![(src_idx, Scalar::ONE)],
                    vec![(src_idx, Scalar::ONE)],
                    vec![(sq_idx, Scalar::ONE)],
                );

                // Constraint 2: sq × sq = qd
                builder.add_cons(
                    vec![(sq_idx, Scalar::ONE)],
                    vec![(sq_idx, Scalar::ONE)],
                    vec![(qd_idx, Scalar::ONE)],
                );

                // Constraint 3: qd × x = out
                builder.add_cons(
                    vec![(qd_idx, Scalar::ONE)],
                    vec![(src_idx, Scalar::ONE)],
                    vec![(out_idx, Scalar::ONE)],
                );
            }
            OpType::Output | OpType::Undefined => {} // No constraints
        }
    }

    SpartanInstance {
        num_cons: builder.current_row,
        num_vars: builder.next_var_idx,
        num_inputs: input_ids.len(),
        A: builder.A,
        B: builder.B,
        C: builder.C,
    }
}

/// Helper struct for building R1CS matrices
#[allow(non_snake_case)]
struct SpartanBuilder {
    A: R1CSMatrix,
    B: R1CSMatrix,
    C: R1CSMatrix,
    node_to_var: HashMap<usize, usize>,
    next_var_idx: usize,
    current_row: usize,
}

impl SpartanBuilder {
    fn new() -> Self {
        Self {
            A: Vec::new(),
            B: Vec::new(),
            C: Vec::new(),
            node_to_var: HashMap::new(),
            next_var_idx: 1, // Index 0 is reserved for constant 1
            current_row: 0,
        }
    }

    /// Gets or allocates a variable index for a node
    fn get_var(&mut self, id: usize) -> usize {
        if let Some(&var) = self.node_to_var.get(&id) {
            var
        } else {
            let var = self.next_var_idx;
            self.next_var_idx += 1;
            self.node_to_var.insert(id, var);
            var
        }
    }

    /// Adds a constraint: sum(A_terms) × sum(B_terms) = sum(C_terms)
    fn add_cons(
        &mut self,
        a_terms: Vec<(usize, Scalar)>,
        b_terms: Vec<(usize, Scalar)>,
        c_terms: Vec<(usize, Scalar)>,
    ) {
        // Add non-zero entries to sparse matrices
        for (idx, val) in a_terms {
            if val != Scalar::ZERO {
                self.A.push((self.current_row, idx, val.to_bytes()));
            }
        }
        for (idx, val) in b_terms {
            if val != Scalar::ZERO {
                self.B.push((self.current_row, idx, val.to_bytes()));
            }
        }
        for (idx, val) in c_terms {
            if val != Scalar::ZERO {
                self.C.push((self.current_row, idx, val.to_bytes()));
            }
        }
        self.current_row += 1;
    }
}

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
}

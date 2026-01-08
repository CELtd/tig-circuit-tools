use super::{Node, OpType};
use super::config::CircuitConfig;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

/// Represents a generated circuit DAG
#[derive(Debug, Clone)]
pub struct DAG {
    /// The nodes in the DAG
    pub nodes: Vec<Node>,
    /// Number of input nodes
    pub num_inputs: usize,
    /// Number of output nodes
    pub num_outputs: usize,
}

impl DAG {
    /// Returns the total number of constraints in the DAG
    pub fn total_constraints(&self) -> usize {
        self.nodes.iter().map(|n| n.constraint_count()).sum()
    }

    /// Returns an iterator over input nodes
    pub fn inputs(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(|n| n.is_input())
    }

    /// Returns an iterator over output nodes
    pub fn outputs(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(|n| n.is_output())
    }
}

/// Generates a random circuit DAG from a seed and configuration
///
/// The DAG is constructed backwards from outputs to inputs, ensuring
/// acyclicity by design. The generation is deterministic given the same
/// seed and configuration.
///
/// # Arguments
/// * `seed` - String seed for deterministic random generation
/// * `config` - Configuration parameters for the circuit
///
/// # Returns
/// A DAG structure containing the generated circuit
pub fn generate_dag(seed: &str, config: &CircuitConfig) -> DAG {
    // Initialize deterministic RNG from seed
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let mut rng = ChaCha20Rng::from_seed(hasher.finalize().into());

    let mut nodes: Vec<Node> = Vec::new();
    let mut frontier: VecDeque<usize> = VecDeque::new();

    // Create output nodes (DAG roots)
    let num_outputs = rng.gen_range(1..=3);
    for _ in 0..num_outputs {
        let id = nodes.len();
        nodes.push(Node::new(id, OpType::Output));
        frontier.push_back(id);
    }

    let mut constraint_count = 0;

    // Grow the DAG backwards from outputs to inputs
    while !frontier.is_empty() {
        if constraint_count >= config.num_constraints {
            break;
        }

        let curr_id = frontier.pop_front().unwrap();
        let rand_val = rng.gen_range(0.0..1.0);

        // Calculate thresholds for operation selection
        let t_alias = config.alias_ratio;
        let t_lin = t_alias + config.linear_ratio;
        let t_pow = t_lin + config.power_map_ratio;

        // Select operation type based on configuration ratios
        if rand_val < t_alias {
            // Alias operation (optimization trap)
            let src = pick_operand(curr_id, &mut nodes, &mut frontier, &mut rng, 0.0);
            nodes[curr_id].op = OpType::Alias(src);
            constraint_count += 1;
        } else if rand_val < t_lin {
            // Linear scaling operation (optimization trap)
            let src = pick_operand(curr_id, &mut nodes, &mut frontier, &mut rng, 0.0);
            let k = rng.gen_range(2..1000);
            nodes[curr_id].op = OpType::Scale(src, k);
            constraint_count += 1;
        } else if rand_val < t_pow && (constraint_count + 3 <= config.num_constraints) {
            // Pow5 operation (algebraic trap, costs 3 constraints)
            let src = pick_operand(
                curr_id,
                &mut nodes,
                &mut frontier,
                &mut rng,
                config.redundancy_ratio,
            );
            nodes[curr_id].op = OpType::Pow5(src);
            constraint_count += 3;
        } else {
            // Add or Mul operation
            let is_mul = rng.gen_bool(0.5);
            let l = pick_operand(
                curr_id,
                &mut nodes,
                &mut frontier,
                &mut rng,
                config.redundancy_ratio,
            );
            let r = pick_operand(
                curr_id,
                &mut nodes,
                &mut frontier,
                &mut rng,
                config.redundancy_ratio,
            );
            nodes[curr_id].op = if is_mul {
                OpType::Mul(l, r)
            } else {
                OpType::Add(l, r)
            };
            constraint_count += 1;
        }
    }

    // Finalize all undefined nodes as inputs
    finalize_inputs(&mut nodes);

    let num_inputs = nodes.iter().filter(|n| n.is_input()).count();

    DAG {
        nodes,
        num_inputs,
        num_outputs,
    }
}

/// Picks an operand for an operation, either reusing an existing node
/// or creating a new one
fn pick_operand(
    parent_id: usize,
    nodes: &mut Vec<Node>,
    frontier: &mut VecDeque<usize>,
    rng: &mut ChaCha20Rng,
    redundancy_ratio: f64,
) -> usize {
    let valid_count = nodes.len().saturating_sub(parent_id + 1);

    // Try to reuse existing node with given probability
    if rng.gen_bool(redundancy_ratio) && valid_count > 0 {
        parent_id + 1 + rng.gen_range(0..valid_count)
    } else {
        // Create new undefined node
        let id = nodes.len();
        nodes.push(Node::new(id, OpType::Undefined));
        frontier.push_back(id);
        id
    }
}

/// Converts all undefined and output nodes to inputs
fn finalize_inputs(nodes: &mut Vec<Node>) {
    for node in nodes {
        if matches!(node.op, OpType::Undefined | OpType::Output) {
            node.op = OpType::Input;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_generation() {
        let config = CircuitConfig::from_difficulty(1);
        let dag1 = generate_dag("test_seed", &config);
        let dag2 = generate_dag("test_seed", &config);

        assert_eq!(dag1.nodes.len(), dag2.nodes.len());
        assert_eq!(dag1.num_inputs, dag2.num_inputs);
        assert_eq!(dag1.num_outputs, dag2.num_outputs);
    }

    #[test]
    fn test_different_seeds_produce_different_dags() {
        let config = CircuitConfig::from_difficulty(1);
        let dag1 = generate_dag("seed1", &config);
        let dag2 = generate_dag("seed2", &config);

        // Different seeds should produce different DAGs
        // (with extremely high probability)
        assert_ne!(dag1.nodes.len(), dag2.nodes.len());
    }

    #[test]
    fn test_dag_has_outputs() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);

        // The DAG should have recorded 1-3 outputs
        assert!(dag.num_outputs >= 1 && dag.num_outputs <= 3);
    }
}

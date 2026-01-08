use crate::dag::{DAG, OpType};
use std::collections::HashMap;

/// Converts a DAG to Circom circuit code
///
/// Generates a Circom 2.0.0 template that implements the circuit
/// described by the DAG. The output is a valid `.circom` file that
/// can be compiled with the Circom compiler.
///
/// # Arguments
/// * `dag` - The DAG to convert
///
/// # Returns
/// A string containing the complete Circom circuit code
pub fn dag_to_circom(dag: &DAG) -> String {
    let mut code = String::new();

    // Header
    code.push_str("pragma circom 2.0.0;\n");
    code.push_str("template Challenge() {\n");

    // Collect input nodes
    let inputs: Vec<usize> = dag
        .nodes
        .iter()
        .filter(|n| n.is_input())
        .map(|n| n.id)
        .collect();

    // Declare inputs and outputs
    code.push_str(&format!(
        "    signal input in[{}];\n",
        inputs.len()
    ));
    code.push_str(&format!(
        "    signal output out[{}];\n",
        dag.num_outputs
    ));

    // Create mapping from node IDs to signal names
    let mut signal_map = HashMap::new();
    for (i, &id) in inputs.iter().enumerate() {
        signal_map.insert(id, i);
    }

    // Declare intermediate signals
    for node in &dag.nodes {
        if !signal_map.contains_key(&node.id) {
            code.push_str(&format!("    signal s_{};\n", node.id));
        }
    }

    // Generate constraints (in reverse order to maintain topological ordering)
    for i in (0..dag.nodes.len()).rev() {
        let node = &dag.nodes[i];
        let target = get_signal_name(node.id, &signal_map);

        match node.op {
            OpType::Add(l, r) => {
                let left = get_signal_name(l, &signal_map);
                let right = get_signal_name(r, &signal_map);
                code.push_str(&format!("    {} <== {} + {};\n", target, left, right));
            }
            OpType::Mul(l, r) => {
                let left = get_signal_name(l, &signal_map);
                let right = get_signal_name(r, &signal_map);
                code.push_str(&format!("    {} <== {} * {};\n", target, left, right));
            }
            OpType::Alias(src) => {
                let source = get_signal_name(src, &signal_map);
                code.push_str(&format!("    {} <== {};\n", target, source));
            }
            OpType::Scale(src, k) => {
                let source = get_signal_name(src, &signal_map);
                code.push_str(&format!("    {} <== {} * {};\n", target, source, k));
            }
            OpType::Pow5(s) => {
                // Implement x^5 as three constraints:
                // sq = x * x
                // qd = sq * sq
                // out = qd * x
                let source = get_signal_name(s, &signal_map);
                let sq = format!("{}_sq", target);
                let qd = format!("{}_qd", target);

                code.push_str(&format!("    signal {};\n", sq));
                code.push_str(&format!("    {} <== {} * {};\n", sq, source, source));

                code.push_str(&format!("    signal {};\n", qd));
                code.push_str(&format!("    {} <== {} * {};\n", qd, sq, sq));

                code.push_str(&format!("    {} <== {} * {};\n", target, qd, source));
            }
            _ => {} // Skip Input, Output, Undefined
        }
    }

    // Connect outputs
    for i in 0..dag.num_outputs {
        let output_signal = get_signal_name(i, &signal_map);
        code.push_str(&format!("    out[{}] <== {};\n", i, output_signal));
    }

    // Close template and instantiate main component
    code.push_str("}\n");
    code.push_str("component main = Challenge();\n");

    code
}

/// Helper function to get the signal name for a node ID
fn get_signal_name(id: usize, map: &HashMap<usize, usize>) -> String {
    if let Some(&idx) = map.get(&id) {
        format!("in[{}]", idx)
    } else {
        format!("s_{}", id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{CircuitConfig, generate_dag};

    #[test]
    fn test_circom_generation() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);
        let circom = dag_to_circom(&dag);

        assert!(circom.contains("pragma circom 2.0.0"));
        assert!(circom.contains("template Challenge()"));
        assert!(circom.contains("component main = Challenge()"));
    }

    #[test]
    fn test_circom_has_inputs_outputs() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);
        let circom = dag_to_circom(&dag);

        assert!(circom.contains("signal input in["));
        assert!(circom.contains("signal output out["));
    }
}

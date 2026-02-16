//! TIG Circuit Tools
//!
//! A modular library for generating, converting, and analyzing R1CS circuits
//! for Zero-Knowledge proof systems.
//!
//! # Features
//!
//! - **DAG Generation**: Generate random, deterministic circuit DAGs from seeds
//! - **Format Conversion**: Convert DAGs to Circom or Spartan R1CS formats
//! - **Analysis Tools**: Analyze circuits for optimization potential
//!
//! # Quick Start
//!
//! ```rust
//! use tig_circuit_tools::*;
//!
//! // Generate a circuit DAG
//! let config = CircuitConfig::from_difficulty(5);
//! let dag = generate_dag("my_seed", &config);
//!
//! // Analyze the DAG
//! let analysis = analyze_dag(&dag);
//! println!("Baseline constraints: {}", analysis.baseline_constraints);
//! println!("Optimization potential: {:.2}%", analysis.total_possible_reduction * 100.0);
//!
//! // Convert to Circom
//! let circom_code = dag_to_circom(&dag);
//!
//! // Convert to Spartan
//! let spartan = dag_to_spartan(&dag);
//! ```
//!
//! # Module Organization
//!
//! - [`dag`]: DAG generation and data structures
//! - [`converters`]: DAG to circuit format converters
//! - [`analysis`]: Circuit analysis tools

pub mod dag;
pub mod converters;
pub mod analysis;

// Re-export commonly used items for convenience
pub use dag::{CircuitConfig, Node, OpType, DAG, generate_dag};
pub use converters::{compute_witness, dag_to_circom, dag_to_spartan, R1CSMatrix, SpartanInstance};
pub use analysis::{analyze_dag, analyze_spartan_instance, compare_circuits,
    count_spartan_constraints, AnalysisResult, SpartanMetrics};
#[cfg(feature = "cli")]
pub use analysis::{compare_optimization_levels, count_circom_constraints, CircomMetrics, OptLevel};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_workflow() {
        // Generate a DAG
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("integration_test", &config);

        // Analyze it
        let analysis = analyze_dag(&dag);
        assert!(analysis.baseline_constraints > 0);

        // Convert to Circom
        let circom = dag_to_circom(&dag);
        assert!(circom.contains("pragma circom"));

        // Convert to Spartan
        let spartan = dag_to_spartan(&dag);
        assert_eq!(spartan.num_cons, analysis.baseline_constraints);

        // Analyze Spartan
        let spartan_metrics = analyze_spartan_instance(&spartan);
        assert_eq!(spartan_metrics.total_constraints, analysis.baseline_constraints);
    }

    #[test]
    fn test_determinism() {
        let config = CircuitConfig::from_difficulty(1);
        let dag1 = generate_dag("determinism_test", &config);
        let dag2 = generate_dag("determinism_test", &config);

        assert_eq!(dag1.nodes.len(), dag2.nodes.len());
        assert_eq!(dag1.total_constraints(), dag2.total_constraints());
    }

    #[test]
    fn test_parity_check() {
        // Verify that Circom and Spartan have the same constraint count
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("parity_test", &config);

        let analysis = analyze_dag(&dag);
        let spartan = dag_to_spartan(&dag);

        assert_eq!(
            analysis.baseline_constraints,
            spartan.num_cons,
            "Circom and Spartan constraint counts must match!"
        );
    }
}

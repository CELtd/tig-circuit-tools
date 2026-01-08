//! Circuit analysis tools
//!
//! This module provides analysis capabilities for circuits in different formats:
//! - DAG analysis: Semantic analysis on the circuit DAG
//! - Spartan analysis: Structural metrics from R1CS matrices
//! - Circom analysis: Constraint counting via compiler

mod dag_analysis;
mod spartan_tools;
mod circom_tools;

pub use dag_analysis::{analyze_dag, AnalysisResult};
pub use spartan_tools::{
    analyze_spartan_instance, compare_circuits, count_spartan_constraints, SpartanMetrics,
};
pub use circom_tools::{
    compare_optimization_levels, count_circom_constraints, CircomMetrics, OptLevel,
};

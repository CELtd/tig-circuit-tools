//! DAG (Directed Acyclic Graph) generation for R1CS circuits
//!
//! This module provides the core functionality for generating random,
//! deterministic circuit DAGs. The DAGs are constructed backwards from
//! outputs to inputs, ensuring acyclicity by design.

mod node;
mod config;
mod generator;

pub use node::{Node, OpType};
pub use config::CircuitConfig;
pub use generator::{generate_dag, DAG};

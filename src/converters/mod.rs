// ! DAG to circuit format converters
//!
//! This module provides converters from DAG to various circuit formats:
//! - Circom: Human-readable circuit language
//! - Spartan: R1CS matrices for transparent proof systems

mod circom;
mod spartan;

pub use circom::dag_to_circom;
pub use spartan::{dag_to_spartan, R1CSMatrix, SpartanInstance};

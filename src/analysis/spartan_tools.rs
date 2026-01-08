use crate::converters::SpartanInstance;
use std::fs;
use std::path::Path;

/// Metrics extracted from a Spartan circuit
#[derive(Debug, Clone)]
pub struct SpartanMetrics {
    /// Total number of constraints
    pub total_constraints: usize,
    /// Total number of variables
    pub total_variables: usize,
    /// Number of public inputs
    pub public_inputs: usize,
    /// Number of non-zero entries in A matrix
    pub matrix_a_nonzeros: usize,
    /// Number of non-zero entries in B matrix
    pub matrix_b_nonzeros: usize,
    /// Number of non-zero entries in C matrix
    pub matrix_c_nonzeros: usize,
}

impl SpartanMetrics {
    /// Returns the total number of non-zero entries across all matrices
    pub fn total_nonzeros(&self) -> usize {
        self.matrix_a_nonzeros + self.matrix_b_nonzeros + self.matrix_c_nonzeros
    }

    /// Returns the average number of non-zero entries per constraint
    pub fn avg_nonzeros_per_constraint(&self) -> f64 {
        if self.total_constraints == 0 {
            0.0
        } else {
            self.total_nonzeros() as f64 / self.total_constraints as f64
        }
    }

    /// Returns the sparsity ratio (fraction of non-zero entries)
    pub fn sparsity_ratio(&self) -> f64 {
        let total_possible = self.total_constraints * self.total_variables * 3;
        if total_possible == 0 {
            0.0
        } else {
            self.total_nonzeros() as f64 / total_possible as f64
        }
    }

    /// Pretty-prints the metrics
    pub fn display(&self) -> String {
        format!(
            "Spartan Metrics:\n\
             - Total Constraints: {}\n\
             - Total Variables: {}\n\
             - Public Inputs: {}\n\
             - Matrix A Non-zeros: {}\n\
             - Matrix B Non-zeros: {}\n\
             - Matrix C Non-zeros: {}\n\
             - Total Non-zeros: {}\n\
             - Avg Non-zeros/Constraint: {:.2}\n\
             - Sparsity Ratio: {:.6}",
            self.total_constraints,
            self.total_variables,
            self.public_inputs,
            self.matrix_a_nonzeros,
            self.matrix_b_nonzeros,
            self.matrix_c_nonzeros,
            self.total_nonzeros(),
            self.avg_nonzeros_per_constraint(),
            self.sparsity_ratio()
        )
    }
}

/// Counts constraints in a Spartan circuit from a JSON file
///
/// # Arguments
/// * `path` - Path to the Spartan JSON file
///
/// # Returns
/// Metrics extracted from the Spartan circuit
///
/// # Errors
/// Returns an error if the file cannot be read or parsed
pub fn count_spartan_constraints<P: AsRef<Path>>(path: P) -> Result<SpartanMetrics, String> {
    let content = fs::read_to_string(path.as_ref())
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let instance: SpartanInstance = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    Ok(SpartanMetrics {
        total_constraints: instance.num_cons,
        total_variables: instance.num_vars,
        public_inputs: instance.num_inputs,
        matrix_a_nonzeros: instance.A.len(),
        matrix_b_nonzeros: instance.B.len(),
        matrix_c_nonzeros: instance.C.len(),
    })
}

/// Analyzes a Spartan instance directly (without file I/O)
///
/// # Arguments
/// * `instance` - The Spartan instance to analyze
///
/// # Returns
/// Metrics extracted from the instance
pub fn analyze_spartan_instance(instance: &SpartanInstance) -> SpartanMetrics {
    SpartanMetrics {
        total_constraints: instance.num_cons,
        total_variables: instance.num_vars,
        public_inputs: instance.num_inputs,
        matrix_a_nonzeros: instance.A.len(),
        matrix_b_nonzeros: instance.B.len(),
        matrix_c_nonzeros: instance.C.len(),
    }
}

/// Compares two Spartan circuits (e.g., before and after optimization)
///
/// # Arguments
/// * `baseline` - Metrics from the baseline circuit
/// * `optimized` - Metrics from the optimized circuit
///
/// # Returns
/// A string describing the comparison
pub fn compare_circuits(baseline: &SpartanMetrics, optimized: &SpartanMetrics) -> String {
    let constraint_reduction = if baseline.total_constraints > 0 {
        (1.0 - (optimized.total_constraints as f64 / baseline.total_constraints as f64)) * 100.0
    } else {
        0.0
    };

    let variable_reduction = if baseline.total_variables > 0 {
        (1.0 - (optimized.total_variables as f64 / baseline.total_variables as f64)) * 100.0
    } else {
        0.0
    };

    format!(
        "Circuit Comparison:\n\
         - Baseline Constraints: {} → Optimized: {} ({:.2}% reduction)\n\
         - Baseline Variables: {} → Optimized: {} ({:.2}% reduction)\n\
         - Public Inputs: {} → {}",
        baseline.total_constraints,
        optimized.total_constraints,
        constraint_reduction,
        baseline.total_variables,
        optimized.total_variables,
        variable_reduction,
        baseline.public_inputs,
        optimized.public_inputs
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{CircuitConfig, generate_dag};
    use crate::converters::dag_to_spartan;

    #[test]
    fn test_analyze_spartan_instance() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);
        let spartan = dag_to_spartan(&dag);
        let metrics = analyze_spartan_instance(&spartan);

        assert_eq!(metrics.total_constraints, spartan.num_cons);
        assert_eq!(metrics.total_variables, spartan.num_vars);
        assert_eq!(metrics.public_inputs, spartan.num_inputs);
    }

    #[test]
    fn test_metrics_calculations() {
        let metrics = SpartanMetrics {
            total_constraints: 100,
            total_variables: 150,
            public_inputs: 10,
            matrix_a_nonzeros: 200,
            matrix_b_nonzeros: 200,
            matrix_c_nonzeros: 200,
        };

        assert_eq!(metrics.total_nonzeros(), 600);
        assert_eq!(metrics.avg_nonzeros_per_constraint(), 6.0);
    }
}

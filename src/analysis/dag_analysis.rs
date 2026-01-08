use crate::dag::{DAG, OpType};

/// Result of semantic analysis on a DAG
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// Total baseline constraint count
    pub baseline_constraints: usize,
    /// Number of alias constraints (removable via substitution)
    pub alias_removable: usize,
    /// Number of linear scaling constraints (removable via folding)
    pub linear_removable: usize,
    /// Number of algebraic constraints removable from Pow5 operations
    pub algebraic_removable: usize,
    /// Theoretical maximum reduction percentage
    pub total_possible_reduction: f64,
}

/// Analyzes a DAG to determine optimization potential
///
/// Performs semantic analysis on the DAG to identify:
/// - Alias operations (can be eliminated via variable substitution)
/// - Linear scaling (can be folded into subsequent operations)
/// - Pow5 operations (can be optimized using advanced algebraic techniques)
///
/// # Arguments
/// * `dag` - The DAG to analyze
///
/// # Returns
/// Analysis results showing baseline constraints and optimization potential
pub fn analyze_dag(dag: &DAG) -> AnalysisResult {
    let mut alias_removable = 0;
    let mut linear_removable = 0;
    let mut algebraic_removable = 0;
    let mut baseline = 0;

    for node in &dag.nodes {
        match node.op {
            OpType::Alias(_) => {
                baseline += 1;
                alias_removable += 1;
            }
            OpType::Scale(_, _) => {
                baseline += 1;
                linear_removable += 1;
            }
            OpType::Pow5(_) => {
                baseline += 3;
                algebraic_removable += 2; // Can theoretically reduce 2 out of 3
            }
            OpType::Add(_, _) | OpType::Mul(_, _) => {
                baseline += 1;
            }
            _ => {} // Input, Output, Undefined don't contribute constraints
        }
    }

    let total_removable = alias_removable + linear_removable + algebraic_removable;
    let total_possible_reduction = calculate_reducibility(baseline as f64, (baseline - total_removable) as f64);

    AnalysisResult {
        baseline_constraints: baseline,
        alias_removable,
        linear_removable,
        algebraic_removable,
        total_possible_reduction,
    }
}

/// Calculates the reducibility percentage between baseline and optimized
fn calculate_reducibility(baseline: f64, optimized: f64) -> f64 {
    if baseline <= 1e-6 {
        return 0.0;
    }
    1.0 - (optimized / baseline)
}

impl AnalysisResult {
    /// Returns the total number of removable constraints
    pub fn total_removable(&self) -> usize {
        self.alias_removable + self.linear_removable + self.algebraic_removable
    }

    /// Returns the theoretical minimum constraint count after optimization
    pub fn optimized_constraints(&self) -> usize {
        self.baseline_constraints.saturating_sub(self.total_removable())
    }

    /// Pretty-prints the analysis result
    pub fn display(&self) -> String {
        format!(
            "Analysis Result:\n\
             - Baseline Constraints: {}\n\
             - Alias Removable: {}\n\
             - Linear Removable: {}\n\
             - Algebraic Removable: {}\n\
             - Total Removable: {}\n\
             - Theoretical Minimum: {}\n\
             - Max Reduction: {:.2}%",
            self.baseline_constraints,
            self.alias_removable,
            self.linear_removable,
            self.algebraic_removable,
            self.total_removable(),
            self.optimized_constraints(),
            self.total_possible_reduction * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{CircuitConfig, generate_dag};

    #[test]
    fn test_analysis_on_generated_dag() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);
        let analysis = analyze_dag(&dag);

        assert!(analysis.baseline_constraints > 0);
        assert!(analysis.total_possible_reduction >= 0.0);
        assert!(analysis.total_possible_reduction <= 1.0);
    }

    #[test]
    fn test_baseline_matches_dag_constraints() {
        let config = CircuitConfig::from_difficulty(1);
        let dag = generate_dag("test", &config);
        let analysis = analyze_dag(&dag);

        assert_eq!(analysis.baseline_constraints, dag.total_constraints());
    }
}

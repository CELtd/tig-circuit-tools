/// Configuration for circuit generation
#[derive(Debug, Clone)]
pub struct CircuitConfig {
    /// Target number of constraints
    pub num_constraints: usize,
    /// Probability of reusing existing nodes (creates shared subexpressions)
    pub redundancy_ratio: f64,
    /// Frequency of Pow5 operations (algebraic trap)
    pub power_map_ratio: f64,
    /// Frequency of alias operations (optimization trap)
    pub alias_ratio: f64,
    /// Frequency of linear scaling operations (linear trap)
    pub linear_ratio: f64,
}

impl CircuitConfig {
    /// Creates a new configuration with custom parameters
    pub fn new(
        num_constraints: usize,
        redundancy_ratio: f64,
        power_map_ratio: f64,
        alias_ratio: f64,
        linear_ratio: f64,
    ) -> Self {
        Self {
            num_constraints,
            redundancy_ratio,
            power_map_ratio,
            alias_ratio,
            linear_ratio,
        }
    }

    /// Creates a default configuration for the given difficulty
    ///
    /// # Arguments
    /// * `difficulty` - Difficulty parameter (num_constraints = difficulty * 1000)
    pub fn from_difficulty(difficulty: u32) -> Self {
        let num_constraints = (difficulty as usize) * 1000;
        Self {
            num_constraints,
            redundancy_ratio: 0.25,
            power_map_ratio: 0.15,
            alias_ratio: 0.15,
            linear_ratio: 0.20,
        }
    }
}

impl Default for CircuitConfig {
    fn default() -> Self {
        Self::from_difficulty(1)
    }
}

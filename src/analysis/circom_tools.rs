use std::path::Path;
use std::process::Command;

/// Optimization level for Circom compiler
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    /// No optimization (--O0)
    O0,
    /// Level 1 optimization (--O1): Simplifications and basic optimizations
    O1,
    /// Level 2 optimization (--O2): Aggressive optimizations including constraint reduction
    O2,
}

impl OptLevel {
    /// Returns the command-line flag for this optimization level
    pub fn as_flag(&self) -> &str {
        match self {
            OptLevel::O0 => "--O0",
            OptLevel::O1 => "--O1",
            OptLevel::O2 => "--O2",
        }
    }
}

/// Metrics extracted from Circom compiler output
#[derive(Debug, Clone)]
pub struct CircomMetrics {
    /// Number of non-linear constraints
    pub non_linear_constraints: usize,
    /// Number of linear constraints
    pub linear_constraints: usize,
    /// Total constraints (non-linear + linear)
    pub total_constraints: usize,
}

impl CircomMetrics {
    /// Pretty-prints the metrics
    pub fn display(&self) -> String {
        format!(
            "Circom Metrics:\n\
             - Non-linear Constraints: {}\n\
             - Linear Constraints: {}\n\
             - Total Constraints: {}",
            self.non_linear_constraints,
            self.linear_constraints,
            self.total_constraints
        )
    }

    /// Returns the constraint reduction compared to a baseline
    pub fn reduction_from(&self, baseline: &CircomMetrics) -> f64 {
        if baseline.total_constraints == 0 {
            return 0.0;
        }
        1.0 - (self.total_constraints as f64 / baseline.total_constraints as f64)
    }
}

/// Counts constraints in a Circom circuit by compiling it
///
/// This function runs the Circom compiler with the specified optimization
/// level and parses its output to extract constraint counts.
///
/// # Arguments
/// * `path` - Path to the `.circom` file
/// * `opt_level` - Optimization level to use
///
/// # Returns
/// Metrics extracted from the compiler output
///
/// # Errors
/// Returns an error if:
/// - The file doesn't exist
/// - The Circom compiler is not installed
/// - The compilation fails
/// - The output cannot be parsed
///
/// # Note
/// Requires the `circom` compiler to be installed and available in PATH.
pub fn count_circom_constraints<P: AsRef<Path>>(
    path: P,
    opt_level: OptLevel,
) -> Result<CircomMetrics, String> {
    // Run circom compiler
    let output = Command::new("circom")
        .arg(path.as_ref())
        .arg("--r1cs")
        .arg(opt_level.as_flag())
        .arg("--sym")
        .output()
        .map_err(|e| format!("Failed to run circom compiler: {}. Is it installed?", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Circom compilation failed: {}", stderr));
    }

    // Parse compiler output
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_circom_output(&stdout)
}

/// Parses Circom compiler output to extract constraint counts
fn parse_circom_output(output: &str) -> Result<CircomMetrics, String> {
    // Use regex to extract constraint counts
    // Example output:
    //   non-linear constraints: 1234
    //   linear constraints: 567

    let nl_pattern = regex::Regex::new(r"non-linear constraints:\s*(\d+)")
        .map_err(|e| format!("Regex error: {}", e))?;
    let l_pattern = regex::Regex::new(r"(?m)^linear constraints:\s*(\d+)")
        .map_err(|e| format!("Regex error: {}", e))?;

    let non_linear = nl_pattern
        .captures(output)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .ok_or_else(|| "Could not find 'non-linear constraints' in output".to_string())?;

    let linear = l_pattern
        .captures(output)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0); // Linear constraints might be 0

    Ok(CircomMetrics {
        non_linear_constraints: non_linear,
        linear_constraints: linear,
        total_constraints: non_linear + linear,
    })
}

/// Compares constraint counts across different optimization levels
///
/// # Arguments
/// * `path` - Path to the `.circom` file
///
/// # Returns
/// A tuple of (O0_metrics, O1_metrics, O2_metrics)
pub fn compare_optimization_levels<P: AsRef<Path>>(
    path: P,
) -> Result<(CircomMetrics, CircomMetrics, CircomMetrics), String> {
    let o0 = count_circom_constraints(&path, OptLevel::O0)?;
    let o1 = count_circom_constraints(&path, OptLevel::O1)?;
    let o2 = count_circom_constraints(&path, OptLevel::O2)?;

    Ok((o0, o1, o2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_circom_output() {
        let output = "template instances: 1\n\
                      non-linear constraints: 1234\n\
                      linear constraints: 567\n\
                      public inputs: 10\n\
                      private inputs: 100\n\
                      public outputs: 2\n\
                      wires: 1500\n\
                      labels: 2000";

        let metrics = parse_circom_output(output).unwrap();
        assert_eq!(metrics.non_linear_constraints, 1234);
        assert_eq!(metrics.linear_constraints, 567);
        assert_eq!(metrics.total_constraints, 1801);
    }

    #[test]
    fn test_opt_level_flags() {
        assert_eq!(OptLevel::O0.as_flag(), "--O0");
        assert_eq!(OptLevel::O1.as_flag(), "--O1");
        assert_eq!(OptLevel::O2.as_flag(), "--O2");
    }

    #[test]
    fn test_reduction_calculation() {
        let baseline = CircomMetrics {
            non_linear_constraints: 1000,
            linear_constraints: 0,
            total_constraints: 1000,
        };

        let optimized = CircomMetrics {
            non_linear_constraints: 800,
            linear_constraints: 0,
            total_constraints: 800,
        };

        let reduction = optimized.reduction_from(&baseline);
        assert!((reduction - 0.2).abs() < 0.001); // 20% reduction
    }
}

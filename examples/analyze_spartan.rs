/// Example: Analyze a Spartan circuit file
///
/// Run with: cargo run --example analyze_spartan

use tig_circuit_tools::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Spartan Circuit Analysis Example ===\n");

    // First, generate a circuit to analyze
    println!("Generating a sample circuit...");
    let config = CircuitConfig::from_difficulty(2);
    let dag = generate_dag("analysis_example", &config);
    let spartan = dag_to_spartan(&dag);

    // Save it
    let filename = "sample_circuit.spartan.json";
    let json = serde_json::to_string_pretty(&spartan)?;
    std::fs::write(filename, json)?;
    println!("✅ Saved sample circuit to: {}\n", filename);

    // Now analyze it
    println!("Analyzing Spartan circuit...");
    let metrics = count_spartan_constraints(filename)?;

    println!("\n{}", metrics.display());

    println!("\nAdditional Insights:");
    println!("  - Total non-zero matrix entries: {}", metrics.total_nonzeros());
    println!("  - Average non-zeros per constraint: {:.2}", metrics.avg_nonzeros_per_constraint());
    println!("  - Matrix sparsity: {:.6}", metrics.sparsity_ratio());

    // Demonstrate comparison
    println!("\n=== Simulating Optimization ===");
    let baseline_metrics = metrics.clone();

    // Create a "fake" optimized version (30% reduction)
    let optimized_metrics = SpartanMetrics {
        total_constraints: (baseline_metrics.total_constraints as f64 * 0.7) as usize,
        total_variables: (baseline_metrics.total_variables as f64 * 0.7) as usize,
        public_inputs: baseline_metrics.public_inputs,
        matrix_a_nonzeros: (baseline_metrics.matrix_a_nonzeros as f64 * 0.7) as usize,
        matrix_b_nonzeros: (baseline_metrics.matrix_b_nonzeros as f64 * 0.7) as usize,
        matrix_c_nonzeros: (baseline_metrics.matrix_c_nonzeros as f64 * 0.7) as usize,
    };

    println!("\n{}", compare_circuits(&baseline_metrics, &optimized_metrics));

    // Clean up
    std::fs::remove_file(filename)?;
    println!("\n✅ Cleaned up temporary file");

    Ok(())
}

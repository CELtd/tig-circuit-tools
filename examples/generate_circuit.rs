/// Example: Generate a circuit from a seed
///
/// Run with: cargo run --example generate_circuit

use tig_circuit_tools::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Circuit Generation Example ===\n");

    // Create configuration for difficulty level 3
    let config = CircuitConfig::from_difficulty(3);
    println!("Config: {:?}\n", config);

    // Generate DAG from seed
    let seed = "example_seed_123";
    println!("Generating circuit from seed: {}", seed);
    let dag = generate_dag(seed, &config);

    println!("Generated DAG:");
    println!("  - Total nodes: {}", dag.nodes.len());
    println!("  - Input nodes: {}", dag.num_inputs);
    println!("  - Output nodes: {}", dag.num_outputs);
    println!("  - Total constraints: {}\n", dag.total_constraints());

    // Analyze the DAG
    let analysis = analyze_dag(&dag);
    println!("Analysis Results:");
    println!("{}\n", analysis.display());

    // Convert to Circom
    let circom_code = dag_to_circom(&dag);
    std::fs::write("example_circuit.circom", &circom_code)?;
    println!("✅ Saved Circom code to: example_circuit.circom");
    println!("   Lines of code: {}", circom_code.lines().count());

    // Convert to Spartan
    let spartan = dag_to_spartan(&dag);
    let spartan_json = serde_json::to_string_pretty(&spartan)?;
    std::fs::write("example_circuit.spartan.json", &spartan_json)?;
    println!("✅ Saved Spartan matrices to: example_circuit.spartan.json");

    // Verify parity
    println!("\nParity Check:");
    println!("  DAG constraints:     {}", analysis.baseline_constraints);
    println!("  Spartan constraints: {}", spartan.num_cons);
    if analysis.baseline_constraints == spartan.num_cons {
        println!("  ✨ PASSED: Constraint counts match!");
    } else {
        println!("  ⚠️  WARNING: Constraint counts differ!");
    }

    Ok(())
}

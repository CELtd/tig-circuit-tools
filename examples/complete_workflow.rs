/// Example: Complete workflow from generation to analysis
///
/// Run with: cargo run --example complete_workflow

use tig_circuit_tools::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Complete Circuit Workflow Example ===\n");

    // Step 1: Configure and Generate
    println!("Step 1: Generating Circuit");
    println!("{}", "-".repeat(50));
    let config = CircuitConfig::from_difficulty(2);
    let seed = "workflow_demo";
    let dag = generate_dag(seed, &config);

    println!("Generated circuit from seed: '{}'", seed);
    println!("  Nodes: {}", dag.nodes.len());
    println!("  Inputs: {}", dag.num_inputs);
    println!("  Outputs: {}", dag.num_outputs);
    println!("  Constraints: {}\n", dag.total_constraints());

    // Step 2: DAG Analysis
    println!("Step 2: Analyzing DAG Structure");
    println!("{}", "-".repeat(50));
    let analysis = analyze_dag(&dag);
    println!("Baseline Constraints: {}", analysis.baseline_constraints);
    println!("Removable Constraints:");
    println!("  - Alias operations:    {}", analysis.alias_removable);
    println!("  - Linear operations:   {}", analysis.linear_removable);
    println!("  - Algebraic (Pow5):    {}", analysis.algebraic_removable);
    println!("  - Total removable:     {}", analysis.total_removable());
    println!("  - Theoretical minimum: {}", analysis.optimized_constraints());
    println!("  - Max reduction:       {:.2}%\n", analysis.total_possible_reduction * 100.0);

    // Step 3: Convert to Formats
    println!("Step 3: Converting to Multiple Formats");
    println!("{}", "-".repeat(50));

    // Circom
    let circom = dag_to_circom(&dag);
    std::fs::write("workflow.circom", &circom)?;
    println!("✅ Circom: workflow.circom ({} lines)", circom.lines().count());

    // Spartan
    let spartan = dag_to_spartan(&dag);
    let spartan_json = serde_json::to_string_pretty(&spartan)?;
    std::fs::write("workflow.spartan.json", &spartan_json)?;
    println!("✅ Spartan: workflow.spartan.json ({} bytes)\n", spartan_json.len());

    // Step 4: Verify Parity
    println!("Step 4: Verifying Format Parity");
    println!("{}", "-".repeat(50));
    println!("DAG Analysis:      {} constraints", analysis.baseline_constraints);
    println!("Spartan Instance:  {} constraints", spartan.num_cons);

    if analysis.baseline_constraints == spartan.num_cons {
        println!("✨ PARITY CHECK PASSED\n");
    } else {
        println!("⚠️  PARITY CHECK FAILED\n");
    }

    // Step 5: Analyze Spartan Instance
    println!("Step 5: Analyzing Spartan Metrics");
    println!("{}", "-".repeat(50));
    let metrics = analyze_spartan_instance(&spartan);
    println!("Constraints: {}", metrics.total_constraints);
    println!("Variables:   {}", metrics.total_variables);
    println!("Inputs:      {}", metrics.public_inputs);
    println!("Matrix A non-zeros: {}", metrics.matrix_a_nonzeros);
    println!("Matrix B non-zeros: {}", metrics.matrix_b_nonzeros);
    println!("Matrix C non-zeros: {}", metrics.matrix_c_nonzeros);
    println!("Sparsity ratio:     {:.6}\n", metrics.sparsity_ratio());

    // Step 6: Demonstrate Determinism
    println!("Step 6: Verifying Determinism");
    println!("{}", "-".repeat(50));
    let dag2 = generate_dag(seed, &config);
    println!("Generated second DAG with same seed");
    println!("First DAG:  {} nodes, {} constraints", dag.nodes.len(), dag.total_constraints());
    println!("Second DAG: {} nodes, {} constraints", dag2.nodes.len(), dag2.total_constraints());

    if dag.nodes.len() == dag2.nodes.len() && dag.total_constraints() == dag2.total_constraints() {
        println!("✨ DETERMINISM VERIFIED\n");
    } else {
        println!("⚠️  DETERMINISM FAILED\n");
    }

    // Clean up
    println!("Cleaning up files...");
    std::fs::remove_file("workflow.circom")?;
    std::fs::remove_file("workflow.spartan.json")?;
    println!("✅ Done!\n");

    Ok(())
}

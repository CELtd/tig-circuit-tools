use clap::{Parser, Subcommand};
use indicatif::ProgressBar;
use std::fs;
use tig_circuit_tools::*;

#[derive(Parser)]
#[command(name = "tig-tool")]
#[command(version, about = "TIG Circuit Tools - Generate and analyze R1CS circuits", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a circuit from a seed
    Generate {
        #[arg(short, long)]
        seed: String,

        #[arg(short, long, default_value_t = 1)]
        difficulty: u32,

        #[arg(short, long, default_value = "challenge.circom")]
        output: String,
    },

    /// Analyze a Spartan JSON file
    AnalyzeSpartan {
        #[arg(short, long)]
        file: String,
    },

    /// Count constraints in a Circom file
    CountCircom {
        #[arg(short, long)]
        file: String,

        #[arg(short, long, default_value = "O0")]
        opt_level: String,
    },

    /// Run calibration analysis
    Calibrate {
        #[arg(short, long)]
        difficulty: u32,

        #[arg(short, long, default_value_t = 20)]
        samples: usize,
    },

    /// Run benchmark across difficulty levels
    Benchmark {
        #[arg(short, long, default_value_t = 10)]
        max_difficulty: u32,

        #[arg(short, long, default_value_t = 3)]
        samples: usize,
    },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Generate { seed, difficulty, output } => {
            run_generate(seed, *difficulty, output);
        }
        Commands::AnalyzeSpartan { file } => {
            run_analyze_spartan(file);
        }
        Commands::CountCircom { file, opt_level } => {
            run_count_circom(file, opt_level);
        }
        Commands::Calibrate { difficulty, samples } => {
            run_calibrate(*difficulty, *samples);
        }
        Commands::Benchmark { max_difficulty, samples } => {
            run_benchmark(*max_difficulty, *samples);
        }
    }
}

fn run_generate(seed: &str, difficulty: u32, output: &str) {
    println!("🔹 Generating Challenge (Dual-Head Mode)...");
    let config = CircuitConfig::from_difficulty(difficulty);

    // Generate DAG
    let dag = generate_dag(seed, &config);

    // Analyze DAG
    let analysis = analyze_dag(&dag);

    // Convert to Circom
    let circom_code = dag_to_circom(&dag);
    fs::write(output, &circom_code).expect("Failed to write .circom file");
    println!("✅ Saved Circom reference to {}", output);

    // Convert to Spartan
    let spartan = dag_to_spartan(&dag);
    let spartan_filename = format!("{}.spartan.json", output);
    let spartan_json = serde_json::to_string_pretty(&spartan).unwrap();
    fs::write(&spartan_filename, spartan_json).expect("Failed to write Spartan file");
    println!("✅ Saved Spartan matrices to {}", spartan_filename);
    println!("   Constraints: {}", spartan.num_cons);

    // Report
    println!("\n🔮 THEORETICAL ORACLE REPORT");
    println!("   Baseline Constraints: {}", analysis.baseline_constraints);
    println!("   Spartan Constraints:  {}", spartan.num_cons);

    // Parity Check
    if analysis.baseline_constraints != spartan.num_cons {
        println!(
            "⚠️  WARNING: Constraint mismatch! Expected {} but got {}.",
            analysis.baseline_constraints, spartan.num_cons
        );
    } else {
        println!("✨ PARITY CHECK PASSED: Circom and Spartan models define identical difficulty.");
    }

    println!("\n   Alias Removable:      {}", analysis.alias_removable);
    println!("   Linear Removable:     {}", analysis.linear_removable);
    println!("   Algebraic Removable:  {}", analysis.algebraic_removable);
    println!(
        "   Total Possible Reduction: {:.2}%",
        analysis.total_possible_reduction * 100.0
    );
}

fn run_analyze_spartan(file: &str) {
    println!("📊 Analyzing Spartan circuit: {}", file);

    match count_spartan_constraints(file) {
        Ok(metrics) => {
            println!("\n{}", metrics.display());
        }
        Err(e) => {
            eprintln!("❌ Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_count_circom(file: &str, opt_level_str: &str) {
    let opt_level = match opt_level_str.to_uppercase().as_str() {
        "O0" => OptLevel::O0,
        "O1" => OptLevel::O1,
        "O2" => OptLevel::O2,
        _ => {
            eprintln!("❌ Invalid optimization level: {}. Use O0, O1, or O2.", opt_level_str);
            std::process::exit(1);
        }
    };

    println!("📊 Counting constraints in: {} (optimization: {})", file, opt_level_str);

    match count_circom_constraints(file, opt_level) {
        Ok(metrics) => {
            println!("\n{}", metrics.display());
        }
        Err(e) => {
            eprintln!("❌ Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_calibrate(difficulty: u32, samples: usize) {
    println!("🔹 Running Calibration (Tier {}, Samples: {})", difficulty, samples);
    let config = CircuitConfig::from_difficulty(difficulty);

    let mut raw_counts = Vec::new();
    let mut o1_reductions = Vec::new();
    let mut o2_reductions = Vec::new();
    let mut potentials = Vec::new();

    let bar = ProgressBar::new(samples as u64);

    for i in 0..samples {
        let seed = format!("calib_{}_{}", difficulty, i);

        // Generate and analyze
        let dag = generate_dag(&seed, &config);
        let analysis = analyze_dag(&dag);
        let circom = dag_to_circom(&dag);

        // Write temporary file
        let temp_file = format!("temp_calib_{}.circom", i);
        fs::write(&temp_file, circom).expect("Failed to write temp file");

        // Compile with different optimization levels
        let o0 = count_circom_constraints(&temp_file, OptLevel::O0).unwrap_or_else(|_| CircomMetrics {
            non_linear_constraints: 0,
            linear_constraints: 0,
            total_constraints: 0,
        });
        let o1 = count_circom_constraints(&temp_file, OptLevel::O1).unwrap_or_else(|_| CircomMetrics {
            non_linear_constraints: 0,
            linear_constraints: 0,
            total_constraints: 0,
        });
        let o2 = count_circom_constraints(&temp_file, OptLevel::O2).unwrap_or_else(|_| CircomMetrics {
            non_linear_constraints: 0,
            linear_constraints: 0,
            total_constraints: 0,
        });

        // Clean up
        let _ = fs::remove_file(&temp_file);
        let _ = fs::remove_file(temp_file.replace(".circom", ".r1cs"));
        let _ = fs::remove_file(temp_file.replace(".circom", ".sym"));

        let baseline = o0.total_constraints as f64;
        let red_o1 = o1.reduction_from(&o0);
        let red_o2 = o2.reduction_from(&o0);

        raw_counts.push(baseline);
        o1_reductions.push(red_o1);
        o2_reductions.push(red_o2);
        potentials.push(analysis.total_possible_reduction);

        bar.inc(1);
    }

    bar.finish();

    // Calculate statistics
    let mean = |v: &Vec<f64>| v.iter().sum::<f64>() / v.len() as f64;
    let stddev = |v: &Vec<f64>| {
        let m = mean(v);
        (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    };

    println!("\n📈 CALIBRATION RESULTS (Difficulty {})", difficulty);
    println!("{}", "-".repeat(60));
    println!("Raw Constraint Count:     {:.1} ± {:.1}", mean(&raw_counts), stddev(&raw_counts));
    println!("O1 Reduction:             {:.2}% ± {:.2}%", mean(&o1_reductions) * 100.0, stddev(&o1_reductions) * 100.0);
    println!("O2 Reduction:             {:.2}% ± {:.2}%", mean(&o2_reductions) * 100.0, stddev(&o2_reductions) * 100.0);
    println!("Theoretical Potential:    {:.2}% ± {:.2}%", mean(&potentials) * 100.0, stddev(&potentials) * 100.0);
}

fn run_benchmark(max_difficulty: u32, samples: usize) {
    println!("🔹 Running Benchmark (Max Difficulty: {}, Samples: {})", max_difficulty, samples);
    println!("{}", "=".repeat(85));
    println!("{:<6} {:<12} {:<12} {:<12} {:<12} {:<12} {:<12}", "Tier", "Raw", "O1", "O2", "O1 Red%", "O2 Red%", "Potential%");
    println!("{}", "-".repeat(85));

    for difficulty in 1..=max_difficulty {
        let config = CircuitConfig::from_difficulty(difficulty);
        let mut raw_sum = 0.0;
        let mut o1_red_sum = 0.0;
        let mut o2_red_sum = 0.0;
        let mut pot_sum = 0.0;

        for i in 0..samples {
            let seed = format!("bench_{}_{}", difficulty, i);
            let dag = generate_dag(&seed, &config);
            let analysis = analyze_dag(&dag);
            let circom = dag_to_circom(&dag);

            let temp_file = format!("temp_bench_{}_{}.circom", difficulty, i);
            fs::write(&temp_file, circom).ok();

            if let Ok(o0) = count_circom_constraints(&temp_file, OptLevel::O0) {
                if let Ok(o1) = count_circom_constraints(&temp_file, OptLevel::O1) {
                    if let Ok(o2) = count_circom_constraints(&temp_file, OptLevel::O2) {
                        raw_sum += o0.total_constraints as f64;
                        o1_red_sum += o1.reduction_from(&o0);
                        o2_red_sum += o2.reduction_from(&o0);
                        pot_sum += analysis.total_possible_reduction;
                    }
                }
            }

            let _ = fs::remove_file(&temp_file);
            let _ = fs::remove_file(temp_file.replace(".circom", ".r1cs"));
            let _ = fs::remove_file(temp_file.replace(".circom", ".sym"));
        }

        let s = samples as f64;
        println!(
            "{:<6} {:<12.0} {:<12.2} {:<12.2} {:<12.2} {:<12.2} {:<12.2}",
            difficulty,
            raw_sum / s,
            (raw_sum / s) * (1.0 - (o1_red_sum / s)),
            (raw_sum / s) * (1.0 - (o2_red_sum / s)),
            (o1_red_sum / s) * 100.0,
            (o2_red_sum / s) * 100.0,
            (pot_sum / s) * 100.0
        );
    }
    println!("{}", "-".repeat(85));
}

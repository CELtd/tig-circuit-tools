# TIG Circuit Tools

A modular Rust library and CLI for generating, converting, and analyzing R1CS circuits for Zero-Knowledge proof systems.

Built by **CryptoEconLab** for The Innovation Game (TIG) - a Zero-Knowledge proof challenge competition.

## Features

- **DAG Generation**: Generate random, deterministic circuit DAGs from seeds
- **Dual Format Support**: Convert DAGs to both Circom and Spartan R1CS formats
- **Semantic Analysis**: Identify optimization opportunities in circuits
- **Constraint Counting**: Analyze Spartan JSON and Circom files
- **Calibration Tools**: Statistical analysis of circuit difficulty
- **Benchmarking**: Performance testing across difficulty tiers

## Prerequisites

### Required

- **Rust** (1.70 or later): Install from [rustup.rs](https://rustup.rs/)
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

### Optional

- **Circom Compiler**: Required only for `count-circom` command and benchmarking features
  - Install from [Circom documentation](https://docs.circom.io/getting-started/installation/)
  ```bash
  # Example installation (check official docs for latest method)
  cargo install --git https://github.com/iden3/circom.git --bin circom
  ```

## Installation

### Option 1: Install from Source (Recommended)

```bash
# Clone the repository
git clone https://github.com/cryptoeconlab/tig-circuit-tools
cd tig-circuit-tools

# Build and install the CLI tool
cargo install --path .

# Or just build without installing
cargo build --release
# Binary will be at: ./target/release/tig-tool
```

### Option 2: Use as Library Dependency

Add to your `Cargo.toml`:

```toml
[dependencies]
tig-circuit-tools = { git = "https://github.com/cryptoeconlab/tig-circuit-tools" }
```

Or if published to crates.io:

```toml
[dependencies]
tig-circuit-tools = "0.1.0"
```

### Option 3: Library-Only (No CLI)

To use only the library without CLI dependencies:

```toml
[dependencies]
tig-circuit-tools = { version = "0.1.0", default-features = false }
```

## Usage

### Command-Line Interface (CLI)

#### 1. Generate a Circuit

Generate a circuit from a seed and difficulty level:

```bash
tig-tool generate --seed "my_seed_123" --difficulty 5 --output challenge.circom
```

**Output:**
- `challenge.circom` - Human-readable Circom circuit code
- `challenge.circom.spartan.json` - Spartan R1CS matrices (JSON format)

**Example output:**
```
🔹 Generating Challenge (Dual-Head Mode)...
✅ Saved Circom reference to challenge.circom
✅ Saved Spartan matrices to challenge.circom.spartan.json
   Constraints: 5023

🔮 THEORETICAL ORACLE REPORT
   Baseline Constraints: 5023
   Spartan Constraints:  5023
✨ PARITY CHECK PASSED: Circom and Spartan models define identical difficulty.

   Alias Removable:      753
   Linear Removable:     1005
   Algebraic Removable:  1504
   Total Possible Reduction: 64.89%
```

#### 2. Analyze a Spartan Circuit

Extract metrics from a Spartan JSON file:

```bash
tig-tool analyze-spartan --file challenge.circom.spartan.json
```

**Example output:**
```
📊 Analyzing Spartan circuit: challenge.circom.spartan.json

Spartan Metrics:
- Total Constraints: 5023
- Total Variables: 5842
- Public Inputs: 892
- Matrix A Non-zeros: 10234
- Matrix B Non-zeros: 10234
- Matrix C Non-zeros: 10234
- Total Non-zeros: 30702
- Avg Non-zeros/Constraint: 6.11
- Sparsity Ratio: 0.000105
```

#### 3. Count Circom Constraints

Run the Circom compiler to count constraints (requires `circom` installed):

```bash
tig-tool count-circom --file challenge.circom --opt-level O2
```

**Optimization levels:**
- `O0` - No optimization (baseline)
- `O1` - Basic optimizations
- `O2` - Aggressive optimizations (recommended)

**Example output:**
```
📊 Counting constraints in: challenge.circom (optimization: O2)

Circom Metrics:
- Non-linear Constraints: 3521
- Linear Constraints: 0
- Total Constraints: 3521
```

#### 4. Run Calibration

Test consistency across multiple circuits with the same difficulty:

```bash
tig-tool calibrate --difficulty 5 --samples 20
```

This generates 20 circuits with different seeds but same difficulty, measuring:
- Raw constraint count variance
- O1/O2 optimization consistency
- Theoretical optimization potential

**Example output:**
```
🔹 Running Calibration (Tier 5, Samples: 20)
████████████████████ 20/20

📈 CALIBRATION RESULTS (Difficulty 5)
------------------------------------------------------------
Raw Constraint Count:     5012.3 ± 48.2
O1 Reduction:             18.42% ± 2.31%
O2 Reduction:             29.87% ± 3.45%
Theoretical Potential:    64.23% ± 1.89%
```

#### 5. Run Benchmark

Test across multiple difficulty tiers:

```bash
tig-tool benchmark --max-difficulty 10 --samples 3
```

**Example output:**
```
🔹 Running Benchmark (Max Difficulty: 10, Samples: 3)
=====================================================================================
Tier   Raw          O1           O2           O1 Red%      O2 Red%      Potential%
-------------------------------------------------------------------------------------
1      1024         834          716          18.55        30.08        65.23
2      2048         1668         1432         18.55        30.08        64.87
...
10     10240        8340         7160         18.55        30.08        65.11
-------------------------------------------------------------------------------------
```

### Library Usage

#### Basic Example: Generate and Analyze

```rust
use tig_circuit_tools::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Configure and generate DAG
    let config = CircuitConfig::from_difficulty(5);
    let dag = generate_dag("my_seed", &config);

    println!("Generated circuit with {} nodes", dag.nodes.len());
    println!("Total constraints: {}", dag.total_constraints());

    // Step 2: Analyze the DAG
    let analysis = analyze_dag(&dag);
    println!("Baseline: {}", analysis.baseline_constraints);
    println!("Removable: {} ({:.2}%)",
        analysis.total_removable(),
        analysis.total_possible_reduction * 100.0
    );

    // Step 3: Convert to Circom
    let circom_code = dag_to_circom(&dag);
    std::fs::write("circuit.circom", circom_code)?;

    // Step 4: Convert to Spartan
    let spartan = dag_to_spartan(&dag);
    let json = serde_json::to_string_pretty(&spartan)?;
    std::fs::write("circuit.spartan.json", json)?;

    // Step 5: Verify parity
    assert_eq!(analysis.baseline_constraints, spartan.num_cons);
    println!("✅ Parity check passed!");

    Ok(())
}
```

#### Advanced Example: Custom Configuration

```rust
use tig_circuit_tools::*;

// Create custom configuration
let config = CircuitConfig::new(
    2000,   // num_constraints
    0.30,   // redundancy_ratio
    0.20,   // power_map_ratio (Pow5 frequency)
    0.10,   // alias_ratio (alias trap frequency)
    0.15,   // linear_ratio (linear trap frequency)
);

let dag = generate_dag("custom_seed", &config);
let analysis = analyze_dag(&dag);
```

#### Example: Analyze Existing Spartan File

```rust
use tig_circuit_tools::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load and analyze existing Spartan circuit
    let metrics = count_spartan_constraints("circuit.spartan.json")?;

    println!("Constraints: {}", metrics.total_constraints);
    println!("Variables: {}", metrics.total_variables);
    println!("Sparsity: {:.6}", metrics.sparsity_ratio());

    Ok(())
}
```

#### Example: Compare Circuits

```rust
use tig_circuit_tools::*;

// Generate baseline
let config = CircuitConfig::from_difficulty(5);
let dag = generate_dag("seed1", &config);
let baseline = dag_to_spartan(&dag);
let baseline_metrics = analyze_spartan_instance(&baseline);

// Simulate optimized version (in practice, you'd load an optimized circuit)
let optimized_metrics = count_spartan_constraints("optimized.spartan.json")?;

// Compare
let comparison = compare_circuits(&baseline_metrics, &optimized_metrics);
println!("{}", comparison);
```

## Running Examples

The repository includes several complete examples:

```bash
# Basic circuit generation
cargo run --example generate_circuit

# Spartan circuit analysis
cargo run --example analyze_spartan

# Complete workflow demonstration
cargo run --example complete_workflow
```

## Architecture

The library is organized into three main modules:

### `dag` Module - Circuit Generation

Core DAG generation functionality:
- `CircuitConfig` - Generation parameters
- `Node`, `OpType` - Circuit node definitions
- `generate_dag()` - Deterministic DAG generation

### `converters` Module - Format Conversion

Convert DAGs to different formats:
- `dag_to_circom()` - Generate Circom code
- `dag_to_spartan()` - Generate R1CS matrices
- `SpartanInstance` - R1CS representation

### `analysis` Module - Circuit Analysis

Analyze circuits in various formats:
- `analyze_dag()` - Semantic analysis on DAG
- `count_spartan_constraints()` - Parse Spartan JSON
- `count_circom_constraints()` - Run Circom compiler

## How It Works

### Deterministic Generation

Circuits are generated deterministically from seeds:

```
Seed → SHA256 → ChaCha20Rng → Deterministic operations
```

This enables seed-based challenge protocols without transmitting circuit files.

### Backward DAG Construction

The DAG grows backwards from outputs to inputs:

1. Create 1-3 output nodes
2. Expand backwards using frontier queue
3. Randomly select operations based on configuration ratios
4. Finalize undefined nodes as inputs

This naturally creates acyclic graphs without cycle detection.

### Dual Rendering

The same DAG renders to two mathematically isomorphic formats:

- **Circom** (BN254): Human-readable, calibratable with Circom compiler
- **Spartan** (Curve25519): Production format for transparent proofs

Both produce **identical constraint counts**.

### Optimization Traps

The generator intentionally injects suboptimal structures:

| Operation | Cost | Optimization Potential |
|-----------|------|------------------------|
| **Alias** (`A = B`) | 1 constraint | Removable via substitution |
| **Linear Scaling** (`A = k × B`) | 1 constraint | Removable via folding |
| **Pow5** (`x^5`) | 3 constraints | Reducible to ~1 with advanced techniques |

These create the "optimization game" where competitors reduce constraints.

## Development

### Run Tests

```bash
# All tests
cargo test

# With output
cargo test -- --nocapture

# Specific test
cargo test test_determinism
```

### Build Documentation

```bash
cargo doc --open
```

### Run Clippy

```bash
cargo clippy --all-targets --all-features
```

### Format Code

```bash
cargo fmt
```

## Troubleshooting

### "circom: command not found"

The `count-circom`, `calibrate`, and `benchmark` commands require the Circom compiler. Install it from [docs.circom.io](https://docs.circom.io/getting-started/installation/).

To use the library without Circom:
- Use only `generate`, `analyze-spartan` commands
- Or install without CLI: `tig-circuit-tools = { version = "0.1.0", default-features = false }`

### Build Errors

Ensure you have:
- Rust 1.70 or later: `rustc --version`
- Updated dependencies: `cargo update`

### Performance Issues

Use release mode for production:
```bash
cargo build --release
cargo run --release --bin tig-tool -- generate ...
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Acknowledgments

Developed by **CryptoEconLab** for The Innovation Game (TIG).

## Contact

For questions or support, please open an issue on GitHub.

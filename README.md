# TIG Circuit Tools

A Rust library and CLI for generating, converting, and analyzing R1CS circuits for the TIG ZK Challenge.

Built by **CryptoEconLab** for [The Innovation Game (TIG)](https://github.com/tig-foundation/tig-monorepo).

## What This Library Does

This library is the **random circuit instance generator** and **witness solver** for the TIG ZK Challenge. The challenge asks participants to optimize R1CS circuits — reducing constraint counts while preserving functional equivalence. This library:

1. **Generates random R1CS circuits** deterministically from a cryptographic seed and difficulty parameter
2. **Converts** the internal DAG representation to Spartan R1CS format (for ZK proving) and Circom format (for analysis)
3. **Computes witnesses** for any circuit — either from the DAG (`compute_witness`) or from raw R1CS matrices (`solve_witness_from_r1cs`)
4. **Optimizes** circuits via `remove_aliases` — a baseline optimizer that detects and removes alias constraints directly from R1CS matrices
5. **Analyzes** circuits to measure optimization potential

### How It Fits in the Pipeline

```
seed + difficulty
       |
       v
  tig-circuit-tools                    <-- THIS LIBRARY
  +------------------------------+
  | generate_dag()               |  seed -> SHA256 -> ChaCha20 PRNG -> backward BFS -> DAG
  | dag_to_spartan()             |  DAG -> sparse R1CS matrices (A, B, C)
  | compute_witness()            |  DAG + inputs -> (private_vars, public_io)
  | solve_witness_from_r1cs()    |  R1CS + inputs -> (private_vars, public_io)  [no DAG needed]
  | remove_aliases()             |  R1CS -> optimized R1CS (alias constraints removed)
  +--------------+---------------+
                 |
                 v
  tig-monorepo (zk.rs)                 <-- CHALLENGE PROTOCOL
  +------------------------------+
  | generate_instance()          |  Creates Challenge with baseline circuit C0
  | solve_challenge()            |  Participant optimizes C0->C*, proofs generated automatically
  | verify_solution()            |  Verifier checks K*<K0, output equivalence, proof validity
  +------------------------------+
```

## Public API

### Core Functions

```rust
use tig_circuit_tools::*;
use curve25519_dalek::scalar::Scalar;

// Generate a circuit configuration from a difficulty scalar
// delta=1 -> ~1000 constraints, delta=2 -> ~2000, etc.
let config = CircuitConfig::from_difficulty(1);

// Deterministically generate a DAG from seed + config
let dag = generate_dag("my_seed", &config);

// Convert to Spartan R1CS (for ZK proving with libspartan)
let spartan_instance = dag_to_spartan(&dag);

// --- Witness computation (two methods) ---

// Method 1: From DAG (used for baseline circuit C0)
let (vars, public_io) = compute_witness(&dag, &input_scalars);

// Method 2: From raw R1CS matrices (used for optimized circuit C*)
// No DAG needed — solves the witness by fixed-point constraint propagation
let (vars, public_io) = solve_witness_from_r1cs(
    &spartan_instance,
    dag.num_outputs,     // how many public I/O slots are outputs
    &input_scalars,      // known circuit inputs
).unwrap();

// Both methods produce identical results for the same circuit and inputs.

// Convert to Circom (for analysis/calibration only, uses BN254 field)
let circom_code = dag_to_circom(&dag);

// Analyze optimization potential
let analysis = analyze_dag(&dag);
```

### The R1CS Witness Solver

`solve_witness_from_r1cs` is the key function that enables the participant API. It takes raw R1CS matrices and circuit inputs, and computes all intermediate values (the witness) without needing the computation graph.

**How it works:**

1. Allocates the z-vector: `[private_vars | 1 | outputs... | inputs...]`
2. Fills known values: the constant 1 and the circuit inputs
3. Iterates over constraint rows. For each row with exactly one unknown variable, solves the linear equation:
   ```
   x = (C_known - A_known * B_known) / (a_j * B_known + b_j * A_known - c_j)
   ```
4. Repeats until convergence (no new variables solved in a full pass)
5. If all variables are solved: success. If stuck: the circuit is invalid.

The solver converges for any R1CS that represents a deterministic forward computation — which includes all DAG-generated circuits and well-formed optimized circuits. Solver failure is a built-in well-formedness check: a circuit that the solver can't propagate through doesn't compute a deterministic function and would fail verification regardless.

**Signature:**

```rust
pub fn solve_witness_from_r1cs(
    instance: &SpartanInstance,  // R1CS matrices (A, B, C) and dimensions
    num_outputs: usize,          // outputs are unknown, placed first in public I/O
    circuit_inputs: &[Scalar],   // known input values (e.g. x_eval)
) -> Result<(Vec<Scalar>, Vec<Scalar>), WitnessError>
// Returns (private_vars, public_io) same as compute_witness
```

### Baseline Optimizer: `remove_aliases`

`remove_aliases` is a reference optimizer that removes alias constraints from raw R1CS matrices. It demonstrates how participants can optimize circuits without access to the DAG.

**How it works:**

1. Scans each constraint row for the alias pattern: `A = [(col_out, 1)]`, `B = [(const, 1)]`, `C = [(col_src, 1)]`
2. Builds a substitution map: `col_out → col_src` (or reversed if `col_out` is a public I/O)
3. Resolves substitution chains (a → b → c flattened to a → c)
4. Applies substitutions across all surviving constraints, merging duplicate columns
5. Compacts private variable columns so `num_vars` shrinks

**Usage:**

```rust
use tig_circuit_tools::*;

let config = CircuitConfig::from_difficulty(1);
let dag = generate_dag("my_seed", &config);
let c0 = dag_to_spartan(&dag);

let c_star = remove_aliases(&c0);
// c_star.num_cons < c0.num_cons  (~13% reduction from aliases alone)
```

**Signature:**

```rust
pub fn remove_aliases(instance: &SpartanInstance) -> SpartanInstance
```

Pure function. If no aliases are found, returns a clone. The optimized circuit is compatible with `solve_witness_from_r1cs` — the solver converges on the compacted circuit.

### Key Types

```rust
// Circuit configuration
pub struct CircuitConfig {
    pub num_constraints: usize,   // target constraint count (delta * 1000)
    pub redundancy_ratio: f64,    // shared subexpression density (0.25)
    pub power_map_ratio: f64,     // x^5 S-box frequency (0.15)
    pub alias_ratio: f64,         // identity op frequency (0.15)
    pub linear_ratio: f64,        // constant scaling frequency (0.20)
}

// Directed acyclic graph of circuit operations
pub struct DAG {
    pub nodes: Vec<Node>,
    pub num_inputs: usize,
    pub num_outputs: usize,
}

// Sparse R1CS instance for libspartan
pub struct SpartanInstance {
    pub num_cons: usize,
    pub num_vars: usize,
    pub num_inputs: usize,
    pub A: R1CSMatrix,  // Vec<(row, col, [u8; 32])>
    pub B: R1CSMatrix,
    pub C: R1CSMatrix,
}

// Witness solver errors
pub enum WitnessError {
    InvalidInputs { expected: usize, got: usize },
    SolverStuck { solved: usize, total: usize },
}
```

### Operation Types and Their Optimization Potential

The generator injects intentional "optimization traps" alongside core arithmetic:

| Operation | Constraints | Purpose |
|-----------|------------|---------|
| `Add(l, r)` | 1 | Core arithmetic: `out = l + r` |
| `Mul(l, r)` | 1 | Core arithmetic: `out = l * r` |
| `Alias(src)` | 1 | **Trivially removable** via substitution (`out = src`) |
| `Scale(src, k)` | 1 | **Removable** via constant folding (`out = k * src`) |
| `Pow5(src)` | 3 | **Reducible** algebraic power map (`out = src^5`, like Poseidon S-boxes) |

With default ratios, roughly **50% of constraints are intentionally removable**.

## Using as a Dependency

In `Cargo.toml`:

```toml
[dependencies]
# Library only (no CLI dependencies)
tig-circuit-tools = { git = "https://github.com/CELtd/tig-circuit-tools", default-features = false }
```

The `tig-monorepo` (branch `zk`) uses this library in `tig-challenges/src/zk.rs` for circuit instance generation and witness computation.

## CLI Tool

Build with: `cargo build --release`

### Generate a Circuit

```bash
tig-tool generate --seed "my_seed_123" --difficulty 5 --output challenge.circom
```

Outputs:
- `challenge.circom` -- Circom source (for analysis with `circom --O0/O1/O2`)
- `challenge.circom.spartan.json` -- Spartan R1CS matrices (for ZK proving)

### Analyze a Spartan Circuit

```bash
tig-tool analyze-spartan --file challenge.circom.spartan.json
```

### Count Circom Constraints (requires `circom` installed)

```bash
tig-tool count-circom --file challenge.circom --opt-level O2
```

### Calibrate Difficulty Consistency

```bash
tig-tool calibrate --difficulty 5 --samples 20
```

### Benchmark Across Tiers

```bash
tig-tool benchmark --max-difficulty 10 --samples 3
```

## Tests

```bash
# Run all 22 tests (< 5 seconds)
cargo test

# With output
cargo test -- --nocapture
```

### What the Tests Validate

| Test | What it proves |
|------|---------------|
| `test_deterministic_generation` | Same seed produces identical DAG every time |
| `test_different_seeds_produce_different_dags` | Different seeds produce different circuits |
| `test_constraint_count_matches_dag` | DAG constraint count == Spartan R1CS row count |
| `test_column_bounds` | All R1CS matrix entries within valid dimensions |
| `test_compute_witness_basic` | Witness has correct dimensions |
| **`test_circuit_satisfiability`** | **(A*z)*(B*z)=C*z** verified via `libspartan::Instance::is_sat()` across **5 seeds x 3 difficulties** |
| **`test_solve_witness_from_r1cs`** | R1CS solver produces **byte-identical** results to DAG-based `compute_witness` across **5 seeds x 3 difficulties** |
| **`test_remove_aliases_reduces_constraints`** | Alias removal matches expected count from DAG analysis, optimized circuit passes `is_sat()` and produces identical outputs across **5 seeds x 3 difficulties** |

## Architecture

```
src/
+-- lib.rs                  <- Crate root, re-exports, integration tests
+-- dag/
|   +-- config.rs           <- CircuitConfig, from_difficulty()
|   +-- generator.rs        <- generate_dag(), DAG struct, backward BFS algorithm
|   +-- node.rs             <- Node, OpType (Add, Mul, Alias, Scale, Pow5)
+-- converters/
|   +-- circom.rs           <- dag_to_circom() -- Circom 2.0.0 output
|   +-- spartan.rs          <- dag_to_spartan(), compute_witness(), solve_witness_from_r1cs(), remove_aliases()
+-- analysis/
|   +-- dag_analysis.rs     <- analyze_dag() -- optimization potential report
|   +-- spartan_tools.rs    <- Spartan instance metrics and comparison
|   +-- circom_tools.rs     <- Circom compiler integration (requires circom binary)
+-- bin/
    +-- tig-tool.rs         <- CLI entry point
```

### Key Design Decisions

- **Backward BFS construction**: DAG grows from outputs (low IDs) to inputs (high IDs). Evaluating in reverse ID order gives correct topological forward order.
- **Curve25519 scalar field**: Matches libspartan's native field. Circom output uses BN254 but is only for analysis -- not for proving.
- **Deterministic PRNG**: `SHA256(seed) -> ChaCha20Rng` ensures any party can reproduce the exact same circuit from the same seed.
- **z-vector layout**: `[private_vars | 1 | outputs... | inputs...]` following libspartan convention.
- **Fixed-point R1CS solver**: Enables witness computation from raw R1CS matrices without the DAG, making it possible for participants to work at the R1CS level without writing custom witness generators.

## Prerequisites

- **Rust** 1.70+ -- install from [rustup.rs](https://rustup.rs/)
- **Circom** (optional) -- only needed for `count-circom`, `calibrate`, `benchmark` commands. Install from [docs.circom.io](https://docs.circom.io/getting-started/installation/).

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Acknowledgments

Developed by **CryptoEconLab** for The Innovation Game (TIG).

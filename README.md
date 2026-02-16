# TIG Circuit Tools

A Rust library and CLI for generating, converting, and analyzing R1CS circuits for the TIG ZK Challenge.

Built by **CryptoEconLab** for [The Innovation Game (TIG)](https://github.com/tig-foundation/tig-monorepo).

> **Full pipeline documentation**: See [ZK_CHALLENGE_DOCUMENTATION.md](./ZK_CHALLENGE_DOCUMENTATION.md) for a comprehensive explanation of how this library fits into the ZK Challenge, including the verification protocol, data structures, testing, and performance profile.

## What This Library Does

This library is the **random circuit instance generator** for the TIG ZK Challenge. The challenge asks participants to optimize R1CS circuits — reducing constraint counts while preserving functional equivalence. This library:

1. **Generates random R1CS circuits** deterministically from a cryptographic seed and difficulty parameter
2. **Converts** the internal DAG representation to Spartan R1CS format (for ZK proving) and Circom format (for analysis)
3. **Computes witnesses** for generated circuits, producing values that satisfy the R1CS constraints
4. **Analyzes** circuits to measure optimization potential

### How It Fits in the Pipeline

```
seed + difficulty
       │
       ▼
  tig-circuit-tools              ← THIS LIBRARY
  ┌──────────────────────┐
  │ generate_dag()       │  seed → SHA256 → ChaCha20 PRNG → backward BFS → DAG
  │ dag_to_spartan()     │  DAG → sparse R1CS matrices (A, B, C)
  │ compute_witness()    │  DAG + inputs → (private_vars, public_io)
  └──────────┬───────────┘
             │
             ▼
  tig-monorepo (zk.rs)          ← CHALLENGE PROTOCOL
  ┌──────────────────────┐
  │ generate_instance()  │  Creates Challenge with baseline circuit C⁰
  │ solve_challenge()    │  Participant optimizes C⁰→C*, generates Spartan SNARK proofs
  │ verify_solution()    │  Verifier checks K*<K⁰, output equivalence, proof validity
  └──────────────────────┘
```

## Public API

### Core Functions

```rust
use tig_circuit_tools::*;

// Generate a circuit configuration from a difficulty scalar
// delta=1 → ~1000 constraints, delta=2 → ~2000, etc.
let config = CircuitConfig::from_difficulty(1);

// Deterministically generate a DAG from seed + config
let dag = generate_dag("my_seed", &config);

// Convert to Spartan R1CS (for ZK proving with libspartan)
let spartan_instance = dag_to_spartan(&dag);

// Compute witness: given input scalars, evaluate the circuit
// Returns (private_vars, public_io) ready for libspartan
let (vars, public_io) = compute_witness(&dag, &input_scalars);

// Convert to Circom (for analysis/calibration only, uses BN254 field)
let circom_code = dag_to_circom(&dag);

// Analyze optimization potential
let analysis = analyze_dag(&dag);
```

### Key Types

```rust
// Circuit configuration
pub struct CircuitConfig {
    pub num_constraints: usize,   // target constraint count (delta * 1000)
    pub redundancy_ratio: f64,    // shared subexpression density (0.25)
    pub power_map_ratio: f64,     // x⁵ S-box frequency (0.15)
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
```

### Operation Types and Their Optimization Potential

The generator injects intentional "optimization traps" alongside core arithmetic:

| Operation | Constraints | Purpose |
|-----------|------------|---------|
| `Add(l, r)` | 1 | Core arithmetic: `out = l + r` |
| `Mul(l, r)` | 1 | Core arithmetic: `out = l * r` |
| `Alias(src)` | 1 | **Trivially removable** via substitution (`out = src`) |
| `Scale(src, k)` | 1 | **Removable** via constant folding (`out = k * src`) |
| `Pow5(src)` | 3 | **Reducible** algebraic power map (`out = src⁵`, like Poseidon S-boxes) |

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
- `challenge.circom` — Circom source (for analysis with `circom --O0/O1/O2`)
- `challenge.circom.spartan.json` — Spartan R1CS matrices (for ZK proving)

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
# Run all 20 tests (< 1 second)
cargo test

# With output
cargo test -- --nocapture
```

### What the Tests Validate

| Test | What it proves |
|------|---------------|
| `test_deterministic_generation` | Same seed → identical DAG every time |
| `test_different_seeds_produce_different_dags` | Different seeds produce different circuits |
| `test_constraint_count_matches_dag` | DAG constraint count == Spartan R1CS row count |
| `test_column_bounds` | All R1CS matrix entries within valid dimensions |
| `test_compute_witness_basic` | Witness has correct dimensions |
| **`test_circuit_satisfiability`** | **(A*z)*(B*z)=C*z** verified via `libspartan::Instance::is_sat()` across **5 seeds x 3 difficulties** |

The `test_circuit_satisfiability` test is the critical end-to-end correctness proof: it generates circuits, computes witnesses, and confirms the R1CS is satisfied.

## Architecture

```
src/
├── lib.rs                  ← Crate root, re-exports, integration tests
├── dag/
│   ├── config.rs           ← CircuitConfig, from_difficulty()
│   ├── generator.rs        ← generate_dag(), DAG struct, backward BFS algorithm
│   └── node.rs             ← Node, OpType (Add, Mul, Alias, Scale, Pow5)
├── converters/
│   ├── circom.rs           ← dag_to_circom() — Circom 2.0.0 output
│   └── spartan.rs          ← dag_to_spartan(), compute_witness(), column assignment
├── analysis/
│   ├── dag_analysis.rs     ← analyze_dag() — optimization potential report
│   ├── spartan_tools.rs    ← Spartan instance metrics and comparison
│   └── circom_tools.rs     ← Circom compiler integration (requires circom binary)
└── bin/
    └── tig-tool.rs         ← CLI entry point
```

### Key Design Decisions

- **Backward BFS construction**: DAG grows from outputs (low IDs) to inputs (high IDs). Evaluating in reverse ID order gives correct topological forward order.
- **Curve25519 scalar field**: Matches libspartan's native field. Circom output uses BN254 but is only for analysis — not for proving.
- **Deterministic PRNG**: `SHA256(seed) → ChaCha20Rng` ensures any party can reproduce the exact same circuit from the same seed.
- **z-vector layout**: `[private_vars | 1 | outputs... | inputs...]` following libspartan convention.

## Prerequisites

- **Rust** 1.70+ — install from [rustup.rs](https://rustup.rs/)
- **Circom** (optional) — only needed for `count-circom`, `calibrate`, `benchmark` commands. Install from [docs.circom.io](https://docs.circom.io/getting-started/installation/).

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Acknowledgments

Developed by **CryptoEconLab** for The Innovation Game (TIG).

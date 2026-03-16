# ZK Challenge: Accelerating Witness Generation through Circuit Optimization

## Table of Contents

1. [Overview](#1-overview)
2. [Background: R1CS and Witness Generation](#2-background-r1cs-and-witness-generation)
3. [The Challenge](#3-the-challenge)
4. [Random Circuit Generation Pipeline](#4-random-circuit-generation-pipeline)
5. [Verification Protocol: The Double Proof Scheme](#5-verification-protocol-the-double-proof-scheme)
6. [Code Architecture](#6-code-architecture)
7. [Data Structures](#7-data-structures)
8. [Full Pipeline Walkthrough](#8-full-pipeline-walkthrough)
9. [Testing](#9-testing)
10. [Performance Profile](#10-performance-profile)
11. [Difficulty Scaling](#11-difficulty-scaling)
12. [Known Issues and Remaining Work](#12-known-issues-and-remaining-work)

---

## 1. Overview

This challenge addresses a critical bottleneck in zero-knowledge proof systems: **witness generation**, i.e., the computation of all intermediate values required to satisfy a ZK circuit.

While proof generation and verification have been aggressively optimized, witness generation remains under-explored and often dominates runtime. This challenge incentivizes participants to develop **circuit optimization algorithms** that reduce constraint counts, leading to faster witness generation and lower memory usage.

**The task**: Given a randomly generated R1CS circuit C⁰, produce an equivalent circuit C* with fewer constraints. Equivalence is verified via a ZK proof at a random evaluation point (Schwartz-Zippel lemma), making the verification O(1) regardless of circuit size.

**Two repositories** implement this:

| Repository | Branch | Role |
|------------|--------|------|
| `tig-circuit-tools` | `feat/witness-generation` | Random circuit instance generation (seed → DAG → R1CS) |
| `tig-monorepo` | `zk` | Challenge protocol: solving, proving, and verification |

---

## 2. Background: R1CS and Witness Generation

A **Rank-1 Constraint System (R1CS)** is a set of equations:

```
⟨aᵢ, w⟩ · ⟨bᵢ, w⟩ = ⟨cᵢ, w⟩    for each constraint i
```

In matrix form: **(A·w) ⊙ (B·w) = C·w**, where ⊙ is element-wise multiplication.

The **witness vector** `w` contains:
- A leading constant `1`
- Public inputs (known to verifier)
- Public outputs (verifier checks these)
- Private intermediate variables (hidden from verifier)

**Example**: Computing `z = x² · y + y` requires 3 constraints:

```
v1 = x · x        (constraint 1)
v2 = v1 · y       (constraint 2)
z  = v2 + y       (constraint 3, encoded as (v2 + y) · 1 = z)
```

With `x=3, y=5`: the witness is `w = [1, 50, 3, 5, 9, 45]`.

**Witness generation** means computing all the intermediate values (v1, v2) given the inputs (x, y). More constraints = more intermediates = slower witness generation.

---

## 3. The Challenge

### What participants must do

1. Receive a random baseline circuit **C⁰** (generated from a seed + difficulty)
2. Produce an optimized circuit **C*** that computes the **same function** with **fewer constraints**
3. Prove correctness via ZK proofs

### How quality is measured

```
ε = 1 − K*/K⁰
```

Where K* and K⁰ are the constraint counts. Higher ε = better optimization.
For example, ε = 0.6 means a 60% reduction in constraints.

### Why random circuits?

Static benchmarks allow hand-tuning. Instead, circuits are **procedurally generated** from a cryptographic seed, ensuring:
- Participants can't memorize optimal solutions
- Both Prover and Verifier can independently reconstruct C⁰ from `(seed, difficulty)`
- The generation is fully deterministic

---

## 4. Random Circuit Generation Pipeline

This is implemented in **`tig-circuit-tools`**.

### 4.1 Three-Stage Pipeline

```
seed (string) + difficulty (δ)
        │
        ▼
   ┌─────────────────────┐
   │ Stage 1: DAG Gen    │  SHA256(seed) → ChaCha20 PRNG → backward BFS
   │ generate_dag()       │  builds a directed acyclic graph of operations
   └─────────┬───────────┘
             │
             ▼
   ┌─────────────────────┐
   │ Stage 2a: Spartan   │  dag_to_spartan() → sparse R1CS matrices (A, B, C)
   │ (for proving)       │  over Curve25519 scalar field
   └─────────┬───────────┘
             │
             ▼
   ┌─────────────────────┐
   │ Stage 2b: Circom    │  dag_to_circom() → .circom source file
   │ (for analysis only) │  (uses BN254 field — NOT used for proving)
   └─────────────────────┘
```

### 4.2 DAG Generation in Detail

`generate_dag(seed, config)` builds the circuit **backwards** (from outputs to inputs):

1. **Seed determinism**: `SHA256(seed)` → 32 bytes → `ChaCha20Rng`. Same seed always produces the same circuit.

2. **Create output nodes**: 1–3 output nodes (IDs 0, 1, ...).

3. **Backward BFS expansion**: For each node in the frontier queue, randomly assign an operation type and create its operands:

| Operation | What it does | Constraints | Optimization potential |
|-----------|-------------|-------------|----------------------|
| `Add(l, r)` | `out = l + r` | 1 | Core computation |
| `Mul(l, r)` | `out = l * r` | 1 | Core computation |
| `Alias(src)` | `out = src` (identity) | 1 | **Trivially removable** via substitution |
| `Scale(src, k)` | `out = k * src` | 1 | **Removable** via constant folding |
| `Pow5(src)` | `out = src⁵` | 3 | **Reducible**: 3→1 by algebraic simplification |

4. **Redundancy injection**: With probability `redundancy_ratio`, an operand reuses an existing node (shared subexpression) instead of creating a new one. This creates optimization opportunities through common subexpression elimination (CSE).

5. **Finalization**: Any unprocessed nodes become `Input` nodes.

**Key invariant**: Node IDs increase as you move from outputs to inputs. Evaluating in **reverse ID order** gives the correct topological (forward) order.

### 4.3 Configuration Parameters

`CircuitConfig::from_difficulty(delta)` produces:

| Parameter | Value | Purpose |
|-----------|-------|---------|
| `num_constraints` | δ × 1000 | Circuit size |
| `redundancy_ratio` | 0.25 (25%) | Shared subexpression density |
| `alias_ratio` | 0.15 (15%) | Trivially removable identity ops |
| `linear_ratio` | 0.20 (20%) | Removable constant-scaling ops |
| `power_map_ratio` | 0.15 (15%) | Algebraic power maps (x⁵, like Poseidon S-boxes) |
| Remaining ~50% | Add/Mul (50/50) | Core arithmetic |

This means roughly **50% of constraints are intentionally removable** — the "optimization traps" that challengers should eliminate.

### 4.4 R1CS Conversion

`dag_to_spartan(dag)` converts the DAG to sparse R1CS matrices.

**Column layout** (libspartan z-vector convention):

```
z = [ private_vars (0..num_vars-1) | 1 (constant) | public_io (outputs..., inputs...) ]
```

Each operation type maps to R1CS rows:

| Op | A · B = C |
|----|-----------|
| `Alias(src)` | `node · 1 = src` |
| `Add(l, r)` | `(l + r) · 1 = node` |
| `Mul(l, r)` | `l · r = node` |
| `Scale(src, k)` | `(k·src) · 1 = node` |
| `Pow5(src)` | 3 rows: `src·src = sq`, `sq·sq = qd`, `qd·src = node` |

### 4.5 Witness Computation

`compute_witness(dag, input_values)` evaluates the circuit:

1. Assign input scalar values to Input nodes
2. Evaluate all nodes in **reverse ID order** (topological forward pass)
3. Return `(vars, public_io)` where:
   - `vars`: private intermediate values (length = `num_vars`)
   - `public_io`: `[output_values..., input_values...]` (length = `num_inputs`)

These are directly usable with libspartan's `VarsAssignment` and `InputsAssignment`.

---

## 5. Verification Protocol: The Double Proof Scheme

Implemented in **`tig-monorepo/tig-challenges/src/zk.rs`**.

### 5.1 Why ZK Proofs?

We need to verify that C*(x) = C⁰(x) for all inputs x. Testing all inputs is impossible (the field has ~2²⁵⁵ elements). Instead:

1. **Schwartz-Zippel Lemma**: If two polynomial functions disagree, they agree at a random point with probability at most d/|F| ≈ 0 (negligible for cryptographic fields).
2. **Fiat-Shamir anti-grinding**: The evaluation point x_eval is derived from the hash of both circuits, preventing the Prover from crafting C* for a specific input.

### 5.2 Protocol Steps

```
                    PROVER                                           VERIFIER
                      │                                                │
    1. Receive C⁰     │◄──────────── C⁰ (from seed + δ) ─────────────│
                      │                                                │
    2. Optimize        │  C* = optimize(C⁰)                            │
                      │  (K* < K⁰, rows in topological order)         │
                      │                                                │
    3. Hash & derive   │  h₀ = Blake3(C⁰)                             │
                      │  h* = Blake3(C*)                              │
                      │  x_eval = Hash-to-Field(h₀ || h*)            │
                      │                                                │
    4. Execute both    │  w⁰ = witness(C⁰, x_eval) → y⁰_pub          │
                      │  w* = witness(C*, x_eval) → y*_pub            │
                      │                                                │
    5. Prove           │  π⁰ = SNARK.prove(C⁰, w⁰, x_eval)           │
                      │  π* = SNARK.prove(C*, w*, x_eval)             │
                      │                                                │
    6. Submit          │─── (C*, x_eval, y⁰, y*, π⁰, π*) ───────────►│
                      │                                                │
                      │                                  7. Verify:    │
                      │                                  • K* < K⁰     │
                      │                                  • y⁰ == y*    │
                      │                                  • SNARK.verify(π⁰)
                      │                                  • SNARK.verify(π*)
```

### 5.3 Security Properties

1. **Collision resistance** (Blake3): Can't forge circuit hashes
2. **Schwartz-Zippel**: Non-equivalent circuits produce different outputs at random point with overwhelming probability
3. **Fiat-Shamir** (hash-derived x_eval): Can't "grind" C* to work only at a specific input
4. **Spartan soundness**: Can't forge proofs for false statements

### 5.4 Hashing Details

**Blake3 XOF** (extendable output function) is used throughout:

```rust
// Hash a circuit: serialize with bincode, then Blake3 → 512-bit digest
CryptoHash::from_spartan_instance(instance) → [u8; 64]

// Combine two hashes: Blake3(h₀ || h*)
h0.combine(&h_star) → CryptoHash

// Hash-to-Field: derive N scalars from a digest
// For each i: rᵢ = Blake3(digest || i) mod P (Curve25519 scalar field order)
combined.to_scalars(num_inputs) → Vec<Scalar>
```

---

## 6. Code Architecture

### 6.1 Repository Layout

```
tig-circuit-tools/               ← Random circuit generation library
├── src/
│   ├── lib.rs                   ← Crate root, integration tests
│   ├── dag/
│   │   ├── config.rs            ← CircuitConfig, from_difficulty()
│   │   ├── generator.rs         ← generate_dag(), DAG struct
│   │   └── node.rs              ← Node, OpType (Add, Mul, Alias, Scale, Pow5)
│   ├── converters/
│   │   ├── circom.rs            ← dag_to_circom() (analysis only)
│   │   └── spartan.rs           ← dag_to_spartan(), compute_witness(), solve_witness_forward(), solve_witness_from_r1cs(), remove_aliases()
│   └── analysis/                ← DAG analysis, Spartan metrics
└── Cargo.toml

tig-monorepo/ (branch: zk)      ← Challenge protocol
├── tig-challenges/
│   ├── src/
│   │   ├── lib.rs               ← Feature-gated: c007 → pub mod zk
│   │   └── zk.rs                ← THE CORE FILE: Challenge, Solution, solve, verify
│   └── Cargo.toml               ← Dependencies: spartan, curve25519-dalek, merlin, blake3, tig-circuit-tools
├── tig-structs/                 ← Protocol-level data structures
├── tig-verifier/                ← Verification binary (needs c007 integration)
├── tig-runtime/                 ← Benchmark runtime (needs c007 integration)
└── tig-algorithms/              ← Participant algorithm stubs (needs c007 stubs)
```

### 6.2 Dependency Chain

```
tig-challenges (feature: c007)
    ├── tig-circuit-tools (git, branch: feat/witness-generation)
    │     ├── curve25519-dalek   ← Scalar field arithmetic
    │     ├── rand + rand_chacha ← Deterministic PRNG
    │     └── sha2               ← Seed hashing
    ├── spartan 0.9.0 (libspartan) ← ZK-SNARK proving system
    ├── merlin 3.0               ← Fiat-Shamir transcript
    ├── blake3                   ← Circuit hashing
    ├── bincode                  ← Circuit serialization (for hashing)
    └── curve25519-dalek 4.1     ← Scalar type (shared with tig-circuit-tools)
```

### 6.3 Feature Flags

The ZK challenge is gated behind the `c007` feature:

```toml
# tig-challenges/Cargo.toml
c007 = ["zk", "blake3"]
zk = ["c007"]
```

Build with: `cargo build --features c007 -p tig-challenges`

---

## 7. Data Structures

### 7.1 SpartanInstance (R1CS representation)

The circuit type is `SpartanInstance` from `tig-circuit-tools`, used directly in both `Challenge` and `Solution`:

```rust
pub struct SpartanInstance {
    pub num_cons: usize,     // Number of constraints (rows in A, B, C)
    pub num_vars: usize,     // Number of private variables
    pub num_inputs: usize,   // Number of public I/O values (outputs + inputs)
    pub A: R1CSMatrix,       // Sparse matrix A in COO format
    pub B: R1CSMatrix,       // Sparse matrix B
    pub C: R1CSMatrix,       // Sparse matrix C
}

// Each entry: (row_index, column_index, scalar_value_as_32_bytes)
pub type R1CSMatrix = Vec<(usize, usize, [u8; 32])>;
```

The z-vector layout for libspartan:

```
z = [ vars(0..num_vars-1)  |  1 (at index num_vars)  |  public_io(num_vars+1..) ]
                                                          ├── outputs (first)
                                                          └── inputs  (after)
```

**Row ordering requirement**: rows are in topological evaluation order — each row has at most one unknown variable when processed in sequence. `dag_to_spartan` guarantees this. Challengers must preserve it in their C*.

### 7.2 Challenge

```rust
pub struct Challenge {
    pub seed: [u8; 32],                // Cryptographic seed
    pub difficulty: Difficulty,         // Contains delta: usize
    pub circuit_c0: SpartanInstance,   // Baseline circuit C⁰
    pub num_circuit_inputs: usize,     // Number of circuit input signals
    pub num_circuit_outputs: usize,    // Number of circuit output signals
}
```

### 7.3 Solution

```rust
pub struct Solution {
    pub circuit_star: SpartanInstance, // Optimized circuit C* (rows in topological order)
    pub y0_pub: Vec<Scalar>,           // C⁰ output values at x_eval
    pub y_star_pub: Vec<Scalar>,       // C* output values at x_eval
    pub proof0: SNARK,                 // Spartan proof for C⁰
    pub proof_star: SNARK,             // Spartan proof for C*
}
```

Generators and commitments are NOT in the Solution. The verifier recomputes them independently from the circuit matrices — this is what prevents the prover from supplying crafted parameters.

### 7.4 Participant Interface

```rust
/// Takes C⁰, returns C* with strictly fewer constraints.
/// C* rows must be in topological evaluation order.
/// See OptimizeCircuitFn rustdoc for full requirements.
pub type OptimizeCircuitFn = fn(&SpartanInstance) -> SpartanInstance;
```

---

## 8. Full Pipeline Walkthrough

### Step-by-step: from seed to verified proof

```
1. INSTANCE GENERATION (deterministic, shared by Prover and Verifier)
   ─────────────────────────────────────────────────────────────────
   seed: [u8; 32]           (from protocol: calc_seed(rand_hash, nonce))
   difficulty: δ = 1        (from protocol round settings)
        │
        ├─► seed_to_hex(seed) → "2a000...000"
        ├─► CircuitConfig::from_difficulty(1) → { num_constraints: 1000, ... }
        ├─► generate_dag("2a000...000", config)
        │     └─► SHA256 → ChaCha20 PRNG → backward BFS → DAG with ~1000 constraints
        ├─► dag_to_spartan(&dag) → SpartanInstance { A, B, C matrices }
        └─► Challenge { seed, difficulty, circuit_c0, num_inputs: 149, num_outputs: 3 }

2. OPTIMIZATION (Prover only)
   ──────────────────────────
   optimize(&circuit_c0) → (circuit_star, witness_builder)
   // Participant's algorithm reduces constraints: K* < K⁰

3. HASH DERIVATION (anti-grinding, deterministic)
   ───────────────────────────────────────────────
   h₀    = Blake3_XOF(bincode(C⁰))     → [u8; 64]
   h*    = Blake3_XOF(bincode(C*))      → [u8; 64]
   combined = Blake3_XOF(h₀ || h*)      → [u8; 64]
   x_eval = combined.to_scalars(149)    → 149 Curve25519 Scalars

4. WITNESS COMPUTATION (Prover only)
   ─────────────────────────────────
   For C⁰: regenerate DAG, compute_witness(&dag, &x_eval)
           → (vars0: 997 Scalars, public_io0: 152 Scalars)
              public_io0 = [y0_out_0, y0_out_1, y0_out_2, x_eval_0, ..., x_eval_148]

   For C*: solve_witness_forward(&circuit_star, num_outputs, &x_eval)
           → (vars_star, public_io_star)   [single pass; errors if rows not in order]

5. PROOF GENERATION (Prover only, expensive)
   ─────────────────────────────────────────
   For π⁰:
     inst0 = Instance::new(num_cons, num_vars, num_inputs, A, B, C)
     gens0 = SNARKGens::new(num_cons, num_vars, num_inputs, max_nnz)
     (comm0, decomm0) = SNARK::encode(&inst0, &gens0)
     proof0 = SNARK::prove(&inst0, &comm0, &decomm0, vars0, &io0, &gens0, transcript_C0)

   For π*: (same steps with circuit_star, transcript label "ZKChallenge_Cstar")

6. VERIFICATION (Verifier only, cheap)
   ────────────────────────────────────
   a) Recompute h₀, h*, x_eval (same hash derivation as step 3)
   b) Check K* < K⁰                           ← integer comparison
   c) Check y⁰_pub == y*_pub                  ← vector equality
   d) proof0.verify(&comm0, &io0, transcript_C0, &gens0)      ← ~1.8s
   e) proof_star.verify(&comm_star, &io_star, transcript_Cstar, &gens_star)  ← ~1.8s
```

---

## 9. Testing

### 9.1 Running Tests

```bash
# tig-circuit-tools: 20 unit + integration tests (< 1s)
cd tig-circuit-tools
cargo test

# tig-challenges: all 5 ZK tests (~68s, dominated by Spartan proofs)
cd tig-monorepo
cargo test --features c007 -p tig-challenges -- --nocapture

# Just the fast tests (~2s)
cargo test --features c007 -p tig-challenges -- --nocapture --skip test_full_identity_roundtrip

# Just the full prove/verify roundtrip (~65s)
cargo test --features c007 -p tig-challenges -- test_full_identity_roundtrip --nocapture
```

### 9.2 Test Descriptions

#### tig-circuit-tools (20 tests)

| Test | What it validates |
|------|-------------------|
| `test_deterministic_generation` | Same seed → identical DAG |
| `test_different_seeds_produce_different_dags` | Different seeds → different DAGs |
| `test_dag_has_outputs` | DAG has 1-3 output nodes |
| `test_constraint_count_matches_dag` | `dag.total_constraints() == spartan.num_cons` |
| `test_column_bounds` | All R1CS matrix entries within valid range |
| `test_compute_witness_basic` | Witness has correct dimensions |
| **`test_circuit_satisfiability`** | **Critical: (A·z)⊙(B·z)=C·z for 5 seeds × 3 difficulties** |
| `test_circom_generation` | Circom output has valid structure |
| Various analysis tests | DAG analysis and Spartan metrics |

#### tig-challenges/zk.rs (5 tests)

| Test | What it validates | Time |
|------|-------------------|------|
| `test_generate_instance` | Seed → C⁰ with correct dimensions (~1000 constraints for δ=1) | <1s |
| `test_deterministic_generation` | Same seed → byte-identical R1CS matrices | <1s |
| `test_hash_and_xeval_derivation` | Blake3 hashing deterministic, x_eval scalars non-zero | <1s |
| `test_witness_satisfies_c0` | `compute_witness` output passes `Instance::is_sat()` | ~2s |
| **`test_full_identity_roundtrip`** | **Full Spartan SNARK prove + verify for both π⁰ and π*** | **~65s** |

### 9.3 What the Full Identity Roundtrip Tests

This is the most important test. It uses C* = C⁰ (identity, no optimization) to exercise the **entire cryptographic pipeline** without needing a real optimizer:

1. Generate challenge from seed
2. Hash both circuits (identical here), derive x_eval
3. Compute witness for C⁰
4. **Generate Spartan SNARK proof π⁰** (transcript label `b"ZKChallenge_C0"`)
5. **Verify π⁰** — confirms the proof is valid
6. **Generate Spartan SNARK proof π*** (transcript label `b"ZKChallenge_Cstar"`)
7. **Verify π*** — confirms this proof too
8. Check output equivalence: y⁰_pub == y*_pub

The only check skipped is K* < K⁰ (since C* = C⁰ they're equal). This is just an integer comparison — trivially correct. Everything else (hashing, Fiat-Shamir, witness generation, Spartan prove/verify) is exercised end-to-end.

### 9.4 Example Test Output

```
=== Full Identity Roundtrip (delta=1) ===
[1/7] generate_instance: 1000 constraints, 997 vars, 152 public I/O (149 in, 3 out),
      nnz=(1186,1000,1000) in 3.20ms
[2/7] hash + x_eval derivation (149 scalars) in 2.61ms
[3/7] DAG regen + witness computation in 2.63ms
[4/7] SNARK encode=12.15s, prove pi0=19.33s
[5/7] verify pi0 in 1.77s
[6/7] encode+prove pi*=32.60s, verify pi*=1.81s
[7/7] output equivalence OK (3 output scalars match)
=== PASSED in 67.66s ===
```

---

## 10. Performance Profile

Measured with delta=1 (~1000 constraints), debug build:

### Prover (Challenger) Costs

| Step | Time | Notes |
|------|------|-------|
| Circuit generation (seed → DAG → R1CS) | ~3ms | Deterministic, fast |
| Hash derivation + x_eval | ~3ms | Blake3 XOF |
| Witness computation | ~3ms | DAG evaluation in topological order |
| SNARK encode (per circuit) | ~12s | Commitment to R1CS instance |
| SNARK prove (per circuit) | ~20s | The expensive step |
| **Total Prover** | **~65s** | Two encode+prove operations |

### Verifier Costs

| Step | Time | Notes |
|------|------|-------|
| Hash derivation + x_eval | ~3ms | Same as Prover |
| K* < K⁰ check | ~0 | Integer comparison |
| Output equivalence | ~0 | Vector equality |
| SNARK verify (per proof) | ~1.8s | Two verify operations |
| **Total Verifier** | **~3.6s** | O(1) regardless of circuit size |

**Prover/Verifier ratio: ~18×**. The Verifier is lightweight by design — this is the core value proposition of the Double Proof scheme.

Note: These are debug build times. Release builds will be substantially faster.

---

## 11. Difficulty Scaling

Difficulty is parameterized by a single scalar **δ** (delta):

```
K_target = δ × 1000
```

| δ | Target constraints | Approximate time (debug, Prover) |
|---|-------------------|----------------------------------|
| 1 | 1,000 | ~65s |
| 2 | 2,000 | ~2-4min (estimated) |
| 5 | 5,000 | ~10-20min (estimated) |
| 10 | 10,000 | ~30-60min (estimated) |

All other parameters (redundancy, alias, linear, power map ratios) are **held constant** across difficulties. This means:
- The optimization challenge is structurally identical at every tier
- A solver that works at δ=1 scales predictably to higher δ
- Doubling δ roughly doubles circuit size while maintaining ~50% theoretical reduction potential

### Variance Control

To ensure fairness (different seeds at the same δ should present comparable difficulty), the system uses Monte Carlo calibration:
- Generate many instances with the same θ
- Measure reducibility η using a reference solver (e.g., Circom -O1)
- Accept only configurations where standard deviation σ_η < 0.05

---

## 12. Known Issues and Remaining Work

### Completed

- **`Circuit` → `SpartanInstance`**: The custom `Circuit` wrapper has been removed. `Challenge` and `Solution` now use `SpartanInstance` from `tig-circuit-tools` directly, giving challengers access to libspartan utilities like `Instance::is_sat()`.
- **Topological row order**: `dag_to_spartan` now emits rows in topological evaluation order (reverse node-ID). `solve_witness_forward` (single O(n) pass) replaces `solve_witness_from_r1cs` (O(n²) fixed-point) as the witness solver for C*.
- **Order enforcement**: `solve_challenge` uses `solve_witness_forward` for C*. A circuit with out-of-order rows is rejected immediately with `WitnessError::NotInEvaluationOrder { row }`.
- **Alias optimizer end-to-end**: `remove_aliases` is fully tested in `test_alias_optimizer_roundtrip` — generates challenge, optimizes, proves, verifies.

### Remaining Integration Work

| Item | Status | Description |
|------|--------|-------------|
| `tig-verifier` c007 dispatch | Missing | No `"c007"` arm in the verifier binary's challenge dispatch |
| `tig-runtime` c007 dispatch | Missing | No `"c007"` arm in the runtime's `compute_solution` |
| `tig-algorithms` feature name | Bug | `lib.rs` gates on `c006`, `Cargo.toml` defines `c007` |
| `tig-algorithms/src/zk/` | Missing | No participant algorithm stubs exist yet |
| `Solution` wire format | Missing | `zk::Solution` needs `From`/`TryFrom` conversions for the protocol's JSON map format |
| `tig-circuit-tools` branch reference | Needs update | Cargo.toml points to `branch = "feat/witness-generation"` — update to `main` after PR merge |

### Future Enhancements

- **Release build benchmarks**: Debug mode dominates current timing; release builds needed for realistic performance numbers
- **Larger difficulty tests**: Validate scaling behavior at δ=2, 5, 10

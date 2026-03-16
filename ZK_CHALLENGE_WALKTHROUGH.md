# TIG ZK Challenge c007 — Full Walkthrough

> **Challenge ID:** c007
> **Topic:** Accelerating Witness Generation through Circuit Optimization
> **Two roles:** Verifier and Challenger

---

## Table of Contents

1. [Background: What is R1CS?](#1-background-what-is-r1cs)
2. [Challenge Overview](#2-challenge-overview)
3. [Circuit Generation](#3-circuit-generation)
4. [The Protocol](#4-the-protocol)
   - [Step 1 — Verifier generates and sends C⁰](#step-1--verifier-generates-and-sends-c)
   - [Step 2 — Challenger optimizes C⁰ → C*](#step-2--challenger-optimizes-c--c)
   - [Step 3 — Hash commitment and random point derivation](#step-3--hash-commitment-and-random-point-derivation)
   - [Step 4 — Challenger computes both witnesses](#step-4--challenger-computes-both-witnesses)
   - [Step 5 — Challenger generates two Spartan proofs](#step-5--challenger-generates-two-spartan-proofs)
   - [Step 6 — Verifier checks the solution](#step-6--verifier-checks-the-solution)
5. [Security Analysis](#5-security-analysis)
6. [Quality Scoring](#6-quality-scoring)
7. [Difficulty Scaling](#7-difficulty-scaling)
8. [Implementation Reference](#8-implementation-reference)

---

## 1. Background: What is R1CS?

A **Rank-1 Constraint System (R1CS)** is a system of equations of the form:

```
⟨aᵢ, w⟩ · ⟨bᵢ, w⟩ = ⟨cᵢ, w⟩    for each constraint i
```

where `w` is the **witness vector** — all values involved in the computation:

| Position in `w` | Content | Visibility |
|-----------------|---------|------------|
| `w[0]` | Constant `1` | — |
| `w[1..n_out]` | Public outputs | Known to Verifier |
| `w[n_out..n_io]` | Public inputs | Known to Verifier |
| `w[n_io..]` | Private intermediate variables | Hidden |

In matrix notation: `(Aw) ∘ (Bw) = Cw`, where `A`, `B`, `C` are sparse `n_constraints × m_vars` matrices.

**Example:** computing `z = x² · y + y`

```
w = [1, z, x, y, v1, v2]    (v1=x², v2=v1·y are private intermediates)

C1: x · x  = v1
C2: v1 · y = v2
C3: (v2 + y) · 1 = z
```

The number of constraints `K` is the core metric of this challenge — fewer constraints means faster witness generation and smaller memory overhead.

---

## 2. Challenge Overview

> Given a random R1CS circuit C⁰, produce an alternative R1CS circuit C* that:
> - computes the **same function** as C⁰
> - uses **fewer constraints** than C⁰

The Challenger implements a single function:

```rust
type OptimizeCircuitFn = fn(&SpartanInstance) -> SpartanInstance;
```

Correctness is verified cryptographically — the Verifier never re-executes either circuit. Instead, correctness is reduced to **polynomial identity at a random point** (Schwartz-Zippel lemma), backed by two **Spartan SNARK proofs**.

---

## 3. Circuit Generation

Both Verifier and Challenger can independently reconstruct C⁰ from just `(seed, delta)`. The pipeline is fully deterministic and has three stages.

### Stage 1 — Seed to DAG

```
seed ([u8;32]) + delta (usize)
    → seed_to_hex()                    64-char hex string
    → SHA256(seed_hex)                 32-byte key
    → ChaCha20 PRNG                    deterministic random stream
    → backward BFS expansion           DAG
```

The DAG is built **backwards from outputs toward inputs**:

1. Create 1–3 **output nodes** (node IDs 0, 1, 2)
2. For each node in the expansion frontier, the PRNG assigns an operation:

| Operation | Probability | Description |
|-----------|-------------|-------------|
| `Alias(src)` | 15% | `out = src` — trivial identity, removable by substitution |
| `Scale(src, k)` | 20% | `out = k·src` — constant multiplication, removable by folding |
| `Pow5(src)` | 15% | `out = src⁵` — S-box (as in Poseidon), costs **3 constraints** |
| Reuse existing node | 25% | shared sub-expression (redundancy for CSE potential) |
| `Add(l, r)` or `Mul(l, r)` | remaining | core arithmetic |

3. All nodes never defined become **Input nodes**

These probabilities are fixed across all difficulty levels and deliberately inject **known optimization traps**: Alias/Scale nodes are trivially removable, shared nodes reward CSE, and Pow5 intermediates can be merged when the same input appears multiple times.

### Stage 2 — DAG to R1CS

Each DAG node maps to R1CS constraints. The witness vector `z` follows the libspartan layout:

```
z = [ private_vars (0..num_vars-1) | 1 (at num_vars) | outputs... | inputs... ]
        ↑ hidden from Verifier                              ↑ public
```

| Node type | Constraints | R1CS encoding |
|-----------|-------------|---------------|
| `Input` | 0 | — |
| `Add(l, r)` | 1 | `(l + r) · 1 = out` |
| `Mul(l, r)` | 1 | `l · r = out` |
| `Alias(src)` | 1 | `out · 1 = src` |
| `Scale(src, k)` | 1 | `(k·src) · 1 = out` |
| `Pow5(src)` | **3** | `sq = src·src`, `qd = sq·sq`, `out = qd·src` |

### Stage 3 — Result: the baseline circuit C⁰

The output is a `SpartanInstance` (from `tig-circuit-tools`):

```rust
struct SpartanInstance {
    num_cons:   usize,        // K⁰ — number of constraints
    num_vars:   usize,        // number of private variables
    num_inputs: usize,        // number of public I/O values (outputs + inputs)
    A: R1CSMatrix,            // sparse COO format: Vec<(row, col, [u8;32])>
    B: R1CSMatrix,
    C: R1CSMatrix,
}
```

**Rows are in topological evaluation order** — each row has at most one unknown variable when processed in sequence (deep dependencies first, output constraints last). This is guaranteed by `dag_to_spartan` and must be preserved by the Challenger's optimizer.

With `delta = 1`, this produces approximately 1000 constraints. With `delta = N`, it produces approximately `N × 1000`.

---

## 4. The Protocol

### Role Summary

| | Verifier | Challenger |
|--|----------|-----------|
| **Input** | `(seed, delta)` | C⁰ |
| **Action** | Reconstructs C⁰, validates S | Produces C* with K* < K⁰ |
| **Output** | `Ok` or error | Solution `S = (C*, y⁰_pub, y*_pub, π⁰, π*)` |
| **Computational cost** | ~3.6s (two SNARK verifications) | ~66–90s (two SNARK proofs, debug build) |

---

### Step 1 — Verifier generates and sends C⁰

The Verifier runs:

```rust
Challenge::generate_instance(seed, difficulty)
```

Internally: `(seed, delta)` → SHA256 → ChaCha20 → backward BFS DAG → R1CS → C⁰

The Challenger does **not** need to trust this — they can regenerate C⁰ independently from `(seed, delta)` and verify it matches.

---

### Step 2 — Challenger optimizes C⁰ → C*

The Challenger implements an optimization function:

```rust
fn optimize(c0: &SpartanInstance) -> SpartanInstance { ... }
```

C* must satisfy three conditions:
1. `K* < K⁰` — strictly fewer constraints
2. Same function — same outputs for any given inputs
3. **Rows in topological evaluation order** — when the witness solver processes row `i`, all variables in A and B of that row must already be known from previous rows or from the circuit inputs. At most one unknown is allowed per row. Violations are rejected immediately.

**Optimization opportunities in the generated circuits:**

| Technique | Target | Reduction potential |
|-----------|--------|-------------------|
| Variable substitution | `Alias` nodes (`out · 1 = src`) | ~13–15% |
| Constant folding | `Scale` nodes (`k·src · 1 = out`) | ~20% |
| Common subexpression elimination | Reused sub-DAGs (ρ = 0.25) | large, depends on circuit |
| Pow5 intermediate sharing | Multiple `Pow5` on the same input share `sq` and `qd` | up to 2/3 per reuse |
| Linear combination merging | Addition chains | moderate |

A baseline optimizer (`remove_aliases`) is already provided in `tig-circuit-tools`. It removes Alias constraints via variable substitution and achieves ~13% reduction.

---

### Step 3 — Hash commitment and random point derivation

Once C* is produced, the Challenger derives the evaluation point:

```
h⁰     = Blake3_XOF( bincode(C⁰) )        → 512-bit hash
h*     = Blake3_XOF( bincode(C*) )        → 512-bit hash
x_eval = Blake3_XOF( h⁰ || h* )           → num_inputs field scalars
```

This is the **Fiat-Shamir anti-grinding** mechanism. `x_eval` depends on C* itself:

- If the Challenger tries to craft a C* that only agrees with C⁰ at specific inputs, modifying C* changes h*, which changes x_eval, which moves the evaluation point.
- The dependency is circular — grinding is computationally infeasible.

---

### Step 4 — Challenger computes both witnesses

**Witness for C⁰** — uses the original DAG (fast, exact forward evaluation):

```
regenerate DAG from (seed, delta)
→ evaluate each node in topological order with x_eval as inputs
→ (vars⁰, public_io⁰)    where public_io⁰ = [y⁰_out..., x_eval...]
```

**Witness for C*** — single forward pass (no DAG needed, no iteration):

```
initialize z: known = [1, x_eval...], unknown = [private_vars..., outputs...]

for each row i in order 0..num_cons:
    find the one unknown variable in this row → solve algebraically:
        x = (c_known - a_known · b_known) / (aⱼ · b_known + bⱼ · a_known - cⱼ)
    if row has more than 1 unknown → ERROR: NotInEvaluationOrder { row: i }

→ (vars*, public_io*)    where public_io* = [y*_out..., x_eval...]
```

This is O(n) — one pass, no backtracking. It works because C* rows are required to be in topological order. Any violation is an immediate, unrecoverable error — the solution is rejected before any proof is attempted. The full responsibility for correct ordering lies with the Challenger.

Note: `compute_witness` for C⁰ is even simpler — it evaluates the DAG directly in node order without touching the R1CS matrices at all.

---

### Step 5 — Challenger generates two Spartan proofs

```
π⁰ = SNARK::prove(C⁰, vars⁰, public_io⁰,  transcript = "ZKChallenge_C0")
π* = SNARK::prove(C*, vars*, public_io*,   transcript = "ZKChallenge_Cstar")
```

Each proof cryptographically attests that:
- The private witness satisfies all R1CS constraints of the respective circuit
- The public outputs are exactly `y⁰_pub` (resp. `y*_pub`)
- The inputs used are exactly `x_eval`

The Challenger submits:

```
S = ( C*, y⁰_pub, y*_pub, π⁰, π* )
```

---

### Step 6 — Verifier checks the solution

The Verifier runs `Challenge::verify_solution(solution)` with four independent checks:

#### Check 1 — Constraint reduction

```
K* < K⁰
```

A trivial integer comparison. Ensures the Challenger actually reduced the circuit.

#### Check 2 — Random point re-derivation

The Verifier independently recomputes:

```
h⁰     = Blake3_XOF( bincode(C⁰) )        (C⁰ is known from the challenge)
h*     = Blake3_XOF( bincode(C*) )        (C* is in the submission)
x_eval = Blake3_XOF( h⁰ || h* )
```

This ensures the evaluation point was derived honestly and not tampered with.

#### Check 3 — Output equivalence

```
y⁰_pub == y*_pub
```

Both circuits were evaluated at the same random point `x_eval` over a field of size ≈ 2²⁵². By the **Schwartz-Zippel lemma**: if two polynomials agree at a random point, they are identical with probability ≥ 1 − d/|F|, where d is the circuit depth and |F| ≈ 2²⁵². This probability is negligible — the check is essentially a proof of functional equivalence.

#### Check 4 — SNARK proof verification

For each circuit, the Verifier **recomputes the Spartan parameters from scratch** (generators and commitment) from the circuit matrices — it never trusts prover-supplied values. Then:

```
SNARK::verify(π⁰, comm⁰, public_io = [y⁰_pub..., x_eval...])  → Ok
SNARK::verify(π*, comm*, public_io = [y*_pub..., x_eval...])   → Ok
```

- π⁰ proves: y⁰_pub is the genuine output of C⁰ at x_eval
- π* proves: y*_pub is the genuine output of C* at x_eval

Together with Check 3, this constitutes a full cryptographic proof of equivalence — the Verifier never needs to execute either circuit.

---

## 5. Security Analysis

| Threat | Mitigation |
|--------|-----------|
| **Fake equivalence** — submit a C* that computes a different function | Schwartz-Zippel: two distinct polynomials agree at a random point with prob ≤ d/\|F\| ≈ negligible |
| **Input grinding** — craft C* valid only at a specific x_eval | Fiat-Shamir: x_eval = H(H(C⁰) \|\| H(C*)) — modifying C* changes x_eval, making the target move |
| **Forged SNARK proof** — claim y_pub is the output when it is not | Spartan soundness: computationally infeasible under discrete log assumption |
| **Tampered generators or commitments** — supply crafted Spartan params | Verifier recomputes all Spartan parameters independently from circuit matrices |
| **Non-reproducible circuit** — dispute over what C⁰ is | (seed, delta) fully determines C⁰; both sides recompute it independently |
| **Hash collision** — two distinct circuits with same hash | Blake3 collision resistance |

---

## 6. Quality Scoring

```
ε = 1 − K*/K⁰     ∈ [0, 1)
```

| ε | Meaning |
|---|---------|
| 0 | No improvement (invalid — K* must be strictly < K⁰) |
| 0.13 | Alias-only removal (baseline `remove_aliases`) |
| 0.35 | Alias + Scale removal |
| 0.60 | Theoretical target (full CSE + Pow5 sharing + folding) |

Higher ε directly translates to faster witness generation and lower memory usage in production ZK workflows.

---

## 7. Difficulty Scaling

Difficulty is parameterized by a single scalar `delta`:

| Parameter | Value |
|-----------|-------|
| `Ktarget` | `delta × 1000` constraints |
| `Palias` | 0.15 (fixed) |
| `Plin` | 0.20 (fixed) |
| `Pmap` | 0.15 (fixed) |
| `ρ` | 0.25 (fixed) |

All difficulty levels have the **same statistical distribution of optimization traps** — only the circuit *size* scales. A solver achieving ε = 0.5 at delta = 1 should achieve approximately the same ε at delta = 10, making performance benchmarks directly comparable across difficulty tiers.

---

## 8. Implementation Reference

### Key files

| File | Role |
|------|------|
| `tig-challenges/src/zk.rs` | Challenge struct, `solve_challenge()`, `verify_solution()` |
| `tig-circuit-tools/src/dag/generator.rs` | `generate_dag()` — backward BFS DAG construction |
| `tig-circuit-tools/src/converters/spartan.rs` | `dag_to_spartan()`, `compute_witness()`, `solve_witness_forward()`, `remove_aliases()` |
| `tig-algorithms/src/` | Where participant optimizers are registered |

### Data types

```rust
// The circuit handed to the Challenger and returned as C*
struct SpartanInstance {
    num_cons:   usize,
    num_vars:   usize,
    num_inputs: usize,
    A: Vec<(usize, usize, [u8; 32])>,   // sparse COO: (row, col, scalar_bytes)
    B: Vec<(usize, usize, [u8; 32])>,
    C: Vec<(usize, usize, [u8; 32])>,
}

// The Challenger's output
struct Solution {
    circuit_star: SpartanInstance,
    y0_pub:       Vec<Scalar>,
    y_star_pub:   Vec<Scalar>,
    proof0:       SNARK,
    proof_star:   SNARK,
}
```

### Participant entry point

```rust
// Implement this function and register it in tig-algorithms/src/zk/
fn solve(challenge: &Challenge, solve_fn: OptimizeCircuitFn) -> anyhow::Result<Solution> {
    // Everything is handled by solve_challenge() — only optimize() needs implementing
    solve_challenge(challenge, optimize)
}

fn optimize(c0: &SpartanInstance) -> SpartanInstance {
    // Your optimization logic here.
    // Use remove_aliases() from tig-circuit-tools as a starting point.
    remove_aliases(c0)
}
```

### End-to-end test

```bash
cd tig-monorepo
cargo test -p tig-challenges --features c007 -- --nocapture
```

The test `test_alias_optimizer_roundtrip` runs the full pipeline (generate → optimize → prove → verify) and is the reference for any new optimizer implementation.

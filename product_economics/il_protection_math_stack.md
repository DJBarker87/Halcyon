# IL Protection — Mathematics & Technology Stack

## Overview

IL Protection prices a 30-day European impermanent loss insurance contract on-chain using a Normal Inverse Gaussian (NIG) density integration engine. The premium is the expected capped payout under the NIG return distribution, computed via 5-point Gauss-Legendre quadrature with the NIG density evaluated analytically through Bessel function approximations. Everything runs in i64 fixed-point arithmetic at SCALE_6 (6 decimal places) inside a single Solana transaction.

---

## 1. The Pricing Problem

Given an LP position in a 50/50 constant-product pool (SOL/USDC on Raydium), compute the fair premium for protecting against terminal impermanent loss between a deductible d and cap c over T days:

```
Premium = E[min(max(IL(X_T) - d, 0), c - d)]
```

where X_T is the log price ratio at expiry and IL(x) = ½(e^{x/2} - 1)² is the IL function in log-space.

The premium is a single number: what fraction of the LP's position value should be charged upfront. It is then multiplied by the ×1.10 underwriting load to produce the quoted premium.

---

## 2. The IL Function

For a 50/50 constant-product pool with entry-normalised price ratio x (in log-space):

```
IL(x) = ½(e^{x/2} - 1)²
```

This is symmetric: IL(x) = IL(-x) for small x (approximately). A ±14% SOL move produces ~1% IL. A ±38% move produces ~7% IL.

The **IL roots** — the log-returns where IL equals a threshold h — are:

```
x_up(h) = 2 · ln(1 + √(2h))     (positive root: SOL rallied)
x_dn(h) = 2 · ln(1 - √(2h))     (negative root: SOL dropped)
```

For the product parameters (d=1%, c=7%):
- Deductible roots: x_up(0.01) ≈ +0.140, x_dn(0.01) ≈ -0.143
- Cap roots: x_up(0.07) ≈ +0.361, x_dn(0.07) ≈ -0.407

---

## 3. The NIG Model

The same NIG model family as the autocall, but with **different tenor-fitted parameters**:

| | Autocall (1-day step) | IL Hedge (30-day tenor) |
|---|---|---|
| alpha | 13.04 | 3.14 |
| beta | 1.52 | +1.21 |
| Calibration | Daily log returns | 30-day log returns |

Lower alpha (3.14 vs 13.04) reflects the fatter tails of 30-day SOL returns compared to daily returns. The NIG distribution at the 30-day horizon is heavier-tailed and more skewed.

The NIG density at point x is:

```
f(x) = (α · δ_T · K₁(α·R)) / (π · R) · exp(δ_T·γ + β·(x - μ))
```

where:
- R = √(δ_T² + (x - μ)²) — distance measure
- K₁ — modified Bessel function of the second kind, order 1
- γ = √(α² - β²) — skew-adjusted shape
- δ_T = σ² · γ³ / α² · T/365 — scale (linear in tenor)
- μ = δ_T · (γ_s - γ) — martingale drift, where γ_s = √(α² - (β+1)²)

---

## 4. Gauss-Legendre Quadrature (5-Point)

The premium integral is evaluated using **5-point Gauss-Legendre quadrature** over four payoff regions:

### Region layout

```
x_dn(c)   x_dn(d)    0    x_up(d)   x_up(c)
  |←—cap—→|←—linear—→|    |←—linear—→|←—cap—→|
  payoff=  payoff=          payoff=    payoff=
  c-d      IL(x)-d          IL(x)-d   c-d
```

Plus tail extensions: left cap continues to x_dn(c) - L, right cap continues to x_up(c) + L, where L = 10σ√(T/365) (10 standard deviations).

### Per region

Each region is mapped to [-1, 1] and evaluated at 5 Gauss-Legendre nodes:

```
∫_{a}^{b} payoff(x) · f_NIG(x) dx  ≈  (b-a)/2 · Σ_{k=1}^{5} w_k · payoff(x_k) · f_NIG(x_k)
```

**Total: 4 regions × 5 nodes = 20 NIG density evaluations.**

### GL5 nodes and weights (at SCALE_6)

```
Nodes:   [-906,180,  -538,469,  0,  +538,469,  +906,180]
Weights: [ 236,927,   478,629,  568,889,  478,629,  236,927]
```

These are the standard 5-point Gauss-Legendre values scaled to SCALE_6.

---

## 5. Bessel Function K₁

Each density evaluation requires K₁(αR). Two approximation paths, same as the autocall engine:

**Large argument (αR ≥ 2):** Abramowitz & Stegun 9.8.8
```
K₁(z) ≈ √(π/(2z)) · e^{-z} · Σ_{n=0}^{6} c_n · (2/z)^n
```

The exponential term combines with the NIG density exponential to avoid computing e^{very large number}:
```
f(x) ∝ e^{-αR + δ_T·γ + β·(x-μ)}
```
The `-αR` from K₁ partially cancels the `+δ_T·γ + β·(x-μ)` terms, keeping the combined exponent manageable.

**Small argument (0 < αR < 2):** Abramowitz & Stegun 9.8.7 + 9.8.1
```
K₁(z) = ln(z/2)·I₁(z) + 1/z + Σ a_n · (z/2)^{2n}
```

where I₁ is the modified Bessel function of the first kind. This path is more expensive (requires ln6) but only fires when x ≈ μ (near the distribution peak).

All 7+7+7 = 21 Bessel coefficients are hardcoded at SCALE_6, sourced from Abramowitz & Stegun Tables 9.8 and 9.8.

---

## 6. Precision-Critical Accumulator

The inner product of the quadrature (5 nodes × payoff × density × weight) must not lose low-order bits. At low sigma, the premium can be as small as ~40 ticks at SCALE_6 (4e-5 = 0.004%). A naive accumulation with per-step SCALE_6 division would round this to zero.

**Solution:** Accumulate the 5-node products at full **i128 width** (effective SCALE_18 = SCALE_6 × SCALE_6 × SCALE_6), then reduce to SCALE_6 only once at the very end:

```rust
let mut acc: i128 = 0;
for k in 0..5 {
    // payoff_k and density_k are i64 at SCALE_6
    // weight_k is i64 at SCALE_6
    // product is i128 at SCALE_18 (three SCALE_6 factors)
    acc += (payoff_k as i128) * (density_k as i128) * (weight_k as i128);
}
let result = (acc / (SCALE_6 as i128) / (SCALE_6 as i128)) as i64;  // reduce to SCALE_6
```

This preserves the low-sigma premiums that would otherwise be destroyed by intermediate rounding.

---

## 7. EWMA Volatility Pipeline

The sigma input to the pricing engine comes from an on-chain 45-day EWMA estimator on SOL log returns, with an off-chain fvol-based regime overlay and a hard floor.

### Time-weighted EWMA

```
variance_new = λ · variance_old + (1 − λ) · r²/Δt_days
```

where:
- λ = exp(−Δt / τ), τ = 45 days (span; equivalent halflife ≈ 31 days)
- r = ln(price_new / price_old) from Pyth oracle
- Δt_days = time between updates in days

Annualised: `σ_annual = √(daily_variance × 365)`.

### Regime-aware pricing sigma

The production pipeline applies a regime multiplier and a hard floor:

| Regime | Condition | Sigma multiplier |
|---|---|---|
| Calm | fvol < 0.60 | × 1.30 |
| Stress | fvol ≥ 0.60 | × 2.00 |

```
σ_pricing = max(σ_ewma45 × regime_multiplier, 0.40)
```

The fvol regime signal is computed off-chain (requires historical vol-of-vol) and written to `RegimeSignal` with keeper authority. The EWMA update itself is permissionless with a rate limit.

---

## 8. Settlement

At expiry, the payout is computed on-chain from the entry and exit oracle prices:

```rust
pub fn compute_settlement(
    weight: u64,              // 0.5 for 50/50 pool (at SCALE)
    entry_price_ratio: u128,  // Always SCALE (= 1.0, entry-normalised)
    current_price_ratio: u128,// P_exit / P_entry (at SCALE)
    position_value_usdc: u64, // Raw USDC amount (6 decimals)
    deductible: u64,          // d at SCALE
    cap: u64,                 // c at SCALE
) -> Result<(u128, u64), SolMathError>
```

1. Compute IL(x) = w·x + (1-w) - x^w using fixed-point pow
2. Compute payout_fraction = min(max(IL - d, 0), c - d)
3. Compute payout_usdc = payout_fraction × position_value / SCALE

The pow_fixed call (x^w for w=0.5 → square root) costs ~14K CU. The entire settlement fits in <20K CU.

### Alternative entry: from raw prices

```rust
pub fn compute_settlement_from_prices(
    weight: u64,
    price_a_now: u128,    // Current SOL oracle price
    price_b_now: u128,    // Current USDC oracle price (≈ $1)
    price_a_entry: u128,  // Entry SOL oracle price
    price_b_entry: u128,  // Entry USDC oracle price
    position_value_usdc: u64,
    deductible: u64,
    cap: u64,
) -> Result<(u128, u64), SolMathError>
```

Computes x = (P_a_now × P_b_entry) / (P_a_entry × P_b_now), then delegates to compute_settlement.

---

## 9. Shield Variant: First-Passage (Path-Dependent)

A separate product variant ("Shield") covers path-dependent maximum IL over a 10-day window, not just terminal IL. This uses a different mathematical engine.

### COS collocation method

Instead of European quadrature, the Shield uses a **discrete double-barrier collocation solver**:

1. **Grid:** N=32 uniform points in the log-space IL corridor [x_dn, x_up]
2. **Value function:** v(x, t) = P(log-spot survives within corridor from time t to T | X_t = x)
3. **Backward iteration:** v(x, t-1) = Σ_j v(x_j, t) · K(x, x_j) · dx
4. **Kernel:** Toeplitz matrix of NIG one-day transition probabilities, computed via 7-point Simpson quadrature
5. **Touch probability:** 1 - v(0, 0) (probability that the corridor was breached)

### Polynomial kernel optimisation

The kernel build requires ~5K CU with precomputed polynomial tables. Each kernel cell is a degree-11 polynomial in sigma:

```
kernel_cell[k] = Σ_{i=0}^{11} coefficients[k][i] × σ^i
```

Three precomputed tables cover Shield 30d/60d/90d at the product barriers. Sigma range: [0.80, 3.00].

**CU cost:** ~930K CU total (kernel build 5K + backward iteration 920K + readout 2K). Fits in a single Solana transaction.

### Validation

Against 5M-path Monte Carlo on a 720-cell grid:
- Shield 30d: 1.6–5.2% relative error
- Shield 60d: similar
- Hedge 14d: 0.7% relative error

---

## 10. On-Chain CU Budget

| Component | CU cost | Notes |
|---|---|---|
| **NIG European premium (Hedge)** | **~300K** | 4 regions × 5 GL nodes × 20 density evals |
| Per-density evaluation (K₁) | ~12K | Large-arg path; small-arg ~18K |
| Settlement | ~20K | Single pow_fixed + comparisons |
| EWMA update | ~10K | One ln + multiply + blend |
| **Shield touch probability** | **~930K** | N=32 collocation, T_days backward steps |
| Shield polynomial kernel | ~4K | 79 poly evals × 30 CU each |
| Threshold solver | ~14K | IL root-finding |

### Transaction budget

Solana compute limit per transaction: 1,400,000 CU (with priority fee).

- Hedge premium + settlement: ~320K — easily fits, room for multiple quotes
- Shield premium: ~930K — fits with margin for settlement
- Hedge + Shield combined: ~1,230K — fits in a single transaction

---

## 11. Fixed-Point Arithmetic (Same Stack as Autocall)

The IL engine shares the SolMath library with the autocall:

| Layer | Precision | Used for |
|---|---|---|
| i64 at SCALE_6 (1e6) | 6 decimal digits | NIG density, quadrature, Bessel functions |
| i128 accumulator | 18 effective digits | Quadrature inner product (prevents rounding floor) |
| u128 at SCALE (1e12) | 12 decimal digits | Settlement, pool operations |
| DoubleWord (hi + lo/SCALE) | Sub-ULP | Power series in settlement |

No floating point is used anywhere on-chain. The entire pricing and settlement pipeline is deterministic fixed-point arithmetic.

---

## 12. What Runs On-Chain vs Off-Chain

| Component | Where | Why |
|---|---|---|
| NIG European premium | **On-chain** | Real-time pricing per quote |
| Shield touch probability | **On-chain** | Real-time pricing per quote |
| Settlement payout | **On-chain** | Oracle-driven at expiry |
| EWMA variance update | **On-chain** | Oracle price feed → sigma |
| Regime classification (fvol) | **Off-chain** | Requires historical vol-of-vol computation |
| Sigma multiplier selection | **Off-chain** | Calm/stress regime → 1.30 or 2.00 |
| NIG alpha/beta calibration | **Off-chain** | Monthly MLE fit on 30-day returns |
| Polynomial kernel generation | **Off-chain** | Monthly recompute if alpha/beta change |
| Underwriting load (×1.10) | **Parameter** | Set at deployment, adjustable by governance |
| Backtest/replay | **Off-chain** | Rust batch engine + Python orchestration |

---

## 13. Comparison: IL Math vs Autocall Math

| Aspect | IL Protection | SOL Autocall |
|---|---|---|
| Core method | GL5 quadrature over NIG density | CTMC backward recursion with NIG kernel |
| Grid dimension | No grid (analytical integration) | 64-node log-spot grid |
| Number of model evaluations | 20 (4 regions × 5 nodes) | ~3,840 per step × 8 steps |
| Barrier handling | Analytical (IL roots) | Grid-based (barrier snapping) |
| Path dependence | None (European endpoint) | Continuous KI via bridge |
| Time steps | 0 (single-period) | 8 (one per observation) |
| CU cost | ~300K | ~809K (Richardson) |
| Fallback path | None needed (always converges) | Direct SVD at 1.76M CU |
| Precision bottleneck | Low-sigma premiums (~40 ticks) | Barrier mismatch at non-default params |
| Bessel K₁ | Same A&S approximation | Same A&S approximation |
| Fixed-point scale | SCALE_6 (i64) | SCALE_6 (i64) |

The IL engine is simpler (no time-stepping, no grid, no bridge correction) but requires the same core numerical primitives: NIG density, Bessel functions, and fixed-point arithmetic.

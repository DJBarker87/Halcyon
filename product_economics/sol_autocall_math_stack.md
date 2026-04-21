# SOL Autocall — Mathematics & Technology Stack

## Overview

The SOL Autocall prices a 16-day barrier-monitored autocallable note on-chain using a two-tier pricing architecture: a **per-product POD-DEIM pricer with keeper-updated reduced operators** (primary) backed by a **gated Richardson CTMC** (fallback). Both tiers run entirely in Rust fixed-point arithmetic on Solana. The production contract includes a **2-day autocall lockout** — autocall is suppressed at the first observation (day 2), guaranteeing every note runs at least 4 days.

The historical **E11 live-operator** path is preserved in the codebase and documented below because it motivated the fixed-product POD/DEIM artefacts, but it is not the shipping primary architecture. Per `research/complexity_reduction_log.md` §11/§12.1, the shipping one-transaction path for the default fixed product is the keeper-fed DEIM solve at about `946K` total CU, while the live-operator E11 path sits around `1.36M` CU and is not the production default.

---

## 1. The Pricing Problem

Given a SOL autocall note with:
- 8 observation dates (every 2 days over 16 days)
- Autocall barrier at 102.5% of entry (suppressed at observation 1)
- Coupon barrier at 100%
- Knock-in barrier at 70% (discrete, observation-date only)

Compute the **fair coupon** q* such that the note has zero net present value at inception:

```
q* = (1 - V_0) / U_0
```

where V_0 = expected redemption value and U_0 = expected coupon count, both evaluated at the ATM state. The note value is linear in the coupon rate, so two backward passes (coupon = 0 and coupon = 1) give q* exactly without bisection.

---

## 2. The NIG Model

SOL returns have fat tails and skew that Black-Scholes cannot capture. The Normal Inverse Gaussian (NIG) distribution models the log-return X over a time step dt as:

```
X ~ NIG(alpha, beta, mu, delta*dt)
```

| Parameter | Role | Production value |
|---|---|---|
| alpha | Tail heaviness (lower = fatter) | 13.04 |
| beta | Skew (positive = right-skewed) | 1.52 |
| delta | Scale per unit time | Solved from sigma |
| mu | Location (drift) | Martingale condition |

The NIG characteristic function is:

```
phi(u) = exp(i*u*mu + delta*dt*(gamma - sqrt(alpha^2 - (beta + i*u)^2)))
```

where gamma = sqrt(alpha^2 - beta^2). The martingale condition sets mu so that E[e^X] = 1 (zero drift for crypto). The variance relationship ties delta to the observable annualised volatility sigma:

```
Var(X) = delta*dt * alpha^2 / gamma^3 = sigma^2 * dt / 365
```

So delta = sigma^2 * gamma^3 / (alpha^2 * 365). The only live input per quote is sigma (EWMA-45); alpha and beta are static.

---

## 3. The Full-Order CTMC Pricing Problem

Discretise the log-spot axis into N states {s_1, ..., s_N}. At each state, the 2-day NIG transition probabilities form an N x N matrix P(sigma):

```
P(sigma)[i,j] = Prob(log-spot moves from state i to state j in one 2-day step)
```

The backward recursion maintains two value vectors — **untouched** (KI never triggered) and **touched** (KI triggered) — and steps backward through 8 observation periods:

```
For t = 8, 7, ..., 1:
    E_u[i] = sum_j P[i,j] * (touched[j] if j <= KI_state else untouched[j])
    E_t[i] = sum_j P[i,j] * touched[j]

    For each state i:
        if autocall_allowed AND s_i >= autocall_barrier:
            untouched[i] = touched[i] = 1 + coupon        (absorbing)
        else:
            coupon_i = coupon if s_i >= coupon_barrier else 0
            untouched[i] = E_u[i] + coupon_i
            touched[i]   = E_t[i] + coupon_i
```

This costs O(N^2) per step, so O(8 * N^2) per coupon pass, and the two-pass solve requires 2 * 8 * N^2 multiply-accumulates total. At N = 50, that is 40,000 operations — feasible on-chain but leaving no room for the delta surface and other overhead. The shipping POD-DEIM method reduces the on-chain step cost to O(d^2) by loading a keeper-updated reduced operator `P_red(σ)` for the live sigma. The historical E11 variant instead pays an extra live operator-evaluation cost `M = 12` at quote time.

---

## 4. Production POD-DEIM Pricer (Primary)

For the default fixed Structure II product, the shipping architecture is the keeper-fed DEIM path recorded in `research/complexity_reduction_log.md`:

```text
Off-chain (keeper, per vol update):
  1. Build the N=50 Zhang-Li transition matrix P(σ)
  2. Project it to P_red = Φᵀ P(σ) Φ  (15×15, 225 entries)
  3. Write P_red to the product's vol-update state

On-chain:
  1. Load Φ, Φ_idx, P_T_inv, masks, and the current P_red(σ)
  2. Run the d=15 DEIM backward pass for the V leg
  3. Run the d=15 DEIM backward pass for the U leg
  4. Compute q* = (1 - V0) / U0
```

Measured authority numbers:

| Config | Inner CU | Total CU |
|---|---|---|
| DEIM d=15, n_obs=8 | 855,763 | 946,482 |
| DEIM d=15, n_obs=8 + deserialization | 962,540 | 964,441 |

This is the one-transaction shipping note. The keeper burden is small: only the `15 x 15` reduced operator (`225 i64`) changes per volatility update.

### 4.1 Historical E11 live-operator path

The historical E11 path keeps the same POD/DEIM basis infrastructure but reassembles the reduced operator from live sigma at quote time by evaluating `M = 12` operator samples on-chain. It is useful as a reference path and a research artefact, but it is not the shipping primary path.

Measured follow-up numbers from the archived research:

| Config | Total CU | Note |
|---|---|---|
| E11 + DEIM d=15 | 1,358,528 | keeper-free, but too expensive for the production 1-tx target |
| DEIM d=15 (§11, keeper) | 946,482 | shipping fixed-product path |

The remainder of this section describes that historical live-operator construction because it explains where the fixed-product DEIM factors came from.

### 4.2 The key observation

As sigma varies, both the transition matrix P(sigma) and the resulting value function V(sigma) change — but they change *smoothly* and along a low-dimensional manifold. A value function that lives in R^50 can be represented accurately in a 15-dimensional subspace. A transition matrix that lives in R^{50x50} varies along only ~12 independent directions as sigma sweeps [50%, 250%].

The POD-DEIM pricer exploits both facts simultaneously:
- **POD** compresses the value function from N = 50 to d = 15 dimensions
- **DEIM** compresses the operator evaluation from N^2 = 2,500 to M = 12 samples

The result is a pricer whose online cost is dominated by 12 NIG CDF evaluations and an 8-step recursion in 15 x 15 matrices.

### 4.3 POD: compressing the value function

**Problem:** The backward recursion produces value vectors V in R^N. We want a low-rank basis Phi in R^{N x d} such that V(sigma) ≈ Phi * v(sigma) for any sigma in the band, where v(sigma) in R^d is a small coefficient vector.

**Construction:** At K training volatilities {sigma_1, ..., sigma_K} (about 25 values spanning [50%, 250%]), run the full N = 50 backward recursion and collect all intermediate snapshots. Each backward pass produces n_obs + 1 = 9 snapshot vectors. With K = 25 training sigmas and two coupon passes, this gives ~450 snapshot vectors in R^50. Stack them as columns of a snapshot matrix S in R^{50 x 450} and compute its thin SVD:

```
S = U * Sigma * V^T
```

The first d = 15 columns of U form the basis Phi. The singular values decay rapidly — the 15th is typically 10^{-6} times the 1st — meaning 15 modes capture essentially all the variation.

**Why it works:** The autocall payoff is piecewise linear (principal + coupon above the autocall barrier, spot-times-principal below if knocked in). As sigma varies, these shapes stretch and shift but don't develop new features. The POD basis captures the small family of shapes that actually appear.

### 4.4 Galerkin projection: the reduced backward recursion

With the POD basis Phi, project the full-order recursion into the d-dimensional subspace. Define the **reduced transition matrix**:

```
P_red(sigma) = Phi^T * P(sigma) * Phi     (d x d matrix)
```

The backward recursion becomes:

```
v_new = P_red(sigma) * v_old + coupon terms     (d x d multiply, not N x N)
```

This is exact to within the POD truncation error. At d = 15, the per-step cost is 15^2 = 225 multiply-accumulates instead of 50^2 = 2,500.

**The catch:** Computing P_red(sigma) requires the full N x N matrix P(sigma), which requires N^2 = 2,500 NIG probability evaluations — exactly the cost we are trying to avoid. This is where EIM comes in.

### 4.5 EIM: compressing the operator

**Problem:** We need P_red(sigma) at runtime without evaluating the full P(sigma).

**Key idea:** The matrix P(sigma) varies smoothly with sigma. Write it as a reference matrix plus a linear combination of perturbation modes:

```
P(sigma) = P_ref + sum_{m=1}^{M} c_m(sigma) * delta_P_m
```

where P_ref is the transition matrix at a reference sigma, and the modes delta_P_m are learned from training data.

**Construction:** At the K training sigmas, flatten each P(sigma_k) - P_ref into a vector of length N^2. Stack these as columns of a deviation matrix D in R^{N^2 x K}. SVD of D gives:

```
D = U_op * Sigma_op * V_op^T
```

The first M = 12 columns of U_op are the operator modes. DEIM (Discrete Empirical Interpolation Method) selects M = 12 "magic" cell indices — specific (row, col) entries of the N x N matrix — that best identify which linear combination of modes is active. These are chosen greedily: pick the cell where the first mode has the largest magnitude, then pick the cell where the second mode has the largest residual after removing the first mode's contribution, and so on.

The selection produces an M x M interpolation matrix B, where B[i,k] = U_op[magic_cell_i, k]. If B is invertible (it always is by the greedy construction), then the coefficients for any new sigma are:

```
c(sigma) = B^{-1} * (P(sigma) - P_ref)  evaluated at the M magic cells only
```

This requires only **M = 12** NIG probability evaluations instead of N^2 = 2,500.

### 4.6 Pre-computed atoms

Since the basis Phi does not depend on sigma, we can pre-compute the projected modes:

```
A_m = Phi^T * delta_P_m * Phi     (d x d matrix, one per mode m)
P_ref_red = Phi^T * P_ref * Phi   (d x d reference)
```

At runtime, the reduced operator assembles as:

```
P_red(sigma) = P_ref_red + sum_{m=1}^{M} c_m(sigma) * A_m
```

This is M scalar-times-matrix additions in d x d space: 12 * 15^2 = 2,700 multiply-adds. Combined with the 12 NIG CDF evaluations for the coefficients and the 8-step backward recursion in 15 x 15, the total online cost is roughly:

```
12 COS CDF evals + 12 * 225 atom assembly + 8 * 225 backward steps
= 12 CDF + 2,700 + 1,800
≈ 12 CDF evals + 4,500 multiply-adds
```

versus the full-order 2 * 8 * 2,500 = 40,000 multiply-adds (plus 2,500 CDF evals to build P). The reduction is roughly **200x** fewer NIG evaluations.

### 4.7 The DEIM projection for payoff application

One subtlety: the backward recursion does not just multiply by P_red. At each observation step, it also applies the payoff logic (autocall absorption, coupon payment, KI state transition). These operations are nonlinear and state-dependent.

In the reduced space, the payoff is applied at the **DEIM interpolation points** — d = 15 states selected by the same greedy algorithm applied to the POD basis Phi. At these points, the value is reconstructed in full space, the payoff is applied, and the result is projected back:

```
v_at_deim = Phi_deim * v          (reconstruct at d points)
apply payoff at each DEIM point   (autocall/coupon/KI logic)
v_new = (Phi_deim)^{-1} * v_at_deim_new    (project back)
```

Phi_deim is the d x d submatrix of Phi at the DEIM rows, which is invertible by construction.

### 4.8 Summary of the POD-DEIM decomposition

| Component | Dimension | What it captures |
|---|---|---|
| Full grid | N = 50 states | The physical log-spot space |
| POD basis Phi | N x d = 50 x 15 | How value functions vary across the sigma band |
| Operator modes U_op | N^2 x M = 2500 x 12 | How the transition matrix varies with sigma |
| Atoms A_m | d x d x M = 15 x 15 x 12 | Pre-projected operator perturbations |
| DEIM points | M = 12 cells | Where to sample P(sigma) at runtime |
| Payoff DEIM | d = 15 states | Where to apply nonlinear payoff logic |

**Offline cost:** K full-order backward passes + 2 SVDs + DEIM selection. Runs once per alpha/beta calibration.

**Historical E11 online cost per quote:** 12 COS CDF evaluations + d x d backward recursion for 8 steps + payoff at d DEIM points. Two passes for the fair coupon linear trick. This is the keeper-free live-operator variant, not the shipping primary path.

---

## 5. Gated Richardson CTMC (Fallback)

When sigma falls outside the POD-DEIM training band [50%, 250%] or the observation structure is non-standard, the system falls back to Richardson-extrapolated CTMC.

**Idea:** The CTMC discretisation error is O(h^2) where h = 1/N (the grid spacing). Running at two grid sizes N1 and N2 and extrapolating cancels the leading error term:

```
fc* = (N2^2 * fc(N2) - N1^2 * fc(N1)) / (N2^2 - N1^2)
```

With N1 = 10 and N2 = 15, this achieves accuracy comparable to a much finer single grid.

**Gating:** If the coarse and fine grids disagree by more than 10% (|fc(N2) - fc(N1)| / fc(N2) > 0.10), the O(h^2) assumption is violated — the error is not converging monotonically, so extrapolation would amplify noise. In this case the system falls back to the N2 = 15 result directly and flags the quote as Low confidence.

---

## 6. The Lockout in the Backward Recursion

All backward passes (POD-DEIM, CTMC, dense COS) share the same lockout logic. The backward loop counts from maturity toward inception:

- Step 0 processes observation 7 (day 14, nearest maturity)
- Step 6 processes observation 1 (day 2, earliest)
- Step 7 is pure propagation from day 2 back to day 0

With `no_autocall_first_n_obs = 1`, step 6 (observation 1) suppresses the autocall absorbing condition. Coupons still pay, KI still latches, but the note is not allowed to terminate early. This is equivalent to pricing a modified payoff where the first observation date is coupon-only.

The effect on the fair coupon: U_0 (expected coupon count) increases because notes that would have autocalled on day 2 now survive to day 4+, collecting at least one additional coupon. V_0 (expected redemption) decreases slightly because some notes that would have escaped via early autocall now face later KI risk. The net effect is a ~7% reduction in q* at typical SOL volatilities (50-200% annualised).

---

## 7. Delta Surface Computation

Hedge deltas require a value surface at every day of the note's life, not just at inception. For day t, the backward recursion starts from maturity and runs only over the (8 - t/2) remaining observation periods.

Each schedule step carries an `obs_index_from_inception` field — the observation number in the full product schedule. This ensures the lockout is applied correctly regardless of the evaluation day: at day 0, observation 1 is in the future and suppressed; at day 3, observation 1 is in the past and the schedule only covers day 4 onward where autocall is allowed.

Delta at each grid state is computed by central difference:

```
delta[i] = (V[i+1] - V[i-1]) / (S[i+1] - S[i-1])
```

Richardson extrapolation (N1 = 10, N2 = 15) is applied to the value and delta surfaces, then the hedge controller interpolates to the current spot price.

---

## 8. The Zhang-Li Barrier-Aware Grid

The CTMC grid uses barrier-aligned regions from Zhang & Li (2012):

| Region | Share | Coverage |
|---|---|---|
| A: Below KI | ~15% of states | log-spot < ln(0.70) |
| B: KI to coupon | ~65% of states | ln(0.70) to 0 |
| C: Coupon zone | 1 state | Straddles ln(1.00) |
| D: Above autocall | ~10% of states | log-spot > ln(1.025) |

Grid boundaries are constrained so that:
- The midpoint of the last B state and the C state falls exactly on the coupon barrier (log = 0)
- The midpoint of the C state and the first D state falls exactly on the autocall barrier (log = ln(1.025))

This eliminates interpolation error at the payoff discontinuities. Without barrier alignment, a 50-state grid can have 5-10 bps coupon error; with alignment, the error is sub-basis-point.

---

## 9. COS Density and CDF Recovery

The NIG transition probabilities are computed using the COS (Cosine Series Expansion) method, which recovers a density or CDF from its characteristic function.

**Density (used by the 64-node dense pricer):**

```
f(x) ≈ sum_{k=0}^{M-1} c_k * cos(k*pi*(x-a)/(b-a))

where c_k = (2/(b-a)) * Re[phi(k*pi/(b-a)) * exp(-i*k*pi*a/(b-a))]
```

The truncation range [a, b] is set to mean +/- 8 standard deviations of the NIG distribution. 17 COS terms are used near maturity, reduced to 12 at earlier steps where the value function has smoothed.

**CDF (used by the CTMC cell probability builder and E11 online):**

```
F(x) = (x-a)/(b-a) + (2/(b-a)) * sum_{k=1}^{M-1} Re[phi(w_k)*e^{-iw_k*a}] * sin(k*pi*(x-a)/(b-a)) / (k*pi/(b-a))
```

A cell probability is P[i,j] = F(boundary_j) - F(boundary_{j-1}), evaluated with the NIG CF shifted to the row representative.

---

## 10. Brownian Bridge KI Correction

The note has discrete observation-date KI checking. Between observations, the probability that the log-spot path crossed the KI barrier is approximated by a Brownian bridge:

```
P(min < barrier | start, end) = exp(-2 * d_start * d_end / variance)
```

where d_start and d_end are the distances from the barrier at the start and end of the step. The variance is inflated by a 1.3x factor to approximate NIG fat tails (heavier than Brownian). This correction applies only in the 64-node dense pricer; the CTMC uses the discrete observation model directly.

---

## 11. Fixed-Point Arithmetic (SolMath)

### SCALE_6 (i64, 6 decimal digits)
The pricing engine's native precision.

| Function | Method | Accuracy |
|---|---|---|
| mul6(a, b) | (a*b)/10^6 via i128 | 1 ULP |
| div6(a, b) | (a*10^6)/b | 1 ULP |
| exp6(x) | Range reduction + Taylor | 1 ULP |
| sqrt6(x) | Newton-Raphson | 1 ULP |
| sincos6(x) | Angle reduction + Horner | 1 ULP |
| nig_cf6 | Complex CF: csqrt, cexp | 2-4 ULP |
| nig_cdf_cos_at | COS series CDF | ~5 ULP |

### SCALE (u128/i128, 12 decimal digits)
Used for settlement, pool math, and the general library.

| Function | Method | Accuracy |
|---|---|---|
| fp_mul(a, b) | (a*b)/10^12, U256 on overflow | 1 ULP |
| ln_fixed_i(x) | Split-table + Remez degree-3 | 3 ULP |
| exp_fixed_i(x) | Remez rational degree-5 | 1 ULP |

### POD-DEIM high-precision assembly
The operator assembly step accumulates in i128 at SCALE_6^3 (= 10^18) to prevent quantization error:

```
P_red[i,j] = P_ref_red[i,j] + sum_m c_m * A_m[i,j]
```

c_m is at SCALE_6^2 (from B^{-1} * delta_P), A_m is at SCALE_6. The product c_m * A_m is at SCALE_6^3. A single final division by SCALE_6^2 recovers the result at SCALE_6, preserving sub-ULP accuracy through the 12-term summation.

---

## 12. Bessel Function K_1

The NIG density involves the modified Bessel function K_1(z). Two fixed-point paths:

**Large z (>= 2):** Asymptotic expansion (Abramowitz & Stegun 9.8.8):
```
K_1(z) ~ sqrt(pi/(2z)) * exp(-z) * P(1/z)
```
where P is a degree-6 polynomial. Covers >95% of evaluations.

**Small z (< 2):** Series representation via I_1 and logarithm:
```
K_1(z) = ln(z/2)*I_1(z) + 1/z + power_series(z)
```

Both are entirely in i64 arithmetic. No floating point on-chain.

---

## 13. What Runs On-Chain vs Off-Chain

| Component | Where | Why |
|---|---|---|
| POD-DEIM solve (shipping, keeper-fed `P_red`) | **On-chain** | d=15 backward recursion using the current reduced operator |
| E11 live-operator solve (historical) | **On-chain** | 12 COS CDF evals + d=15 backward recursion |
| Gated Richardson CTMC | **On-chain** | Fallback outside POD-DEIM sigma band |
| EWMA volatility | **On-chain** | Oracle price feed -> sigma update |
| Settlement | **On-chain** | Oracle prices -> payoff |
| Delta surfaces | **On-chain** | Richardson-extrapolated Markov surfaces |
| POD-DEIM training | **Off-chain** | SVD on training snapshots, DEIM point selection |
| NIG alpha/beta calibration | **Off-chain** | Static (would be MLE if recalibrated) |
| Hedge execution | **On-chain** | DEX swap via CPI |

---

## 14. Production Constants

| Constant | Value | Location |
|---|---|---|
| N_OBS | 8 | autocall_v2.rs |
| no_autocall_first_n_obs | 1 | AutocallTerms::current_v1() |
| KNOCK_IN_LOG_6 | -356,675 (ln 0.70) | autocall_v2.rs |
| AUTOCALL_LOG_6 | 24,693 (ln 1.025) | autocall_v2.rs |
| NIG_ALPHA_1D | 13,040,000 (13.04) | autocall_v2.rs |
| NIG_BETA_1D | 1,520,000 (1.52) | autocall_v2.rs |
| POD-DEIM full grid N | 50 | autocall_v2_e11.rs (E11_LIVE_QUOTE_N_STATES) |
| POD dimension d | 15 | autocall_v2_e11.rs (E11_LIVE_QUOTE_D) |
| DEIM operator samples M | 12 | autocall_v2_e11.rs (E11_LIVE_QUOTE_M) |
| POD-DEIM sigma band | [50%, 250%] | autocall_v2_e11.rs |
| Richardson N1 | 10 | autocall_v2_parity.rs |
| Richardson N2 | 15 | autocall_v2_parity.rs |
| Richardson gap threshold | 10% | autocall_v2.rs |

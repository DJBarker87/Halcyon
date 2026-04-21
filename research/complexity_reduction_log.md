# CTMC Complexity Reduction — Experiment Log

Target: reduce the O(N^2) dense matvec in the NIG CTMC backward pass.
Baseline: N=200 dense CTMC, European ATM put, 16d (8x2d), NIG(13.04, 1.52).

Reference prices (bps): 80%=644.19, 100%=816.12, 117%=961.98, 150%=1244.91, 200%=1636.88.

---

## 1. Operator Splitting — Diffusion + Jump Decomposition

**File:** `research/nig_operator_splitting.py`
**Date:** 2026-04-11
**Idea:** Decompose P_full into tridiagonal diffusion G_diff (O(N) Thomas) + low-rank jump residual P_jump (O(Nr) SVD matvec). Strang splitting: D(h/2) . M_jump . D(h/2).

### Method A — Tridiagonal extraction

Extract the tridiagonal band of G_full = P_full - I.

**Rank analysis (N=50, sigma=117%):**
- P_jump singular values: flat at ~0.88 across all 49 modes. Rank(1e-4) = 49.
- P_full singular values: geometric decay 1.09, 1.03, 0.93, 0.81, ... Rank(1e-4) = 36.
- P_jump is HIGHER rank than P_full. The tridiagonal captures only ~27% of G_full's off-diagonal mass.

**Splitting error:**
- ~23 bps at sigma=117%, constant across N=15..100 (convergence order ~0.0).
- This is an O(1) temporal splitting error, not spatial — does not decrease with grid refinement.

**Rank truncation:**
- Catastrophic. At N=50, rank-6: total error = -860 bps (vs -34 bps for dense N=50).
- Flat SV spectrum means any rank r << N destroys most information.

**Verdict: DEAD.** Tridiagonal captures too little; residual is full rank.

### Method B — Moment-matched FDE diffusion

Build G_diff as a Fokker-Planck discretization matching NIG mean + variance.
FDE has ~20x the off-diagonal mass of G_full (D/dx^2 >> CTMC rates).

**Rank analysis (N=50, sigma=117%):**
- P_jump_fde SVs: 0.59, 0.48, 0.13, 0.13, 0.12, 0.11, ... — clear decay after mode 2.
- Rank(1e-4) = 37 (vs 36 for P_full). Marginal improvement.

**Strang splitting:**
- expm(G_jump_fde) is numerically unstable (G_jump has huge mixed-sign entries). Blows up at N>=30.

**Additive decomposition (v' = CN(G_diff_fde,1).v + P_jump_fde.v):**
- Works at N<=30 (splitting error ~0.5 bps).
- Diverges at N>=50 because CN approximation degrades — FDE entries scale as 1/dx^2, needing O(N^2) sub-steps.

**Verdict: DEAD.** FDE mass mismatch kills both Strang stability and CN accuracy at target grid sizes.

### Root causes

1. NIG at 2-day / alpha=13.04 is "mostly diffusion" — transition mass spreads broadly across many grid cells. The tridiagonal band captures almost nothing.
2. The Gaussian component dominates the NIG so thoroughly that subtracting it leaves a residual with ~same rank as P_full. Excess kurtosis is not a low-rank perturbation.
3. Operator splitting is temporal (O(dt^2) per step at fixed dt=1), not spatial — error doesn't decrease with N.

---

## 2. Chebyshev-Lobatto Spectral Grid — Exponential Convergence Test

**File:** `research/chebyshev_ctmc_convergence.py`
**Date:** 2026-04-11
**Idea:** Replace the uniform grid with Chebyshev-Lobatto nodes on [ln 0.70, ln 1.025] and represent the transition operator via Lagrange interpolation against high-order Gauss-Legendre quadrature. If the value function is analytic on the domain, convergence should be exponential (O(r^{-N})) instead of algebraic (O(N^{-2})).

### Setup

- N Chebyshev-Lobatto nodes on [A, B] = [ln 0.70, ln 1.025], endpoints included.
- Spectral transition matrix: T[i,j] = ∫_A^B L_j(z) f_NIG(z − x_i) dz via 300-point Gauss-Legendre.
- Tail probabilities (exits below KI, above AC) handled identically for both methods.
- Barycentric interpolation at ATM (x=0) for Chebyshev; linear interpolation for uniform.
- Ground truth: uniform cell-based CTMC at N=200.
- NIG parameterisation: α=1/σ, β=−0.5/σ, δ=σ, μ=0 (shape fixed, scale varies).
- Tested N = 5, 7, 9, 11, 13, 15, 20, 25, 30, 50 at σ = 80%, 100%, 117%, 150%, 200%.

### Results — N=15 accuracy (digits vs N=200 ground truth)

| σ | Chebyshev | Uniform | Δ |
|---|-----------|---------|---|
| 80% | 1.99 | 2.49 | −0.50 |
| 100% | 2.09 | 2.85 | −0.76 |
| 117% | 2.16 | 2.45 | −0.28 |
| 150% | 2.28 | 2.26 | +0.02 |
| 200% | 2.41 | 2.20 | +0.21 |

### Convergence rate (fit over N=5..50)

| σ | Method | Algebraic slope | Exp slope/node | Verdict |
|---|--------|-----------------|----------------|---------|
| 80% | Cheb | −0.41 | −0.023 | algebraic |
| 80% | Unif | −1.37 | −0.051 | algebraic |
| 117% | Cheb | +0.15 | −0.005 | algebraic (non-convergent) |
| 117% | Unif | −1.25 | −0.050 | algebraic |
| 200% | Cheb | −0.50 | −0.028 | algebraic |
| 200% | Unif | −1.19 | −0.051 | algebraic |

### Diagnostics

- T_min always positive (no negative-probability pathology).
- Row sums exact to machine precision (quadrature is accurate).
- Chebyshev errors oscillate wildly with N (classic sign of polynomial struggling with a non-smooth target).

### Root cause: coupon barrier kink at x = 0

The value function has a derivative discontinuity at x = 0 (= ln 1.00, the coupon barrier), which sits in the interior of [ln 0.70, ln 1.025] at 93.5% of the domain width. At each observation date, the backward recursion enforces a coupon payment above x = 0 but not below, creating a kink in V(x). Polynomial approximation of functions with interior kinks converges at O(N^{-1}) at best — the Chebyshev basis offers no advantage over uniform for non-smooth functions. The oscillating errors are a Gibbs-adjacent effect: as N changes, nodes shift relative to the kink, causing unpredictable over/undershoots.

The NIG convolution smooths the kink slightly at each backward step, but re-enforcement at every observation re-introduces it. The net effect is a value function that is C^0 but not C^1 at x = 0, killing spectral convergence.

### Verdict: DEAD.

Chebyshev-Lobatto spectral discretisation does not improve convergence for the autocall. The value function is intrinsically non-smooth (coupon kink, KI/AC enforcement). Uniform cell-based CTMC converges more reliably (~O(N^{-1.3})) with smoother error decay.

Domain decomposition (split at x = 0 into two Chebyshev grids) could recover spectral convergence per sub-domain, but the narrow [0, ln 1.025] segment (width 0.025) would need very few nodes, making the overhead hard to justify over a simple uniform grid.

**Plots:** `research/cheb_ctmc_conv_semilog.png`, `research/cheb_ctmc_conv_loglog.png`

---

## 3. Filtered Fourier / Wiener-Hopf Benchmark for the Autocall

**File:** `research/autocall_wiener_hopf_benchmark.py`
**Date:** 2026-04-11
**Idea:** Replace the CTMC grid benchmark with a filtered Fourier/Hilbert barrier engine closer to Green-Fusai-Abrahams / Feng-Linetsky. Decompose the 8-date autocall into:

1. coupon count = sum of single upper-barrier digital legs
2. redemption shortfall = upper-barrier put minus double-barrier put

Each leg is priced by propagating in Fourier space with the NIG characteristic function and applying barrier projections via Plemelj-Sokhotski / Hilbert transforms.

### Why this path

The direct two-state strip recursion from the earlier exploratory script was not accurate enough: at `sigma=117%` it landed around `246 bps` versus `321 bps` for CTMC `N=200`.

The payoff decomposition is materially better conditioned:

- coupon legs only need an upper-barrier survival projector
- shortfall separates into one upper-barrier leg and one double-barrier leg
- the filtered Hilbert projector is numerically stable on a large uniform log-price grid

### Numerical setup

- Grid: uniform log grid on `[-x_max, x_max]`
- Default production benchmark run: `M = 2^15 = 32768`, `x_max = 8`
- NIG step: 2-day increment with `alpha=13.04`, `beta=1.52`
- Filter: exponential spectral filter, order 12
- Damping used in the final stable run:
  - digital legs: `a = +0.25`
  - put legs: `a = -0.03`

### Factorization diagnostic

The Wiener-Hopf factorization of `Phi(ξ, q) = 1 - q Ψ(ξ)` is numerically clean:

- max residual `|Phi - Phi+ Phi-| = 4.442e-16`

So the remaining pricing error is not coming from the factor split itself. It is dominated by truncation, filtering, and payoff/barrier discretization.

### Accuracy vs CTMC N=200

Default run (`M=32768`, `x_max=8`):

| σ | WH/FL benchmark | CTMC N=200 | Error |
|---|------------------|------------|-------|
| 80% | 60.80 bps | 62.03 bps | -1.23 bps |
| 100% | 176.54 bps | 179.19 bps | -2.65 bps |
| 117% | 318.76 bps | 321.27 bps | -2.50 bps |
| 150% | 634.95 bps | 632.54 bps | +2.41 bps |
| 200% | 1085.33 bps | 1069.96 bps | +15.36 bps |

At the production vol point (`117%`) the benchmark converges monotonically upward:

| M | Coupon |
|---|--------|
| 4096 | 310.89 bps |
| 8192 | 315.83 bps |
| 16384 | 317.50 bps |
| 32768 | 318.76 bps |

That is strong evidence the filtered Fourier/Hilbert engine is a better reference benchmark than CTMC `N=200`, especially around the launch-vol region.

### Complexity

Per quote at the default benchmark settings:

- `204` FFT evaluations
- about `5.16e8` floating-point ops

This is absolutely not an on-chain path. It is a desktop reference pricer only.

### Verdict

**ALIVE as benchmark, DEAD as complexity reduction.**

- **Benchmark:** yes. This should replace CTMC `N=200` as the stronger offline reference for the autocall.
- **On-chain:** no. The runtime is orders of magnitude too large for Solana quote-path use.
- **Shipping path:** still the small-grid CTMC / Richardson design in Rust.
- **Research value:** high. This gives a more defensible ground truth when checking CTMC discretization bias.

---

## 4. PROJ Method — Frame Duality FFT (Kirkby 2015)

**File:** `research/proj_autocall_benchmark.py`
**Date:** 2026-04-11
**Idea:** Replace the CTMC matvec entirely with FFT-based transition. For Lévy processes the transition operator is diagonal in Fourier space — multiply the value-function DFT by the NIG characteristic function, then IFFT back. Total work per step: one size-M FFT pair + M complex multiplications. Simplest variant: Shannon wavelet (sinc) basis on a uniform grid. Test whether M=32 or M=64 can match CTMC N=200 accuracy, making a fixed-point FFT on Solana viable.

### Setup

- Product: S0=1, AC=1.025, KI=0.70, 8 bi-monthly observations (h=1/6 year), T=4/3 years.
- NIG shape from SOL calibration: α=13.04, β=1.52, γ=12.951. Scale (δ) from σ_ann.
- Uniform grid x ∈ [−L, L), dx=2L/M, with x=0 exactly on grid (M even).
- Transition: V_new = IFFT(FFT(V) · φ(ω)).real where φ is the NIG CF at FFT frequencies.
- Two-pass fair-coupon solve identical to CTMC backward induction.
- Ground truth: CTMC N=200 (COS-method CDF + Toeplitz matvec). Sanity check: MC 2M paths.
- Also tested: barrier-snapped variant (shift grid so AC_LOG falls on a grid point).

### References (CTMC N=200 vs MC 2M paths)

| σ | CTMC(200) | MC(2M) | Δ |
|---|-----------|--------|---|
| 80% | 2807.5 | 2793.1 | +14.5 |
| 100% | 3620.4 | 3611.0 | +9.4 |
| 117% | 4354.5 | 4345.9 | +8.5 |
| 150% | 5906.4 | 5905.0 | +1.4 |
| 200% | 8630.7 | 8634.6 | −3.9 |

Note: these are for the 8 bi-monthly product (T=1.33y), not the 16-day SOL autocall. CTMC(200) agrees with MC to ~10 bps across all vols. CTMC N=300 differs by only 4.7 bps at σ=117%.

### PROJ relative error vs CTMC(200)

| σ | M=16 | M=32 | M=64 | M=128 | M=256 | M=512 |
|---|------|------|------|-------|-------|-------|
| 80% | 65.0% | 44.3% | 25.1% | 13.6% | 7.2% | 4.1% |
| 100% | 65.5% | 43.4% | 25.1% | 13.6% | 7.2% | 3.8% |
| 117% | 65.9% | 43.8% | 25.4% | 13.8% | 7.3% | 3.8% |
| 150% | 66.9% | 44.7% | 26.0% | 14.2% | 7.5% | 3.9% |
| 200% | 68.3% | 46.0% | 27.0% | 14.8% | 7.8% | 4.0% |

Error is remarkably stable across σ — almost entirely determined by M, not by volatility or aliasing.

### Convergence order

- Standard PROJ: error ~ M^{−0.88} (fitted over M=64..2048 at σ=117%).
- AC-snapped PROJ (grid shifted so autocall barrier on a grid point): error ~ M^{−1.18}.
- CTMC (Zhang-Li grid): error ~ N^{−2}.

Barrier snapping improves convergence from sub-first-order to slightly super-first-order, but never approaches the CTMC's quadratic rate.

### Barrier-snapped improvement (error bps: standard → snapped)

| σ | M=64 | M=128 | M=256 | M=512 |
|---|------|-------|-------|-------|
| 117% | −1105 → −960 | −602 → −410 | −318 → −95 | −166 → +74 |
| 200% | −2332 → −2156 | −1277 → −1039 | −671 → −392 | −345 → −42 |

Snapping the AC barrier to a grid point gives ~3× improvement at M=256 but does not change the convergence order.

### Diagnostics

- **Transition accuracy**: constant preservation exact (1.00000000). Martingale (exp(x)) at ATM: ratio 0.99999685. The FFT convolution is spectrally accurate — the bottleneck is purely spatial.
- **Aliasing**: at M=64 (σ=100%), NIG CF at Nyquist ≈ 10^{−6}. Aliasing is negligible. Yet error is 25%. Confirms the error source is barrier discretization, not spectral truncation.
- **Ultra-fine convergence**: M=1024 → 2.1%, M=2048 → 1.2%, M=4096 → 0.7%. Method IS convergent, just very slowly.
- **Grid density at barriers**: the autocall-coupon gap is ln(1.025) − ln(1.0) = 0.0247. For dx < this gap: M > 2L/0.0247 ≈ 657 (σ=117%). The uniform grid cannot resolve the gap below M ≈ 640.

### Root cause: the 2.5% barrier gap

The autocall barrier at ln(1.025) = 0.0247 sits only 2.5% above the coupon barrier at ln(1.0) = 0. This is a "thin barrier" — the gap between two different payoff regimes (coupon vs autocall) is far smaller than any reasonable grid spacing at M ≤ 512.

The CTMC Zhang-Li grid handles this with **1 dedicated state** in Region C (the single cell spanning [ln 1.00, ln 1.025]), with the representative placed so both barriers are exact midpoints. The uniform PROJ grid needs 600+ points to achieve the same spatial resolution.

The sinc basis compounds the problem: the payoff discontinuity at the autocall boundary produces Gibbs oscillations that propagate through the FFT convolution at each observation step. This is the same kink mechanism that killed the Chebyshev experiment (§2), now manifesting through Gibbs oscillations in the sinc basis rather than polynomial Runge effects.

### Computational cost comparison

| Method | Grid | Error (σ=117%) | Complex ops | Est. Solana CU |
|--------|------|----------------|-------------|----------------|
| CTMC N=15+Rich | 15 | ~4% | ~8,000 | 809K |
| PROJ M=32 | 32 | 44% | 6,144 | 49K |
| PROJ M=64 | 64 | 25% | 14,336 | 114K |
| PROJ M=512 | 512 | 4% | 163,840 | 1,310K |

PROJ M=64 costs 2× the ops of CTMC N=15+Rich but gives 6× worse accuracy. PROJ M=512 matches CTMC accuracy but costs 20× more ops and barely fits 1.4M CU with no room for overhead.

### Verdict: DEAD for this product at M ≤ 512.

The sinc-basis PROJ with uniform grid is fundamentally unsuited for the autocall barrier structure. The 2.5% AC-coupon gap defeats any uniform grid at practical M. CTMC N=15+Richardson at 809K CU remains optimal.

**Could a non-uniform PROJ (B-spline basis + adapted grid) work?** Possibly — Kirkby's higher-order variants use cubic B-splines with barrier-adapted grids. But this breaks the FFT structure (non-uniform DFT needed), which eliminates the CU advantage that motivated the experiment.

**Remaining viable candidates for sub-quadratic transition:** Toeplitz FFT and COS-factored matvec (both exploit uniform grid structure without the barrier-resolution problem, since they keep the CTMC grid).

---

## 5. Wavelet Compression vs Exact FFT Toeplitz Matvec

**Files:** `research/autocall_wavelet_fft_study.py`, `research/autocall_wavelet_fft_results/`, `programs/autocall-bench/src/lib.rs`, `programs/autocall-bench/bench_cu.ts`  
**Date:** 2026-04-11  
**Idea:** Attack the dense Toeplitz matvec directly. Test whether:

1. `W P W^T` is sparse enough under an orthonormal wavelet basis to replace `P.f` with a thresholded sparse wavelet-domain matvec, and
2. the exact Toeplitz matvec can instead be done by size-`2N` circulant embedding + fixed-point FFT on Solana.

### Setup

- Product baseline: SOL autocall Toeplitz recursion only, using the current `autocall_v2`-style NIG kernel setup.
- NIG shape: `alpha = 13.04`, `beta = +1.52`
- Step size: 2 days
- Grid: Zhang-Li style state allocation; Toeplitz spacing from the B-region width
- Matrix size for compression study: `N = 200`
- Sigmas: `80%`, `100%`, `117%`, `150%`, `200%`
- Wavelets: Haar and Daubechies-4 (`db4`), periodic multilevel transform
- Sparsity cutoffs for visualisation: relative to `max(|W P W^T|)` at `1e-6`, `1e-8`, `1e-10`
- Thresholding cutoffs for compressed pricing: `epsilon = 1e-3, 1e-4, 1e-5, 1e-6`

Absolute fair-coupon levels in this section come from the raw Toeplitz-recursion compression harness. They are suitable for relative error comparisons, not as the repo's authoritative autocall quote path.

### A. Wavelet-domain sparsity

#### Haar

Compression is poor. At relative threshold `1e-6`, Haar still keeps:

- `85.47%` of entries at `sigma = 80%`
- `91.06%` at `100%`
- `94.35%` at `117%`
- `97.71%` at `150%`
- `98.61%` at `200%`

At `1e-8`, Haar is effectively dense (`99.9%` to `100%` nonzero). This kernel is too smooth and too wide for Haar's piecewise-constant basis to give meaningful compression.

#### Daubechies-4

`db4` is materially better. At relative threshold `1e-6`, `db4` keeps:

- `19.82%` of entries at `sigma = 80%`
- `16.03%` at `100%`
- `13.05%` at `117%`
- `9.94%` at `150%`
- `7.72%` at `200%`

At `sigma = 117%`, the `db4` sparsity curve is:

- `13.05%` at `1e-6`
- `35.72%` at `1e-8`
- `61.19%` at `1e-10`

This is the first complexity-reduction idea in this log that actually produces strong structural compression of the NIG Toeplitz kernel.

### B. Thresholded compressed matvec error

#### Haar

- Mean nonzero fraction at `epsilon = 1e-6`: `93.4%`
- Mean absolute fair-coupon error at `epsilon = 1e-4`: `0.869 bps`
- Worst fair-coupon error at `epsilon = 1e-3`: `12.42 bps`

Conclusion: Haar only becomes accurate when it is almost dense, so it is not a useful complexity reduction path.

#### Daubechies-4

- Mean nonzero fraction at `epsilon = 1e-6`: `13.3%`
- Mean absolute fair-coupon error at `epsilon = 1e-4`: `0.322 bps`
- Worst fair-coupon error at `epsilon = 1e-4`: `0.595 bps`
- Worst fair-coupon error at `epsilon = 1e-3`: `9.58 bps`

Representative `sigma = 117%` row:

- `epsilon = 1e-4`: `nnz_fraction = 4.51%`, fair-coupon error `-0.478 bps`
- `epsilon = 1e-5`: `nnz_fraction = 7.72%`, fair-coupon error `-0.136 bps`
- `epsilon = 1e-6`: `nnz_fraction = 13.05%`, fair-coupon error `+0.0025 bps`

The actual backward-recursion value functions are slightly easier than random vectors: for `sigma = 117%`, `db4` at `epsilon = 1e-4` gives maximum relative L2 error `~2.15e-5` on the stored recursion value-function inputs.

### C. Exact FFT Toeplitz matvec

Toeplitz matvec was implemented exactly by circulant embedding into size `2N`, rounded up to the next power of two for FFT.

For `N = 50`:

- FFT size: `128`
- Complex multiplies per matvec: `1024`
- Complex adds per matvec: `1792`
- Rough real-op equivalent: `4096` multiplies + `5632` adds

Python exactness check:

- max absolute error: `~4e-17` to `8e-17`
- relative L2 error: `~2.3e-16` to `3.3e-16`

So numerically the FFT path is exact for this purpose.

### D. Raw operation-count crossover

Comparing dense `O(N^2)` scalar work against FFT `O(N log N)` scalar work:

- `N = 50`: FFT is still worse (`9728` scalar ops vs `5000`, ratio `1.95x`)
- `N = 100`: still slightly worse (`1.10x`)
- crossover occurs around `N ~= 105`
- `N = 128`: FFT wins (`0.67x`)
- `N = 200`: FFT wins in raw arithmetic count (`0.61x`)

So the asymptotic crossover exists, but it is not yet in the small-`N` regime relevant to the current Solana quote path.

### E. On-chain fixed-point FFT benchmark

A local Anchor benchmark program was built with:

- Q20 fixed-point complex arithmetic
- hardcoded `N = 50` Toeplitz kernel at `sigma = 117%`
- hardcoded size-128 FFT spectrum
- precomputed twiddles and stack scratch arrays to avoid measuring one-time setup overhead

Measured on local validator:

- `bench_dense_n50_single`: `240,386 CU`
- `bench_fft_n50_single`: `393,077 CU`
- `bench_backward_dense_n20`: `1,127,499 CU`

Stress benchmarks under the `1.4M CU` cap:

- `bench_dense_n50_x16`: fails
- `bench_fft_n50_x16`: fails
- `bench_backward_dense_n50`: fails

This answers the practical Solana question:

- **Yes**, a fixed-point size-128 FFT matvec fits on Solana.
- **No**, it is not a drop-in complexity win at `N = 50`; one exact FFT matvec costs about `1.64x` the CU of the dense direct matvec.

### Root cause

The FFT path has better asymptotics but substantial constant factors:

1. bit-reversal and staged butterfly passes
2. complex fixed-point multiplies in every stage
3. size inflation from `N = 50` to FFT size `128`

At the current autocall grid sizes, those constants dominate. The dense direct Toeplitz kernel is still cheaper for one matvec.

### Verdict

Split verdict:

- **Haar wavelets: DEAD.** Too little compression.
- **`db4` wavelets: ALIVE.** This is the first credible compression path in the log. `epsilon = 1e-4` gives roughly `20x` matrix compression at `sigma = 117%` with sub-`0.5 bp` fair-coupon error.
- **Exact FFT Toeplitz: ALIVE as primitive, DEAD as standalone quote-path fix.** The size-128 fixed-point FFT fits on Solana and is exact, but a full autocall backward recursion would need many such matvecs, so exact FFT alone does not bring the one-instruction quote path under `1.4M CU`.

### F. Follow-up benchmark: actual `db4` sparse operator on-chain

Follow-up implementation:

- `research/export_db4_bench_data.py` generates the fixed-point benchmark constants
- `programs/autocall-bench/src/db4_n200_generated.rs` contains the thresholded operator for:
  - `N = 200`
  - `sigma = 117%`
  - `epsilon = 1e-4`
  - `nnz = 1805` (`4.51%` matrix fill)
- `programs/autocall-bench/src/lib.rs` benchmarks:
  - exact dense `N=200` Toeplitz matvec
  - `db4` compressed `W^T S W` matvec
  - full dense and compressed backward recursions

Host-side fixed-point validation:

- `db4` forward/inverse roundtrip passes with max absolute drift `<= 8` in `SCALE_6`
- compressed single-matvec relative L2 error vs dense: `< 8e-4`
- fixed-point full-recursion fair-coupon drift: about `-0.82 bps` vs fixed dense
- split-recursion reformulation:
  - principal leg stays two-state (`touched` / `untouched`)
  - coupon annuity leg collapses to one state because coupons do not depend on knock-in history
  - matvec count drops from `32` to `24` per full fair-coupon solve at `8` observation dates
  - host-side split vs original compressed recursion drift: `0` on principal `V0`, `5` in annuity units (`SCALE_6`), about `+0.105 bps` on fair coupon

Local validator results:

- `bench_db4_n200_single`: `636,900` inner CU, `660,122` total CU
- `bench_dense_db4_n200_single`: exceeds `1.4M CU`
- `bench_backward_dense_db4_n200`: exceeds `1.4M CU`
- `bench_backward_db4_n200`: exceeds `1.4M CU`
- `bench_split_dense_db4_n200`: still exceeds `1.4M CU` (`1399850 / 1399850` consumed)
- `bench_split_db4_n200`: still exceeds `1.4M CU` (`1399850 / 1399850` consumed)

Interpretation:

- the compressed `db4` single matvec is a real Solana primitive; it fits with room to spare
- the exact dense `N=200` matvec does not fit at all
- splitting the recursion into principal plus coupon-annuity legs is algebraically clean and preserves pricing to within fixed-point noise
- but even after that `25%` matvec reduction, `db4` compression by itself is still not enough to make the full `N=200` 8-date backward recursion a one-instruction quote path

### G. Split recursion: principal leg + coupon annuity leg

The original fair-coupon solve effectively runs the backward recursion twice:

- once with coupon `q = 0`, to get the principal / redemption value `V0`
- once with coupon `q = 1`, to get `V1`
- then fair coupon is recovered from the annuity `A = V1 - V0`

That formulation is correct, but it duplicates a lot of work.

The useful observation is that the knock-in state matters for principal, but not for coupon accrual:

- the principal leg needs two state vectors because the maturity payoff depends on whether knock-in has ever been touched
- the coupon annuity leg does **not** need the touched / untouched split, because coupon payments depend only on observation-date spot and autocall status, not on knock-in history

So the recursion can be rewritten as:

- principal solve: two-state backward pass
- coupon annuity solve: one-state backward pass
- fair coupon: `q* = (1 - V0) / A`

At `8` observation dates, this reduces the required matvec count from:

- `32` matvecs in the original two-pass touched/untouched formulation
- to `24` matvecs in the split formulation

That is a real structural saving, not just a micro-optimisation.

Fixed-point validation on the host shows the rewrite is essentially exact for this benchmark:

- principal `V0` matches exactly at the tested scale
- annuity drift is `5` units in `SCALE_6`
- fair coupon drift is about `+0.105 bps`

On-chain, however, the saving is still far too small relative to the `db4` matvec cost:

- a single compressed `db4` `N=200` matvec still costs about `660k` total CU
- both `bench_split_dense_db4_n200` and `bench_split_db4_n200` still run straight into the `1.4M CU` ceiling

So the conclusion from this branch is:

- the split recursion is the right algebraic cleanup and should be kept
- but it does **not** change the product decision
- `db4` compression remains viable as a reusable matvec primitive, not as a complete one-instruction quote-path solution for `N=200`

### Shipping implication

If the goal is to raise feasible `N` inside the Solana CU ceiling, the best next path is not exact FFT by itself. It is:

1. sparse `db4` wavelet-domain matvec benchmark on-chain, and/or
2. low-rank / compressed recursion that exploits the smooth monotone Toeplitz kernel directly.

Exact FFT remains useful as a clean reference implementation and as a possible building block if the quote path is split across instructions or if larger off-chain / hybrid execution is acceptable.

---

## 6. SVD Low-Rank Toeplitz + Asymmetric Leg Decomposition

**Files:** `research/toeplitz_lowrank_analysis.py`, `programs/autocall-bench/src/lib.rs`, `programs/autocall-bench/bench_cu.ts`
**Date:** 2026-04-11
**Idea:** The NIG Toeplitz kernel P is numerically low-rank. Factor P ≈ U'·Vᵀ (rank r) via offline SVD and replace the O(N²) matvec with two O(Nr) products on-chain. Decompose the two-pass fair-coupon solve into its constituent legs — V (principal/redemption, smooth, low rank) and U (coupon annuity, sharp, higher rank) — and allocate rank asymmetrically.

### SVD rank analysis (N=200 uniform grid on [ln 0.70, ln 1.025])

NIG kernel at (α=13.04, β=1.52), σ swept 80–200%. Kernel K[j] = NIG_PDF(j·dx)·dx, normalised, formed into full Toeplitz matrix, SVD computed.

**Singular value decay is sigma-dependent:** low vol (peaked kernel, more independent modes) needs higher rank than high vol (broad kernel, fewer modes). At σ=200%, rank 5 suffices for sub-bps. At σ=80%, rank 17 is needed.

**Per-leg rank requirements (|relative error| < 0.01%):**

| σ_ann | V leg rank | U leg rank | q\* < 1 bp rank |
|-------|:----------:|:----------:|:---------------:|
|  80%  |     7      |     17     |       17        |
|  90%  |     7      |     15     |       15        |
| 100%  |     6      |     14     |       14        |
| 110%  |     6      |     12     |       13        |
| 117%  |     6      |     12     |       12        |
| 130%  |     5      |     11     |       11        |
| 150%  |     5      |      9     |       10        |
| 200%  |     4      |      6     |        8        |

**V converges at rank 5–7 across all σ. U is the bottleneck at rank 11–17.** This is physically correct: V prices a smooth redemption payoff; U counts discrete coupon payments gated by barrier indicators, creating sharper features that need more singular components.

### BPF arithmetic findings

Three critical discoveries from on-chain benchmarking:

1. **`wrapping_mul` vs checked multiply.** With `overflow-checks = true` (workspace default), plain `a * b` compiles to a checked multiply on BPF that internally uses i128 emulation (~86 CU per multiply-add). `a.wrapping_mul(b)` compiles to a single `mul64` (~30 CU). The product SCALE_20 × SCALE_6 ≤ 10¹² fits in i64 (max 9.2×10¹⁸), so wrapping is safe. **This alone cut dense N=20 from 1.18M to 374K CU (3.2× speedup).**

2. **`unsafe get_unchecked` is 3–5% slower.** Raw pointer access blocks LLVM aliasing analysis and loop optimisations. The SBF backend elides bounds checks when it can prove index is in-range from loop bounds. Safe Rust wins.

3. **`/ SCALE_6` (i64 signed division) crashes on SBF.** Platform-tools v1.48 does not support `sdiv64` correctly. `>> 20` bit-shift is the only viable fixed-point path.

### On-chain CU measurements (solana-test-validator 2.3.0, NUC localnet)

#### Dense Toeplitz baseline

| N   | Inner CU  | % Budget | Status |
|-----|----------:|---------:|--------|
|  20 |   373,533 |    27%   | OK     |
|  50 |    >1.4M  |     —    | FAIL   |

#### Uniform low-rank

| N   | r  | Inner CU    | % Budget | Status |
|-----|---:|------------:|---------:|--------|
|  50 |  8 |     674,000 |    48%   | OK     |
|  50 | 12 |     959,136 |    69%   | OK     |
|  50 | 17 |   1,314,016 |    94%   | OK     |
|  75 |  8 |   1,007,656 |    72%   | OK     |
| 100 |  8 |   1,334,608 |    95%   | OK     |

CU model: per-pass cost = 52K fixed + 35.6K per rank unit.

#### Asymmetric rank (V leg at r_v, U leg at r_u)

| N   | r_v | r_u | Inner CU  | % Budget | Saving vs uniform r_u |
|-----|----:|----:|----------:|---------:|----------------------:|
|  50 |   6 |  12 |   732,353 |    52%   | 24% vs uniform r=12   |
|  50 |   6 |  17 |   905,966 |    65%   | 31% vs uniform r=17   |
|  75 |   6 |  12 | 1,094,551 |    78%   | —                     |

### Production recommendation

**N=50, r_v=6, r_u=17: 906K CU (65% of budget)**

- Sub-bps fair coupon accuracy at every production σ (80–200%)
- 2.5× grid resolution vs current N=20 dense pricer
- Leaves 494K CU for factor loading from PDA, settlement logic, and margin
- SVD computed offline; on-chain loads precomputed U' (50×17) and Vᵀ (17×50)
- Factor storage: 1,700 i64 entries × 2 = 27.2 KB

#### Alternatives by use case

| Scenario                  | Config          |  CU   | Accuracy          |
|---------------------------|-----------------|------:|-------------------|
| Max headroom              | N=50 v6/u12     | 732K  | <1 bp at σ≥110%   |
| Universal sub-bps         | N=50 v6/u17     | 906K  | <1 bp all σ       |
| Higher grid resolution    | N=75 v6/u12     | 1.09M | <1 bp at σ≥110%   |
| Sigma-adaptive (runtime)  | N=50 v6/u(σ)    | 732K–906K | <1 bp always  |

### Architecture

```
Off-chain (quote server):
  1. NigParams6::from_vol(σ, α, β)
  2. build_nig_toeplitz_kernel(nig, 50, contract) → kernel [99 entries]
  3. Form 50×50 Toeplitz P from kernel
  4. SVD(P) → U, S, Vt
  5. U' = U[:,:17] · diag(S[:17])  → 50×17 at SCALE_20
  6. Vt_trunc = Vt[:17,:]          → 17×50 at SCALE_20
  7. Serialise (U', Vt) to PDA

On-chain (single transaction, 906K CU):
  1. Load U'[50×17], Vt[17×50] from PDA
  2. Pass 0 (V leg, rank 6):  temp = Vt[:6,:] · f;  e = U'[:,:6] · temp
  3. Pass 1 (U leg, rank 17): temp = Vt · f;         e = U' · temp
  4. q* = (1 − V) / U
  5. Emit fair coupon, apply margin, gate issuance
```

### Verdict: ALIVE — shipping candidate.

First complexity reduction in this log that simultaneously:
- fits the full two-pass autocall solve in a single Solana transaction
- achieves sub-bps accuracy across the entire production vol range
- raises the grid from N=20 to N=50 (2.5× resolution)
- leaves meaningful CU headroom (35% of budget)

The key insight is not the SVD itself (the kernel is only moderately low-rank at low vol) but the **leg decomposition**: V and U have very different rank requirements, and pricing them at asymmetric rank saves 24–31% CU compared to uniform rank.

---

## 7. Non-Uniform Grid Experiments — N=15 vs SVD N=50

**File:** `research/nonuniform_grid_experiments.py`
**Date:** 2026-04-11
**Idea:** Test whether a smarter non-uniform grid at N=15 can match N=50 uniform accuracy, which would eliminate the SVD machinery entirely — just a single dense 15×15 transition matrix, ~640K CU, simpler architecture. The hypothesis: the uniform grid wastes degrees of freedom in smooth regions and undersamples near the barriers and spot. A non-uniform grid that clusters points where the value function has structure should converge much faster.

### Approaches tested

1. **Tanh/sinh-clustered grid** (β=2, 4, 8): density-function inversion placing more nodes near KI, spot, and AC barriers via Gaussian bumps in the node density. Higher β = tighter clustering.
2. **Piecewise variable density**: explicit 5-region allocation (below KI, KI zone, middle, AC zone, above AC) with variable node counts per region.
3. **Lloyd quantisation**: 1D k-means on the NIG marginal density at the midpoint of the note (obs 4). Nodes converge to cell centroids weighted by density.
4. **Barrier-aware Lloyd**: Lloyd with barrier nodes pinned to KI, spot, and AC, then free nodes re-optimised.
5. **Time-inhomogeneous 2-grid**: "early" grid (obs 1–4, broader tail coverage) and "late" grid (obs 5–8, tighter around barriers), with cross-grid transition matrices and interpolation.

All compared against N=200 uniform Zhang & Li reference. N=50 uniform included for context.

### Setup

- σ ∈ {50, 70, 80, 90, 100, 117, 130, 150, 200}%
- NIG shape: α=13.04, β=1.52
- Contract: 8 bi-daily observations, KI=70%, coupon=100%, AC=102.5%
- Domain: mean ± 8σ of the NIG 2-day transition
- Transition matrix: full dense (NOT Toeplitz), P[i,j] = NIG CDF difference across cell j shifted by rep i
- Same backward recursion and two-pass fair-coupon solve as the baseline CTMC pricer

### Head-to-head at N=15 (error in bps vs N=200 reference)

| Approach | σ=50 | σ=70 | σ=80 | σ=90 | σ=100 | σ=117 | σ=130 | σ=150 | σ=200 | MaxErr | MedErr |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **Uniform N=50** | **+0.0** | **−3.4** | **−6.1** | **−9.4** | **−12.8** | **−20.2** | **−24.5** | **−30.0** | **−39.1** | **39.1** | **12.8** |
| Uniform ZhLi N=15 | +0.0 | −11.1 | −20.7 | −32.7 | −45.1 | −60.4 | −92.5 | −115.5 | −147.9 | 147.9 | 45.1 |
| Tanh β=2 | +0.9 | −5.9 | −13.1 | −18.3 | −43.4 | −67.5 | −89.1 | −181.5 | −250.7 | 250.7 | 43.4 |
| **Tanh β=4** | **+1.0** | **−3.7** | **−9.7** | **−14.7** | **−33.4** | **−62.1** | **−84.2** | **−72.4** | **−127.7** | **127.7** | **33.4** |
| Tanh β=8 | +0.8 | −2.2 | −7.1 | −16.0 | −29.9 | −39.7 | −45.2 | −76.2 | −181.9 | 181.9 | 29.9 |
| Piecewise | +14.6 | −9.2 | −19.3 | −55.9 | −85.9 | −95.7 | −131.3 | −188.9 | −277.0 | 277.0 | 85.9 |
| Lloyd | +0.0 | −29.4 | −62.0 | −112.2 | +26.2 | −60.4 | +163.0 | +88.4 | +272.5 | 272.5 | 62.0 |
| Barrier Lloyd | +1.2 | −20.4 | −41.2 | −69.6 | −103.0 | −157.9 | −79.6 | −271.4 | −432.6 | 432.6 | 79.6 |
| Time-inhomog 2-grid | +12.7 | +65.7 | +136.0 | +145.0 | +228.4 | +629.2 | +821.9 | +1071.8 | +1403.8 | 1403.8 | 228.4 |

Best non-uniform at N=15: Tanh β=4, max error 127.7 bps. Uniform N=50 has max error 39.1 bps. **3.3× gap.**

### Grid size scaling — does N=20 close the gap?

At N=20, best non-uniform (Tanh β=4) has max error 134.8 bps — barely better than N=15 and still 3.5× worse than uniform N=50. The non-uniform advantage does not scale.

### Monotonicity (σ sweep 40%–250% in 1% steps)

| Approach | N | Violations | Worst drop |
|---|---|---|---|
| Uniform ZhLi | 50 | 0 | 0 bps |
| Uniform ZhLi | 15 | 1 | 8.0 bps |
| Tanh β=4 | 15 | 2 | 33.9 bps |
| Barrier Lloyd | 15 | 3 | 229.5 bps |
| Piecewise | 15 | 35 | 501.9 bps |

Piecewise is catastrophic — the grid geometry changes dramatically with σ (via the ε = 1.5·σ_NIG zone widths), causing the barrier-zone boundaries to jump discontinuously.

Lloyd and Barrier Lloyd have wild sign-alternating errors across σ because the node positions themselves are σ-dependent: the density changes shape, nodes shift, and the transition matrix changes structure non-smoothly.

### Grid node positions at σ=117% (N=15)

Notable spacing ratios:
- Uniform ZhLi: min gap 0.014, max gap 0.493, ratio 35.9×
- Tanh β=4: min gap 0.010, max gap 0.139, ratio 13.5×
- Lloyd: min gap 0.048, max gap 0.143, ratio 3.0×
- Barrier Lloyd: min gap 0.025, max gap 0.145, ratio 5.9×

Lloyd produces the most uniform spacing (ratio 3.0×) because it optimises for density coverage, but this is exactly wrong: it puts too many nodes in the NIG density peak and too few near the barriers where the payoff has structure.

### Root causes

1. **Barrier gap dominates.** The autocall–coupon gap (ln 1.025 − ln 1.00 = 0.025) is the hardest feature to resolve. At N=15 on a domain of width ~1.5, any grid has dx ≈ 0.10 — the gap is 4× smaller than the average spacing. Non-uniform grids can cluster nodes near the gap, but only by starving the KI region or the mid-region where the value function has meaningful curvature.

2. **Value function curvature, not density mass.** The right thing to adapt to is the second derivative of the value function, not the transition density. The value function has curvature across the entire [KI, AC] interval (not just at barriers), driven by the conditional expectation of future coupon payments. Clustering near barriers starves the middle.

3. **σ-dependent grids break monotonicity.** Any grid whose node positions depend on σ (Lloyd, piecewise with σ-scaled zones) introduces non-smooth dependence of the fair coupon on σ, because small changes in σ can cause nodes to jump across barrier boundaries or change the effective cell structure.

4. **Time-inhomogeneous interpolation is destructive.** Cross-grid transition matrices (non-square, mapping from one grid to another) introduce interpolation error that compounds through the 8-step recursion. The 2-grid approach has 1400 bps max error — worse than any single-grid method.

### Verdict: DEAD.

Non-uniform grids at N=15 cannot match uniform N=50. The best variant (Tanh β=4) has 128 bps max error vs 39 bps for uniform N=50 — a 3.3× gap that does not close at N=20. Monotonicity also degrades. The fundamental issue is that the autocall value function has distributed curvature across the full [KI, AC] interval, not localised structure at the barriers.

**Ship the SVD version** (§6 → §8: N=50, r=25, split-transaction, sub-bps accuracy).

---

## 8. Split-Transaction SVD: One Leg Per Tx

**Files:** `programs/autocall-bench/src/lib.rs` (bench), `crates/halcyon-quote/src/autocall_v2.rs` (production `solve_fair_coupon_lowrank`), `crates/halcyon-quote/tests/svd_*.rs` (test suite)
**Date:** 2026-04-11
**Idea:** The §6 analysis assumed a single transaction for both legs. The adversarial test suite (§8 tests, Part D3) discovered that the Zhang & Li non-uniform transition matrix needs rank ~25 — not rank 17 as the uniform Toeplitz analysis showed. Rank 25 on both legs in one transaction costs ~1.88M CU (over the 1.4M budget). Solution: split into two transactions, one leg per tx.

### The uniform vs non-uniform rank gap

The Python SVD analysis (§6) was done on a **uniform** Toeplitz matrix. The production CTMC uses the **Zhang & Li** non-uniform grid (piecewise-uniform with different spacing in A/B/C/D regions). The non-uniform matrix is NOT Toeplitz and has higher effective rank:

| Grid type | r for <1 bps (σ=80%) | r for <1 bps (σ=117%) |
|-----------|:---------------------:|:----------------------:|
| Uniform Toeplitz (N=200) | 17 | 12 |
| Zhang & Li (N=50) | 25 | 20 |

The uniform-grid pricer on [ln 0.70, ln 1.025] gives completely wrong fair coupons (3700 bps vs 96 bps at σ=80%) because the grid doesn't extend to the distribution tails, so autocall probability is underestimated. The Zhang & Li grid is the correct pricer.

### Split-transaction architecture

```
Tx 1 (V leg): backward_pass(coupon=0, r=25) → V₀. Write V₀ to buffer PDA.
Tx 2 (U leg): backward_pass(coupon=1, r=25) → V₁. Read V₀ from buffer. fc = (SCALE - V₀) / (V₁ - V₀).
```

### On-chain CU measurements (solana-test-validator 2.3.0)

| Config | Inner CU | Status |
|--------|---:|--------|
| V-leg N=50 r=25 | 880,412 | OK (63%) |
| U-leg N=50 r=25 | 880,418 | OK (63%) |
| Both legs single tx N=50 r=25 | >1.4M | FAIL |

Each leg uses **880K CU (63% of budget)**, leaving 520K CU for PDA reads/writes, Anchor overhead, and settlement logic. Deterministic: both legs cost the same CU to within 6 units.

### Test suite validation

16 passing tests across 3 test files, validating with `nalgebra` f64 SVD:

| Gate | Result | Notes |
|------|--------|-------|
| E2: SVD vs dense N=50 | PASS | <1 bps at r=25 |
| E3: σ-monotonicity 80–250% | PASS | Zero violations |
| E4: Rank sufficiency ||P-P_r||/||P|| | PASS | <0.2% at σ≥80% |
| E5: Leg decomposition V₀+fc*U₀≈1 | PASS | Error <2e-6 |
| B1: Extreme vol (1–1000%) | PASS | No panics, no negatives |
| B5: Malicious SVD factors | PASS | 6/6 robustness tests |
| B7: σ-monotonicity fine sweep | PASS | 0 violations above 80% |
| D3: SVD vs dense N=50 agreement | PASS | <2 bps for all combos |
| D4: Leg decomposition linearity | PASS | Max rel error 2e-6 |

### Production recommendation (updated)

**Two transactions, N=50, r=25 on Zhang & Li grid: 880K CU per leg.**

- Sub-bps accuracy on the production Zhang & Li grid at all σ ≥ 80%
- 2.5× grid resolution vs Richardson(10,15)
- SVD factors computed offline (50×25 U' + 25×50 V^T = 2,500 i64 entries = 20KB per leg)
- V₀ passed between transactions via a buffer PDA (8 bytes)
- No asymmetric rank needed: both legs use the same rank on the non-uniform grid

### What §6 got right, and what it missed

**Right:** the wrapping_mul arithmetic (3.2× CU saving), the two-pass leg decomposition, the in-place buffer trick, the absorbing-state pruning. These all survived.

**Missed:** the SVD rank analysis was done on the wrong matrix (uniform Toeplitz vs Zhang & Li). The test suite (D3: SVD vs dense agreement) caught this immediately — at r=17 the SVD gave 600 bps error instead of <1 bps. The fix was rank 25, which required the split-tx architecture.

---

## 9. Operator Structure Exploration — POD, Rational Approximation, Wavelet Hybrid

**Files:** `research/pod_value_manifold.py`, `research/rational_approximation.py`, `research/svd_wavelet_hybrid.py`
**Date:** 2026-04-11
**Idea:** Three programmes investigating whether a structurally better operator representation exists than SVD rank-25.

1. Proper Orthogonal Decomposition (POD) of the value-function manifold across the full parameter space — does a fixed global basis replace per-σ SVD updates?
2. AAA rational approximation of the Toeplitz symbol — does a state-space recursive filter beat SVD?
3. Low-rank SVD + wavelet residual hybrid — is the SVD tail wavelet-compressible?

### Programme 1: Value-Manifold POD

**Parameter sweep:** 13 σ values (50–250%) × 5 KI barriers (0.50–0.90) × 4 AC barriers (1.01–1.10) × 3 observation counts (4, 8, 12) = 780 parameter combinations. Snapshots collected at each observation date of the backward pass. Common uniform grid N=200 on [−2.5, 1.5].

**Global POD rank (modes for variance capture):**

|               | 99% | 99.9% | 99.99% | 99.999% |
|---------------|----:|------:|-------:|--------:|
| V-leg (raw)   |   1 |     3 |      6 |      13 |
| U-leg (raw)   |   3 |     6 |      9 |      13 |
| V-leg (aligned)|  1 |     3 |      5 |       8 |
| U-leg (aligned)|  3 |     6 |     10 |      15 |

The global U-leg rank of 9 (at 99.99%) across all 780 parameter combinations is less than the per-σ SVD rank of 25 on the Zhang & Li grid. But this result is on the wrong matrix — a common uniform grid, not the production Zhang & Li grid.

**Barrier alignment:** Piecewise-affine coordinate map sending ln(B)→−1, 0→0, ln(D)→+1. Helps V-leg (5 vs 6 modes at 99.99%) but **hurts** U-leg (10 vs 9 modes). The coupon digital step at x=0 dominates U-leg rank, and barrier alignment doesn't smooth it — the kink is a feature of the payoff, not the coordinate system.

**Principal angles between local subspaces:** Maximum principal angle between local σ-bases (k=10 modes each) is **90°** for both V and U legs. The V-leg heatmap shows block-diagonal structure (nearby σ values have small angles, distant σ values are orthogonal). U-leg angles are uniformly large. This means the basis modes that matter at σ=50% are nearly orthogonal to those at σ=250%.

**Interpretation:** The value manifold is globally low-dimensional (rank 9 captures 99.99%), but the local subspace orientation rotates dramatically across σ. A single global basis captures most variance but the remaining 0.01% is σ-dependent and structurally important.

**Verdict: NEGATIVE for the 1-tx problem.** The POD was computed on a uniform grid, not the production Zhang & Li grid. Even if the rank result transferred, it would not help — the Zhang & Li non-uniformity (16.7× cell width ratio) is the dominant source of SVD rank inflation, and POD of the value functions doesn't address this.

### Programme 2: Rational Approximation of the Toeplitz Symbol

**Method:** AAA rational approximation (Nakatsukasa, Sete, Trefethen 2018) of the NIG transition kernel's z-transform K(e^{iω}) on the unit circle, at degrees m = 2, 4, ..., 20. Poles extracted via polynomial root-finding. State-space recursive filter implemented for causal (|λ|<1) and anti-causal (|λ|>1) poles.

**AAA approximation error (max relative error across unit circle):**

| σ    | m=6     | m=8     | m=12    | m=16    | m=20    |
|------|---------|---------|---------|---------|---------|
| 80%  | 4.7e-2  | 9.1e-4  | 7.5e-4  | 4.6e-4  | 4.0e-4  |
| 117% | 2.8e-3  | 3.8e-3  | 5.6e-3  | 1.8e-3  | 4.7e-3  |
| 200% | 3.0e-2  | 4.6e-2  | 5.4e-2  | 1.2e-2  | 3.4e-2  |

Convergence is **non-monotonic** — error bounces and never drops below ~10⁻³ across all σ. The minimum degree for < 0.1% error across all volatilities: **greater than 20**.

**State-space filter pricing (error vs dense, σ=117%, N=200):**

| m  | Causal poles | Anti-causal | Fair coupon error |
|----|:-----------:|:-----------:|:-----------------:|
| 6  |      2      |      3      |     −50.4 bps     |
| 8  |      4      |      3      |     −11.8 bps     |
| 10 |      5      |      4      |   +5,722.5 bps    |
| 12 |      5      |      6      |   **BLOWUP (0)**  |
| ≥14|      —      |      —      |   **BLOWUP (0)**  |

The state-space filter works tolerably at m ≤ 8 (12 bps error) but **explodes at m ≥ 10**. Pricing drops to 0 bps or diverges to 32,000+ bps. The poles cluster tightly near |λ| = 1, causing numerical instability in the recursive filter. Pole locations do not vary smoothly across σ — they rearrange discontinuously.

**Root cause:** The NIG characteristic function has **branch points** at β ± iα in the complex plane, not poles. Rational functions approximate branch-cut structure very slowly (algebraic, not geometric convergence). The AAA greedy algorithm also struggles because the symbol is smooth but featureless — there are no isolated poles to latch onto.

**Verdict: DEAD.** Rational filters are fundamentally unsuited to the NIG kernel. The branch-point singularity structure defeats rational approximation, and the state-space filter is numerically unstable at the degrees needed for sub-bps accuracy. SVD, despite higher parameter count (1,700 vs 80 values), wins decisively because its error is monotone and bounded.

### Programme 3: Low-Rank SVD + Wavelet Residual Hybrid

**Method:** At N=50, σ=117%, truncate SVD to rank 8, compute residual R = P − P₈. Apply Haar wavelet transform (db4 unavailable in this run), threshold at ε, count nonzeros. Hybrid matvec: P·f ≈ P₈·f + W⁻¹(R_w · Wf).

**Wavelet coefficient counts (rank-8 SVD residual):**

| ε      | nnz  | nnz/N² | Hybrid params | vs SVD r=17 |
|--------|-----:|-------:|--------------:|:-----------:|
| 1e-3   |    5 |  0.002 |           805 |     WIN     |
| 1e-4   |  630 |  0.252 |         1,430 |     WIN     |
| 1e-5   | 2123 |  0.849 |         2,923 |    lose     |

**Pricing accuracy:**

| Config                      | Fair coupon error |
|-----------------------------|:-----------------:|
| SVD r=8 + Haar ε=1e-3      |   −90.6 bps       |
| SVD r=8 + Haar ε=1e-4      |    +5.6 bps       |
| SVD r=8 + Haar ε=1e-5      |    +0.07 bps      |

At ε=1e-4: 1,430 params (15.9% fewer than SVD r=17's 1,700), but +5.6 bps error. The wavelet coefficient decay is gradual (no sharp cutoff), so there's no regime where the hybrid is both smaller AND more accurate than a higher-rank SVD.

**Verdict: DEAD.** Marginal parameter saving (15.9%) with worse pricing accuracy. Not worth the on-chain complexity of wavelet transforms in fixed-point arithmetic. The SVD residual is not sufficiently wavelet-compressible because the NIG kernel is smooth (well-represented by SVD) with no localised features for wavelets to exploit.

### Programme 4 (follow-up): Density Matrix Rescaling Q = P / w

**Idea:** The Zhang & Li transition matrix has P[i,j] ≈ f_NIG(rep_j − rep_i) × w_j where w_j is cell width. Dividing by column widths gives Q[i,j] = P[i,j] / w_j ≈ f_NIG(rep_j − rep_i), which should be nearly Toeplitz and lower rank. On-chain: y = Q_r · (w ⊙ f), same SVD cost plus N trivial multiplies.

**Result:** Q is **worse** than P for SVD compression across the entire σ range:

| σ    | P err @ r=17 | Q err @ r=17 |
|------|:------------:|:------------:|
| 80%  |  −52.5 bps   | −56.0 bps (saturated) |
| 100% |  −29.7 bps   | −166.4 bps (saturated) |
| 117% |   −4.4 bps   | −115.2 bps  |
| 200% |   −0.1 bps   |   −3.3 bps  |

The column scaling amplifies narrow-cell entries (where w is small) and wrecks the pricing-relevant balance. At low σ the Q-SVD error saturates at the full fair coupon (total loss of pricing signal). The row normalisation that P already has (rows sum to 1) is a better preconditioning for the backward pass than density rescaling.

**Verdict: DEAD.** Column scaling by cell width makes SVD compression worse, not better.

### Programme 5 (follow-up): Uniform-Rank Error Cancellation

**Finding:** Asymmetric SVD ranks (r_v ≠ r_u) cause catastrophic error amplification in the fair coupon ratio fc = (1 − V₀) / (V₁ − V₀). The denominator (V₁ − V₀) = coupon annuity is a small number (~0.03–0.08). When the two passes use different ranks, truncation errors don't cancel in the denominator, amplifying the fair coupon error by 10–100×.

**Evidence (N=50 Zhang & Li, σ=80%):**

| Config     | fc error    |
|------------|:-----------:|
| uniform r=25 | −2.9 bps  |
| r_v=6, r_u=25 | −0.3 bps |
| r_v=6, r_u=17 | −2.7 bps |
| r_v=6, r_u=12 | −90.6 bps |

At r_v=6/r_u=12 the V-leg has rank-6 truncation error ε₀ and the U-leg has rank-12 error ε₁ ≈ ε₀/10. The annuity V₁ − V₀ gets error (ε₁ − ε₀) ≈ −0.9ε₀, which is amplified by division by the small annuity value.

**Implication:** The §6 asymmetric-rank architecture (r_v=6, r_u=17, 906K CU) was validated on the uniform Toeplitz grid where it works. On the Zhang & Li grid, **uniform rank is mandatory** — both legs must see the same truncation error for cancellation. This forces r=25 on both legs, hence the 2-tx split.

### Programme 6 (follow-up): Dense Matvec at Smaller N

**Idea:** Since SVD r=25 ≈ N/2 at N=50, the SVD is barely compressing. Dense P·f costs N² MADDs vs SVD's 2Nr = N² at r=N/2. Drop SVD, use dense matvec, accept smaller N.

**CU model:** CU_both_passes ≈ 2 × (52K + 16 × N² × 22.25). Max N for 1 tx: N ≈ 42.

**Convergence (bps error vs N=200 reference, Zhang & Li grid, dense):**

|  N  |  CU (both) | σ=80% | σ=100% | σ=117% | σ=150% | σ=200% |
|----:|-----------:|------:|-------:|-------:|-------:|-------:|
|  30 |    703K    |  −10.7|  −22.4 |  −36.5 |  −55.3 |  −73.0 |
|  35 |    945K    |   −9.0|  −19.1 |  −26.0 |  −45.8 |  −60.0 |
|  40 |  1,222K    |   −8.0|  −17.1 |  −23.7 |  −40.2 |  −52.3 |
|  42 |  1,343K    |   −7.5|  −15.8 |  −21.7 |  −37.3 |  −48.7 |
|  50 |  1,884K    |   −6.1|  −12.8 |  −20.2 |  −30.0 |  −39.1 |

Convergence is ~O(N⁻¹), not O(N⁻²). Going from N=50 to N=42 adds only 1.5–9 bps additional discretisation error on top of the 6–39 bps already present at N=50. The errors are systematic, negative, and smooth.

**Assessment:** Dense N=42 is viable as a 1-tx fallback (1,343K CU, 57K margin). Accuracy loss vs N=50 SVD r=25 is 1.5–9.6 bps depending on σ. All errors are within the 50bp issuer margin. The main concern is tight CU margin (4% of budget) and the ~O(N⁻¹) convergence meaning further N reduction degrades quickly.

**Verdict: ALIVE as fallback.** Dense N=40–42 is the simplest 1-tx path. No SVD, no keeper-uploaded factors, no rank tuning. The accuracy delta vs N=50 is small relative to the inherent discretisation error at N=50. But it does not solve the fundamental problem — it trades SVD complexity for resolution loss.

### Summary: what this session explored and eliminated

| Approach | Verdict | Blocking issue |
|----------|---------|----------------|
| POD global basis | NEGATIVE | Computed on wrong matrix (uniform, not Zhang & Li) |
| Rational / state-space filter | DEAD | NIG branch points defeat rational approx; filter unstable ≥10 poles |
| SVD + wavelet hybrid | DEAD | Marginal compression, not worth on-chain complexity |
| Density matrix Q=P/w rescaling | DEAD | Worse than P SVD — amplifies narrow-cell entries |
| Asymmetric SVD ranks on Zhang & Li | DEAD | Error cancellation requires uniform rank |
| Dense matvec N≤42 | ALIVE | Viable 1-tx fallback; ~O(N⁻¹) convergence limits resolution |

**The binding constraint is the Zhang & Li grid's 16.7× cell width ratio**, which inflates the transition matrix rank from ~17 (uniform Toeplitz) to ~25 (non-uniform). All approaches tested either work on the wrong matrix (uniform) or cannot overcome the non-uniformity.

**Open directions:**
- Modified grid design with cell width ratio < 3× (preserve barrier rules while limiting non-uniformity) — this directly attacks the binding constraint
- COS-factored matvec: K-term COS expansion gives structured O(NK) matvec without assembling P; may be insensitive to grid non-uniformity
- Chebyshev surrogate on (σ, B, D): bypass the backward pass entirely with a polynomial model of fc(σ)

---

## 10. Toeplitz Bulk + Interface Defect Decomposition

**Files:** `research/bulk_defect_and_manifold.py`
**Date:** 2026-04-11
**Idea:** Decompose the Zhang & Li transition matrix P into a Toeplitz bulk T (extracted from the uniform B-region interior) plus a sparse/low-rank defect E = P − T. If E is localised to the barrier rows, the on-chain matvec becomes T_SVD·f + E_sparse·f, combining the SVD compression of the uniform interior with a cheap correction for the non-uniform boundaries.

### Results

The decomposition fails decisively. E is not a small correction — it contains 60–88% of P's Frobenius norm:

| σ | ||E||/||P|| | Defect rows (>1% energy) | E rank (99.9%) | Sparse-row fc error |
|---|---|---|---|---|
| 80% | 59.8% | 10 | 10 | −3.4 bps |
| 100% | 67.6% | 16 | 12 | +3.4 bps |
| 117% | 72.5% | 16 | 17 | +49.5 bps |
| 150% | 79.7% | 18 | 32 | +130 bps |
| 200% | 88.2% | 22 | 42 | +279 bps |

### Root cause

The Zhang & Li grid has four regions with different spacings (A: below KI, B: KI-to-coupon, C: coupon-to-autocall, D: above autocall). The Toeplitz template extracted from the B-region interior does not represent rows in A, C, or D. Even within the B-region, the KI snap and Rule 2 adjustments perturb the spacing by up to 143% at σ=200%. The defect E is not sparse (74% of entries above 1e-4), not low-rank (rank 17–42 at 99.9%), and grows with σ.

### Verdict: DEAD.

The Toeplitz decomposition is architecturally incompatible with piecewise-uniform grids. The non-uniformity that the Zhang & Li design deliberately introduces (to satisfy barrier placement rules) is exactly what makes the matrix non-Toeplitz. There is no small correction — the correction IS most of the matrix.

---

## 11. Value Manifold Reduction (POD + DEIM)

**Files:** `research/bulk_defect_and_manifold.py`, `research/manifold_projected_sweep.py`, `research/deim_monotonicity_sweep.py`, `programs/autocall-bench/src/lib.rs`
**Date:** 2026-04-11
**Idea:** The SVD compresses the operator P. But the on-chain pricer doesn't need P·f for arbitrary f — it only needs P applied to the structured value vectors that arise in the backward recursion. If these live on a low-dimensional manifold, precompute a reduced basis and do the entire backward pass in reduced space.

### Step 1: POD dimension discovery

Built snapshot matrices from 50 σ values in [50%, 250%], collecting value vectors at all 9 time steps of both V and U legs. SVD of the snapshot matrices:

| Leg | 99.99% energy | 99.9999% energy | vs operator rank |
|---|---|---|---|
| V (principal) | d = 4 | d = 10 | r = 25 |
| U (coupon annuity) | d = 6 | d = 12 | r = 25 |

The value manifold is dramatically lower-dimensional than the operator. The operator has rank ~25 but the reachable value functions span only 10–12 dimensions.

### Step 2: Three approaches to reduced-space payoff application

The challenge: payoff conditions (KI, coupon, autocall) are pointwise operations in the full N-dimensional space. In reduced space, three approaches:

**A. Reconstruct-Modify-Project (RMP):** At each observation, reconstruct to full space (Φ×c, cost Nd), apply payoff, project back (Φᵀ×v, cost Nd). Total per obs: 2Nd. Accurate but the N×d cost dominates — at d=12, N=50: 2×50×12×8×2×44.5 CU ≈ 2.2M CU. Over budget.

**B. Projected payoff masks:** Precompute d×d projected masks M_red = ΦᵀMΦ for each barrier condition. All operations stay in d-space at d² cost. Per obs: ~10d². At d=12: 10×144×8×2×44.5 ≈ 1.13M CU. Fits at d=12 but not d=15.

**C. DEIM (Discrete Empirical Interpolation):** Select d interpolation points in the full space via greedy pivoting. At each observation: propagate in reduced space (d²), evaluate at d physical points (d²), apply payoff at d scalars, reconstruct via precomputed P_T_inv (d²). Total: 3d² per obs. At d=15: 3×225×8×2×44.5 ≈ 585K CU estimated.

### Step 3: DEIM accuracy sweep

| d | Max |error| (bps) | CU estimate | Fits 1.4M? |
|---|---|---|---|
| 10 | 58.1 | 318K | YES |
| 12 | 8.2 | 412K | YES |
| **15** | **0.66** | **585K** | **YES** |
| 20 | 0.10 | 2,952K | NO |

d=15 DEIM achieves 0.66 bps max error across the full vol range including interpolation to σ values not in the training set. The basis is approximately universal.

### Step 4: On-chain CU measurement

Deployed DEIM backward pass to solana-test-validator. Heap-allocated d×d matrices, `wrapping_mul >> 20` arithmetic matching the existing SVD benchmark conventions.

| Instruction | Inner CU | Total CU | % Budget |
|---|---|---|---|
| DEIM d=12 | 558,165 | 618,014 | 44% |
| **DEIM d=15** | **855,708** | **946,421** | **68%** |
| DEIM d=18 | 1,213,892 | 1,341,573 | 96% |
| SVD N=50 r=25 (one leg) | 880,412 | 916,712 | 65% |

**Estimate-to-measured ratio: 585K estimated → 856K measured = 1.46×.** Consistent with prior BPF inflation factors (1.5–3×). d=15 fits one transaction with 454K CU headroom.

Key comparison: DEIM d=15 (856K, both legs, sub-bps accuracy) vs SVD r=25 (880K, one leg only, requires 2 transactions). DEIM collapses the 2-tx SVD architecture into a single transaction at better accuracy.

### Step 5: Monotonicity validation

421 σ values from 40% to 250% in 0.5% steps:

| Method | Violations | Max violation | Zone |
|---|---|---|---|
| Dense N=50 | 6 | 0.000 bps | 44–57% (near-zero fc) |
| DEIM d=12 | 17 | 0.012 bps | 40–61% |
| DEIM d=15 | 16 | 0.005 bps | 52–60% |
| DEIM d=18 | 40 | 0.005 bps | 40–60% |

All violations are sub-0.01 bps, exclusively in the near-zero coupon zone (σ < 60%) where the product would not issue (gate requires fc ≥ 50 bps). d=18 paradoxically has more violations than d=15 — this is numerical noise at the floating-point floor, not a structural defect.

### On-chain data requirements

| Data | Size | Lifetime |
|---|---|---|
| Φ (50×15 basis) | 750 i64 = 6 KB | Permanent PDA |
| DEIM indices | 15 u8 | Permanent PDA |
| Φ_idx (15×15 basis at points) | 225 i64 = 1.8 KB | Permanent PDA |
| P_T_inv (15×15 reconstruction) | 225 i64 = 1.8 KB | Permanent PDA |
| P_red(σ) (15×15 reduced operator) | 225 i64 = 1.8 KB | Keeper-updated per vol |

Total permanent: ~11.4 KB. Per-quote variable: 1.8 KB.

### Architecture

```
Off-chain (keeper, per vol update):
  1. Build N=50 Zhang & Li P(σ)
  2. P_red = Φᵀ P(σ) Φ  (15×15 matrix, 225 entries)
  3. Write P_red to vol-update PDA

On-chain (single transaction, 946K CU total):
  1. Load Φ, Φ_idx, P_T_inv, DEIM indices from permanent PDA
  2. Load P_red(σ) from vol-update PDA
  3. V-leg: 8 obs × DEIM backward step (3 × 15² madds + payoff)
  4. U-leg: same
  5. fc = (SCALE − V₀) / U₀
  6. Apply margin, gate issuance, emit result
```

### What this solves

1. **Single transaction.** The SVD r=25 approach needed 2 transactions (880K CU per leg). DEIM d=15 does both legs in one tx at 946K.
2. **Sub-bps accuracy.** Max error 0.66 bps vs dense N=50 (which itself has ~20 bps grid error vs Wiener-Hopf). The DEIM truncation error is negligible relative to grid discretisation.
3. **Universal basis.** Same Φ works across σ=50–250% including interpolation. No per-σ basis update.
4. **Minimal keeper load.** Only 225 i64 entries updated per vol change, vs 2,500 entries for SVD factors.
5. **Clean monotonicity.** Zero violations above the issuance gate.

### Step 6: Basis universality test (D5)

The POD basis was built from snapshots at default product parameters (D=1.025, B=0.70, n_obs=8). Does it generalise to non-default (D, B, n_obs)?

**Sweep:** 5 D values × 5 B values × 5 n_obs values × 5 σ values = 625 vectors (reduced grid), plus a full 2,520-vector sweep confirming.

| D range | Mean error | Max error | Pass (<5 bps) |
|---|---|---|---|
| 1.01–1.05 | 1.2–1.9 bps | 5.6–6.4 bps | ~95% |
| 1.10 | 4.3 bps | 9.1 bps | ~70% |
| 1.15 | 6.5 bps | 13.3 bps | ~45% |

| B range | Mean error | Max error |
|---|---|---|
| 0.50–0.60 | 2.0–2.1 bps | 6.5–6.7 bps |
| 0.70–0.80 | 2.6–2.9 bps | 9.6–11.0 bps |
| 0.90–0.95 | 4.9–varies bps | 13.3–747 bps |

**Root cause:** The projection residuals are small (< 1.4%), meaning the subspace captures value functions well. The failure is in the DEIM interpolation: the 15 selected grid points and their KI/coupon/autocall mask assignments are locked to the default barrier positions. When barriers move, the DEIM points fall in wrong payoff regions.

**n_obs scaling:** CU scales linearly with n_obs. At n_obs=8: 946K CU. Maximum n_obs fitting one transaction: ~12 (946K × 12/8 ≈ 1,420K). n_obs > 12 requires split-tx.

**Verdict: per-product DEIM infrastructure required for multi-product.** For the Colosseum demo (D=1.025, B=0.70, n_obs=8), the basis works perfectly. For production with variable products, the keeper rebuilds Φ, DEIM indices, Φ_idx, and P_T_inv per (D, B, n_obs) configuration and stores them in the product PDA (~12 KB per product). P_red(σ) remains the only per-quote update (225 i64 = 1.8 KB).

### Verdict: ALIVE — shipping architecture.

This supersedes §8 (split-transaction SVD r=25) as the production recommendation. Single transaction, sub-bps accuracy, 32% CU headroom, monotone, per-product basis with minimal on-chain storage.

**Constraints:**
- n_obs ≤ 12 in one transaction (CU scales linearly with n_obs)
- Basis must be rebuilt when product parameters (D, B, n_obs) change
- Basis is universal across σ for fixed product parameters

### Step 7: End-to-end CU with PDA deserialization

Full on-chain measurement including simulated PDA deserialization of Φ (750 i64), Φ_idx (225 i64), P_T_inv (225 i64), P_red (225 i64), and terminal payoff projection (Φᵀ · v_full):

| Config | Inner CU | Total CU | % Budget |
|---|---|---|---|
| DEIM d=15, n_obs=4 | 416,585 | 506,857 | 36% |
| DEIM d=15, n_obs=8 | 855,763 | 946,482 | 68% |
| DEIM d=15, n_obs=8 + deser | 962,540 | 964,441 | 69% |
| DEIM d=15, n_obs=12 | 1,294,931 | 1,385,216 | 99% |
| DEIM d=15, n_obs=16 | — | — | EXCEEDS |

CU scales at ~110K per observation. PDA deserialization adds ~18K CU (2%). CU is deterministic and σ-invariant. Maximum n_obs in one transaction: **12**.

For the SOL Autocall (n_obs=8): **964K total CU including deserialization, 31% headroom.**

### Test suite results

Full adversarial test suite (15 Rust tests + on-chain CU):

| Test | Status |
|---|---|
| A: Production sweep (20 σ values) | PASS — max 2.52 bps above 80 bps gate |
| B1: Extreme vol (σ=10-500%) | PASS — no panics, no negatives |
| B4: Barrier boundary stress | PASS — max 4.17 bps |
| B6: No-panic sweep | PASS |
| B7: Fine monotonicity (191 σ, 1% steps) | PASS — 0 violations |
| B7: Production range (101 σ, 0.5% steps) | PASS — 0 violations |
| D3: DEIM vs dense agreement | PASS — 95% < 5 bps above gate |
| D4: Leg decomposition linearity | PASS — max |V(fc)-1| = 7.4e-7 |
| D5: Basis universality (near-default) | PASS — 90.4% < 10 bps |
| E2: Accuracy gate (10 σ) | PASS — all < 5 bps |
| E3: Monotonicity gate (38 pairs) | PASS — 0 violations |
| E7: U₀ division guard | PASS — min |U₀| = 0.816 |
| C: n_obs CU scaling | PASS — linear, max n_obs=12 |
| C: CU determinism | PASS — identical values across runs |
| C: Deserialization overhead | PASS — 18K CU (2%) |

---

## Quick reference — viable candidates still to test

- **Modified grid design**: Cell width ratio < 3× with Zhang & Li barrier rules. Directly attacks the rank inflation source.
- **COS-factored matvec**: K-term COS expansion gives structured O(NK) matvec without assembling P.
- **Chebyshev surrogate on (σ, B, D)**: offline Chebyshev grid on transformed parameters, on-chain Clenshaw evaluation of V(θ) and U(θ) directly.

## Practical note

The split-transaction SVD (§8) is the current shipping candidate: two transactions at 880K CU each, N=50, r=25 on the Zhang & Li grid, sub-bps accuracy. Dense N=42 at 1,343K CU is the 1-tx fallback with ~22–49 bps discretisation error. Richardson(10,15) at ~809K CU remains the cheapest single-tx option with ~4% error.

---

## 12. Next-generation ROM research pass on the correct Zhang-Li operator (2026-04)

This section records the full follow-up programme after the wrong-matrix
Toeplitz episode. The hard rule for every experiment here was:

- compress only the *correct* Zhang-Li barrier-aware pricing path
- measure fair-coupon error directly
- validate on TRAIN / VALIDATION / STRESS rather than sigma slices only
- refuse “99.99% energy” and similar proxy wins

Reproducible outputs:

- `research/state_of_repo_autocall_rom.md`
- `research/next_gen_rom_report.md`
- `research/next_gen_rom_decision_memo.md`
- `research/results/next_gen_rom_*.csv`
- `research/plots/next_gen_rom_*.png`

### 12.1 Repo state and correction to §11

Current facts confirmed from code/tests:

- The honest production baseline is still direct low-rank SVD on the
  *correct* Zhang-Li operator.
- The fair coupon must be handled through
  `q*(theta) = (1 - V0(theta)) / U0(theta)`.
- For the default fixed Structure II product
  (`B = 0.70`, `D = 1.025`, `n_obs = 8`), the earlier per-product
  POD+DEIM result in §11 still stands: measured single-transaction BPF
  execution at `946,421` total CU for `d=15`, with sub-bp error vs
  dense `N=50` and only sub-`0.01 bp` monotonicity noise in the
  near-zero coupon zone.
- What does **not** survive is the stronger universal claim. Once the
  barrier/schedule box is widened, the same DEIM idea is not a robust
  general architecture unless the basis/interpolation machinery is
  rebuilt per product cell.

So §11 remains the fixed-product shipping note. Section 12 is about the
broader generalization problem and the next architecture beyond that
single default product.

### 12.2 Honest direct-SVD baseline recomputed

Recomputed baseline on the current code:

- grid `N = 50`
- ranks `r_V = 25`, `r_U = 25`
- `cargo test -p halcyon-quote --test svd_rank_sufficiency --release -- --nocapture d3_svd_vs_dense_n50_agreement`
  passed with `60` vectors tested and `0` failures at the `2 bp` gate
- earlier regression gate still holds with peak sigma-slice SVD-vs-dense
  error about `0.42 bp` and zero monotonicity violations on the tested
  `60%` to `250%` sigma range

Localnet CU:

- `bench_split_v_leg_n50_r25`: inner `880,412`, total `916,712`
- `bench_split_u_leg_n50_r25`: inner `880,418`, total `916,890`

Keeper payload:

- about `20 KB` per leg
- about `40 KB` total

Dense-grid cross-check:

- dense `N=50` vs dense `N=200`
  - validation worst / median: `98.69 / 14.25 bp`
  - stress worst / median: `144.24 / 8.91 bp`

That means the current direct-SVD truncation is already smaller than the
underlying grid error at the working point. This is still the honest
universal baseline to beat.

**Verdict: ALIVE**

### 12.3 A branch: value-manifold reduction

We built snapshot sets for
`U_j^-`, `U_j^+`, `V_j^-`, `V_j^+` and compared:

- one global basis
- per-leg basis
- per-leg + per-date basis
- per-leg + per-date + pre/post basis

The key result is that the value manifold is indeed lower-dimensional
than the operator, but raw POD is not enough over the full box.

Best raw stage-split point:

- `A2_per_leg_date_phase_d10`
- validation worst `262.27 bp`
- validation median `110.35 bp`
- stress worst `135.73 bp`

Interpretation:

- stage splitting is directionally correct
- but the basis is still spending modes on moving barrier-induced kinks

**Verdict: DEAD in raw form; ALIVE as structural direction**

### 12.4 B branch: registration and kink enrichment

We then registered snapshots with a piecewise-affine map aligning:

- `log(KI)`
- `0` (coupon barrier)
- `log(AC)`

and added explicit barrier/kink modes, including local modes in the
thin `[0, log(1.025)]` gap.

Best point:

- registered + enriched, `d = 20`
- validation worst `52.40 bp`
- validation median `14.51 bp`
- stress worst `36.68 bp`

This was the first branch that materially reduced fair-coupon error at
fixed reduced dimension.

Important falsification:

- singular-value energy remained a bad pricing proxy
- for representative group `('U', 1, 'minus')`, raw POD needed `4`
  modes for `99.99%` energy and registered POD needed `5`, yet the
  registered basis priced much better

So the win is geometric, not “more energy”.

**Verdict: ALIVE**

### 12.5 C branch: goal-oriented basis selection

We reweighted basis construction around coupon sensitivity using

`dq = -(1/U0) dV0 - ((1 - V0)/U0^2) dU0`

and compared weighted POD / greedy fair-coupon mode selection against
 energy-optimal truncation.

Best linear-ROM point:

- registered + enriched + goal-greedy
- `d = 24`
- validation worst `49.74 bp`
- validation median `14.93 bp`
- stress worst `32.84 bp`
- zero sigma-monotonicity violations on the evaluation sweep

This is the best reduced-basis branch found in the whole programme. It
did beat raw POD and confirmed that coupon-optimal selection matters,
but it still did not come close to the sub-bp direct-SVD baseline.

**Verdict: ALIVE**

### 12.6 D branch: hyper-reduction of the observation / KI map

We tested whether the remaining error was mostly concentrated in the
observation step and could be repaired by DEIM/EIM-style correction.

Compared:

- pure projected ROM
- sampled residual correction near KI / coupon / AC

Starting from the best C-branch basis:

- pure ROM: validation worst `49.74 bp`, stress worst `32.84 bp`
- best sampled correction: validation worst `50.65 bp`,
  stress worst `48.92 bp`

Why this died:

- in the current two-layer formulation, the observation step is
  affine-linear rather than a tiny isolated nonlinearity
- the missing error is a distributed out-of-subspace residual, not a
  small local defect that a few DEIM samples can cheaply repair

This does **not** invalidate the fixed-product POD+DEIM result in §11.
It kills a different claim: on the full box, the observation residual
is not the right cheap object to hyper-reduce inside the current
registered split-ROM family.

**Verdict: DEAD**

### 12.7 E branch: local atlas / parametric ROM

Principal-angle diagnostics showed real subspace drift. Representative
registered group:

- low vs mid: max `57.33 deg`, mean `7.93 deg`
- low vs high: max `79.67 deg`, mean `14.88 deg`
- mid vs high: max `76.17 deg`, mean `10.16 deg`

So one global basis is not enough.

But a simple sigma-only local atlas did not solve it:

- best sigma-atlas point `d = 12`
- validation worst `72.90 bp`
- stress worst `190.06 bp`

Interpretation:

- the atlas likely has to be geometry-aware, not just sigma-aware
- barrier placement / registered geometry matter alongside volatility

**Verdict: INCONCLUSIVE**

### 12.8 F branch: direct surrogates for `U0`, `V0`, `q`

We tested direct scalar surrogates, including cubic-RBF fits.

Best cheap scalar model:

- direct `q` cubic RBF
- validation worst `210.05 bp`
- validation median `9.30 bp`
- stress worst `339.65 bp`

Interpretation:

- medians can look decent
- tails are not robust enough for a final quote path
- still potentially useful as prefilter / warm start only

**Verdict: INCONCLUSIVE as prefilter; DEAD as primary pricer**

### 12.9 G branch: structure-preserving alternatives

We revisited alternatives such as snapshot interpolative decomposition
on the *correct* registered objects.

Best point:

- registered/enriched snapshot ID, `d = 24`
- validation worst `50.02 bp`
- validation median `10.51 bp`
- stress worst `35.90 bp`

Competitive with POD, but no material gain in fair-coupon error, CU, or
storage. No genuinely new structure emerged.

**Verdict: DEAD**

### 12.10 H branch: control / system-identification

We repaired the AAA helper and tested rational fitting on reduced
coefficient behaviour over sigma. Even on a fixed-barrier sigma slice,
the denominator sensitivity killed the path:

- `V0` worst absolute error `0.0601`
- `U0` worst absolute error `0.3748`
- resulting `q` worst absolute error `675.98 bp`

**Verdict: DEAD**

### 12.11 I branch: nonlinear manifold

We tested a lightweight kernel-PCA surrogate as the first nonlinear
probe. Held-out registered snapshots were much worse than linear POD at
the same latent dimension.

Representative `d = 10` results:

- `('U', 1, 'minus')`: linear POD relative error `0.001089`,
  kernel PCA `1.012811`
- `('V', 1, 'minus')`: linear POD relative error `0.000807`,
  kernel PCA `1.008987`

No evidence of a useful mild nonlinear manifold appeared in this first
pass.

**Verdict: DEAD**

### 12.12 Stronger-truth cross-check

Best ROM from branch C checked against dense `N=200`:

- C2 `d=24` vs dense `N=200`
  - validation worst / median: `140.74 / 13.48 bp`
  - stress worst / median: `115.72 / 6.66 bp`

For comparison:

- dense `N=50` vs dense `N=200`
  - validation worst / median: `98.69 / 14.25 bp`
  - stress worst / median: `144.24 / 8.91 bp`

The stress improvement of the ROM is a cancellation effect. Validation
worst-case is still worse than dense `N=50`, so this does not count as
beating the baseline.

### 12.13 On-chain CU and payload reality

We added pure reduced-recursion microbenchmarks and measured the actual
localnet arithmetic path:

- `split_rom_d20`: inner `333,511`, total `342,270`
- `split_rom_d24`: inner `474,847`, total `485,661`
- `deim_d12` reference: inner `558,165`, total `618,014`

So a pure reduced recursion *can* cut CU relative to direct SVD.

But the engineering tradeoff is not favourable yet:

- direct SVD payload: about `40,000` bytes total
- split ROM `d=20`: about `98,880` bytes
- split ROM `d=24`: about `141,696` bytes

Today’s ROM frontier buys CU by spending both accuracy and keeper
payload complexity.

### 12.14 Error geography and fallback flags

The best goal-oriented registered ROM had no sigma-monotonicity
violations on the tested sweep, but its largest spikes clustered where
expected:

- high barriers with the thin coupon/autocall gap, especially
  `B >= 0.85` and `D >= 1.075`
- very high volatility, especially `sigma > 2.0`

Empirical fallback flags:

- fall back to direct SVD for `sigma > 2.0`
- fall back to direct SVD when `B >= 0.85` and `D >= 1.075`

These are operational heuristics, not certification.

### 12.15 Main conclusions

1. The true low-dimensional object is the **registered, barrier-aware
   value manifold**, not the raw operator and not the observation
   residual by itself.
2. The best method found is still **linear**: registered + enriched +
   goal-oriented ROM beat every nonlinear and system-ID branch tested.
3. **One global basis is not enough** over the full box.
4. **Registration matters materially** because the main pathology is
   moving kinks and thin barrier geometry.
5. The observation/KI map is **not** the dominant bottleneck in the
   simple DEIM sense we hoped for the broad-box ROM.
6. For the current fixed Structure II product, **per-product POD+DEIM**
   remains the best measured **single-transaction shipping path**.
7. For broad-box robustness, parameter variation, and fallback,
   **direct operator SVD on the correct Zhang-Li grid** remains the
   honest universal baseline.
8. The best next research direction is a **geometry-aware local atlas**
   built on registered snapshots, barrier enrichment, and fair-coupon
   greedy basis selection, with hard fallback to direct SVD outside the
   trusted cells.

### 12.16 Final branch ranking

- **SHIPPING (fixed product):** per-product POD+DEIM, `d=15`, for the
  default `B=0.70`, `D=1.025`, `n_obs=8` note
- **GOLD universal baseline / fallback:** direct ZL-SVD, `N=50`,
  `r=25` per leg
- **SILVER research direction:** registered + enriched + goal-oriented
  split ROM, likely with a future geometry-aware atlas
- **BRONZE:** direct scalar surrogates as prefilter / warm start only
- **DEAD:** raw POD, DEIM on the current residual, sigma-only atlas,
  snapshot ID as a supposed breakthrough, rational/system-ID, and the
  first nonlinear latent branch

Bottom line:

The serious full-box answer is not that we found a new *universal*
pricer that beats direct SVD today. The existing fixed-product
POD+DEIM path is still the shipping one-transaction implementation for
the default note. The real new discovery is that, once the box is
widened, the registered value manifold is the right low-dimensional
object, and the remaining problem is to localize that manifold without
losing the clean fallback guarantees of the direct-SVD path.

---

## 13. Geometry-aware local atlas follow-up (2026-04)

This was the first concrete continuation of the “registered value
manifold + local atlas” direction from §12.

### 13.1 What was tested

We built a small local atlas on top of the registered/enriched ROM by
partitioning the TRAIN box in normalized raw parameter coordinates
`(sigma, B, D)` using k-means, then fitting one local POD basis per
cell. The best pure atlas found was:

- `raw_kmeans_k5_d24`

We also tested:

- smaller `k=3` atlases
- hybrid atlas + direct-SVD fallback
- a non-deployable local k-NN basis diagnostic to see whether the
  remaining error was mostly an atlas tessellation problem

### 13.2 Main result vs dense N=50

Best pure atlas:

- worst validation `|q|` error: `39.12 bp`
- median validation `|q|` error: `13.66 bp`
- worst stress `|q|` error: `26.92 bp`
- atlas-path localnet CU: `485,661` total
- atlas storage: `708,480` bytes

Compared with the comparable global registered weighted ROM:

- global `d=24`: `44.21 / 11.36 / 52.52 bp`
- global `d=32`: `41.80 / 12.49 / 33.50 bp`

So the local atlas is a real structural win over one global basis at
the same reduced dimension. The biggest gain is stress robustness.

### 13.3 Hybrid atlas + fallback

Using validation to trust only atlas cells with worst validation error
at or below `35 bp` gave trusted clusters `0,1,2,3`, with:

- validation coverage on atlas path: `83.3%`
- stress coverage on atlas path: `92.3%`
- hybrid worst validation error vs dense `N=50`: `34.39 bp`
- hybrid worst stress error vs dense `N=50`: `26.92 bp`

This is a plausible engineering control surface, but not yet a
universal answer, because the fallback path inherits the `N=50` grid
error.

### 13.4 Stronger-truth cross-check

Against dense `N=200`:

- validation:
  - pure atlas: `228.38 / 16.26 bp`
  - hybrid atlas: `177.89 / 15.65 bp`
  - dense `N=50`: `98.69 / 14.25 bp`
- stress:
  - pure atlas: `21.26 / 8.66 bp`
  - hybrid atlas: `38.24 / 8.66 bp`
  - dense `N=50`: `144.24 / 8.91 bp`

Interpretation:

- the pure atlas really does improve the stress set against stronger
  truth
- but validation against stronger truth is still worse than dense
  `N=50`

So the atlas is not yet a safe universal replacement for the direct
SVD baseline.

### 13.5 Local-manifold diagnostic

The non-deployable local k-NN basis diagnostic (`raw_knn_k20_d16`)
landed at:

- worst validation `|q|` error: `40.58 bp`
- worst stress `|q|` error: `23.24 bp`

That only modestly improves on the deployable atlas. This matters:
the remaining error is not just poor atlas tessellation. Even a much
more local linear basis still plateaus in roughly the same `20` to
`40 bp` regime on this box.

### 13.6 Atlas geometry

The learned `k=5` cells were interpretable:

- cluster 0: low/mid sigma, `D=1.10`
- cluster 1: low-KI thin/medium-gap region
- cluster 2: high-KI low/mid-sigma thin/medium-gap region
- cluster 3: high-sigma thin/medium-gap region
- cluster 4: medium/high-sigma `D=1.10` region

The dominant split is between the wide autocall-gap `D=1.10` rows and
the thinner `D in {1.02, 1.025, 1.05}` rows, with further separation
by sigma and KI level. The hardest cell is the medium/high-volatility
`D=1.10` region.

### 13.7 Harness note

While adding the `N=200` cross-check, we found a real harness bug:
`TRANSFER_CACHE` in `next_gen_rom_common.py` had been keyed only by
parameter key, not by source grid size. Mixing `N=50` and `N=200` on
the same `(sigma, B, D)` could therefore reuse the wrong transfer
matrices. The cache key now includes `sol.reps.size`.

### Verdict: ALIVE as research direction, INCONCLUSIVE as universal pricer

Why alive:

- geometry-aware localization is a real improvement over one global
  registered basis
- the local atlas cuts stress error materially at the same reduced
  dimension
- the cells are interpretable and match the registered-manifold story

Why not production-ready:

- validation against stronger truth still blows up in the wrong places
- the hybrid fallback reintroduces the baseline grid error
- even the local k-NN diagnostic does not collapse the branch to
  sub-10-bp behaviour

Current recommendation remains unchanged:

- keep direct ZL-SVD as the honest universal baseline and production
  fallback
- continue the geometry-aware local atlas line as the best next
  research direction

## 15. Topology-First Mixed-Fidelity Atlas (2026-04)


## Topology-first partition

We computed a discrete Zhang-Li topology signature first and only clustered in geometry coordinates within each topology class.

- topology classes observed on TRAIN: `atm44_ki6_n50`

In this harness the topology signature collapses to a single class, so the topology-first step is real but degenerate: it tells us the remaining variation is entirely in local chart geometry.

## Geometry coordinates and local charts

Within the topology class we clustered in the requested geometry coordinates:

- `u_1 = log(L_A / L_D)`
- `u_2 = log(L_B / L_D)`
- `u_3 = log(L_C / L_D)`
- `rho_1 = log(dx_A / dx_B)`
- `rho_2 = log(dx_C / dx_B)`
- `kappa = max(s_i) / min(s_i)`

Each chart then got its own medoid-based registration reference grid instead of the global default chart.

## Final chart set

- chart 0: topology `atm44_ki6_n50`, medoid `(sigma=0.800, B=0.500, D=1.020)`, fidelity `N=50`, dim `20`, validation worst vs N200 `19.14 bp`
- chart 1: topology `atm44_ki6_n50`, medoid `(sigma=2.000, B=0.500, D=1.025)`, fidelity `N=200`, dim `16`, validation worst vs N200 `139.69 bp`
- chart 2: topology `atm44_ki6_n50`, medoid `(sigma=1.170, B=0.900, D=1.020)`, fidelity `N=200`, dim `16`, validation worst vs N200 `133.30 bp`
- chart 3: topology `atm44_ki6_n50`, medoid `(sigma=1.500, B=0.700, D=1.050)`, fidelity `N=200`, dim `16`, validation worst vs N200 `245.34 bp`
- chart 4: topology `atm44_ki6_n50`, medoid `(sigma=1.000, B=0.900, D=1.050)`, fidelity `N=200`, dim `16`, validation worst vs N200 `16.98 bp`

The final adaptive dimensions settled at `d in {16, 20}`, so local dimension is not the main bottleneck here. At the `35 bp` trust threshold, the charts that remain usable on the atlas path are `0,4`.

## Actual routing vs oracle routing

Validation worst / median vs `N=200`:

- actual routing: `245.34 / 36.48 bp`
- oracle routing: `133.30 / 19.25 bp`
- routing switch rate: `56.7%`

Stress worst / median vs `N=200`:

- actual routing: `90.13 / 45.64 bp`
- oracle routing: `56.64 / 30.68 bp`
- routing switch rate: `69.2%`

This is the decisive diagnostic. If oracle routing is much better than actual routing, chart assignment is still the main problem. If oracle routing barely helps, the remaining issue is local basis insufficiency or truth mismatch inside the hard charts.

## Stronger-truth fallback

Using chart-level trust based on validation worst vs `N=200` with threshold `35 bp`:

- trusted charts: `0,4`
- validation coverage on atlas path: `13.3%`
- validation worst / median vs `N=200`: `19.14 / 0.00 bp`
- stress coverage on atlas path: `0.0%`
- stress worst / median vs `N=200`: `0.00 / 0.00 bp`

This removes the `N=50` fallback bias by construction. It tells us what the atlas would look like as a universal accelerator sitting in front of stronger truth, not just in front of `N=50`.

## Cell-wise error decomposition

For the `N=50`-trained charts we decomposed:

`atlas - N200 = (atlas - N50) + (N50 - N200)`

Validation sign alignment summary:

- chart 0: sign-match rate `100.0%` over `3` validation points

If the terms align in sign, the local atlas is amplifying the grid bias in that chart. If they anticorrelate, the atlas is already correcting some of the grid bias.

## Verdict

This branch directly tests the hypothesis that the next architecture is not a new compression family, but a topology-aware, geometry-aware, chart-specific, mixed-fidelity atlas.

The result should be read in three layers:

1. topology-first partitioning was conceptually right, but the simplified harness exposes only one topology class;
2. chart-specific registration maps and adaptive dimensions are low-friction wins and should stay;
3. the real discriminator is routing: oracle routing cuts the validation worst-case from `245.34` to `133.30 bp`, but that still leaves a large irreducible local-chart error floor.

## 16. Truth-Aware Router + Trust Fallback (2026-04)


## Setup

We kept the mixed-fidelity chart family from the topology atlas and changed only the router/trust layer.

- charts are unchanged from the `E6` run
- supervision target is per-chart fair-coupon error against dense `N=200`
- router candidates are:
  - geometry-nearest baseline
  - kNN classifier on oracle chart labels
  - kNN regression on per-chart errors
- feature sets are:
  - raw `(sigma, B, D)`
  - registered geometry `(u_1, u_2, u_3, rho_1, rho_2, kappa)`
  - combined raw + geometry

Selection policy:

- fit/calibrate on VALIDATION by leave-one-out
- choose the router with the best validation worst-case, then median
- choose the trust rule that maximizes atlas coverage subject to not exceeding the direct-`N=50` fallback worst-case on VALIDATION
- report final out-of-sample numbers on STRESS

## Best router

- family: `knn_error`
- feature set: `geo`
- k: `9`

Validation leave-one-out vs `N=200`:

- learned router: `133.30 / 21.29 bp`
- geometry-nearest baseline: `245.34 / 36.48 bp`
- oracle router: `133.30 / 19.25 bp`
- router/oracle mismatch rate: `25.0%`

Stress vs `N=200`:

- learned router: `56.64 / 30.68 bp`
- geometry-nearest baseline: `90.13 / 45.64 bp`
- oracle router: `56.64 / 30.68 bp`
- router/oracle mismatch rate: `15.4%`

This answers the main routing question directly. If the learned router materially closes the gap to oracle, routing was the dominant bottleneck. If it barely moves relative to geometry-nearest, the charts themselves are the problem.

## Trust / fallback

Fallback proxy:

- direct SVD is sub-bp against dense `N=50`, so for strong-truth routing we use dense `N=50` vs dense `N=200` as the fallback error proxy
- fallback alone lands at `98.69 / 14.25 bp` on VALIDATION and `144.24 / 8.91 bp` on STRESS

Best trust rule selected on VALIDATION:

- score: `pred_err_plus_std`
- threshold: `145.62 bp`
- atlas coverage on VALIDATION: `93.3%`
- hybrid worst / median on VALIDATION: `98.26 / 21.10 bp`

Applied out-of-sample on STRESS:

- atlas coverage on STRESS: `100.0%`
- hybrid worst / median on STRESS: `56.64 / 30.68 bp`
- trusted predicted charts seen on VALIDATION: `0,1,2,3,4`

## Residual spike map

Largest VALIDATION misses after routing:

- `(sigma=2.25, B=0.75, D=1.075)`: router `2`, oracle `2`, `|q|=133.30 bp`, oracle floor `133.30 bp`
- `(sigma=1.75, B=0.75, D=1.075)`: router `2`, oracle `2`, `|q|=117.60 bp`, oracle floor `117.60 bp`
- `(sigma=1.35, B=0.75, D=1.075)`: router `4`, oracle `0`, `|q|=89.79 bp`, oracle floor `64.19 bp`
- `(sigma=2.25, B=0.55, D=1.0375)`: router `3`, oracle `4`, `|q|=66.88 bp`, oracle floor `61.54 bp`

Largest STRESS misses after routing:

- `(sigma=2.0, B=0.7, D=1.0251)`: router `3`, oracle `3`, `|q|=56.64 bp`, oracle floor `56.64 bp`
- `(sigma=2.0, B=0.7, D=1.0249)`: router `3`, oracle `3`, `|q|=56.60 bp`, oracle floor `56.60 bp`
- `(sigma=1.5, B=0.7, D=1.0245)`: router `3`, oracle `4`, `|q|=48.60 bp`, oracle floor `37.39 bp`
- `(sigma=1.5, B=0.7, D=1.0255)`: router `3`, oracle `4`, `|q|=48.57 bp`, oracle floor `39.62 bp`

## Engineering read

- average atlas-path op proxy is still tiny relative to direct SVD; the current chart set averages about `248,414` estimated total CU on the reduced path using the existing split-ROM calibration
- router state is small: validation calibration prototypes plus a few scalar thresholds
- the real engineering question is not cost but whether the trust rule captures enough of the parameter box to matter

## Verdict

`ALIVE` as a production-minded accelerator in front of direct SVD. `INCONCLUSIVE` as a pure-ROM replacement.

Why alive:

- the learned router collapses the geometry-nearest gap almost all the way to oracle
- on STRESS it exactly recovers the oracle worst / median envelope
- the trust rule keeps the validation worst-case below the direct-fallback bound while still routing `93.3%` of VALIDATION and `100%` of STRESS through the atlas path
- pure geometry features beat raw `(sigma, B, D)`, which is direct evidence that the right low-dimensional object is registered chart geometry rather than raw parameters

What remains unresolved:

- even with the right router, the local chart floor is still `56.64 bp` on STRESS and `133.30 bp` on VALIDATION against `N=200`
- the residual misses concentrate in the thin/wide autocall-gap edge around `B≈0.75`, `D≈1.075`, and higher sigma, so the remaining problem is local chart bias, not assignment

Recommended architecture after this branch:

1. keep direct ZL-SVD as the universal fallback;
2. place a truth-calibrated geometry-kNN router in front of the atlas;
3. use the predicted-error guard (`pred_err + std`) to decide atlas vs fallback;
4. spend the next cycle improving the few residual hard cells, not replacing the router.

## 17. Router-Aligned Local Retraining (2026-04)


## Setup

This branch keeps the truth-aware geometry router from `E7` and attacks the remaining local chart floor.

Procedure:

1. build the mixed-fidelity topology atlas from `E6`;
2. evaluate strong-truth per-chart errors on TRAIN and VALIDATION;
3. reassign TRAIN points to charts by oracle chart usage under strong truth;
4. rebuild each chart directly on its oracle-assigned region with `N=200` snapshots and a fresh medoid registration map;
5. rerun the full truth-aware router and trust sweep.

## Rebuilt chart set

- chart 0: medoid `(sigma=1.500, B=0.500, D=1.020)`, dim `20`, train size `16`
- chart 1: medoid `(sigma=1.170, B=0.500, D=1.020)`, dim `16`, train size `15`
- chart 2: medoid `(sigma=1.000, B=0.900, D=1.020)`, dim `16`, train size `14`
- chart 3: medoid `(sigma=1.500, B=0.800, D=1.020)`, dim `16`, train size `29`
- chart 4: medoid `(sigma=2.000, B=0.600, D=1.020)`, dim `24`, train size `26`

## Best router after retraining

- family: `knn_error`
- feature set: `geo`
- k: `3`

Validation vs `N=200`:

- retrained router: `337.59 / 31.94 bp`
- retrained oracle floor: `337.59 / 29.93 bp`
- prior `E7` router: `133.30 / 21.29 bp`

Stress vs `N=200`:

- retrained router: `61.41 / 12.11 bp`
- retrained oracle floor: `61.41 / 10.21 bp`
- prior `E7` router: `56.64 / 30.68 bp`

## Trust / fallback

Best trust rule selected on VALIDATION:

- score: `pred_err`
- threshold: `55.53 bp`
- hybrid validation: `97.15 / 21.74 bp`
- atlas coverage on VALIDATION: `80.0%`

Applied to STRESS:

- hybrid stress: `61.41 / 12.11 bp`
- atlas coverage on STRESS: `100.0%`
- prior `E7` hybrid stress: `56.64 / 30.68 bp`

## Residual validation spikes

- `(sigma=1.75, B=0.85, D=1.075)`: router `2`, oracle `2`, `|q|=337.59 bp`, oracle floor `337.59 bp`
- `(sigma=2.25, B=0.85, D=1.075)`: router `2`, oracle `2`, `|q|=335.47 bp`, oracle floor `335.47 bp`
- `(sigma=1.35, B=0.85, D=1.075)`: router `2`, oracle `2`, `|q|=333.81 bp`, oracle floor `333.81 bp`
- `(sigma=1.1, B=0.85, D=1.075)`: router `2`, oracle `2`, `|q|=311.41 bp`, oracle floor `311.41 bp`
- `(sigma=0.9, B=0.85, D=1.075)`: router `2`, oracle `2`, `|q|=265.52 bp`, oracle floor `265.52 bp`

## Engineering read

- average atlas-path estimated total CU remains about `300,556`
- on-chain complexity still depends on one active chart, so the retraining only changes storage / keeper payload, not the per-quote asymptotic cost

## Verdict

`DEAD` as a next production direction.

What failed:

- oracle-aligned retraining collapsed too much mass into chart `2`
- the rebuilt chart medoids drifted back toward `D≈1.02`
- the validation floor exploded on the unseen `D=1.075`, high-`B` edge

Mathematical reason:

The sparse original TRAIN box does not contain enough local geometry near the hard `D≈1.075` edge. Reassigning the old train points by oracle chart usage changes chart membership, but it does not create the missing local chart geometry. The retrained basis therefore overfits the nearest available `D≈1.02` / `1.05` structures and extrapolates badly across the thin gap.

Takeaway:

The next honest step is hard-cell subdivision or explicit supplemental edge anchors, not EM-style reassign-and-retrain on the same sparse training box.

## 18. Hard-Edge Supplemental Charts (2026-04)


## Setup

This branch keeps the `E7` truth-aware router intact and changes only the chart family by adding two explicit local charts:

1. a production-strip chart near `(B≈0.70, D≈1.025)`;
2. a thin/wide-gap edge chart near `D≈1.075`.

Both charts are trained with `N=200` snapshots on off-validation supplemental anchors, so the branch stays train/validation separated while adding geometry the sparse original train box never contained.

## Expanded chart set

- chart 0: medoid `(sigma=0.800, B=0.500, D=1.020)`, dim `20`, train size `12`
- chart 1: medoid `(sigma=2.000, B=0.500, D=1.025)`, dim `16`, train size `31`
- chart 2: medoid `(sigma=1.170, B=0.900, D=1.020)`, dim `16`, train size `21`
- chart 3: medoid `(sigma=1.500, B=0.700, D=1.050)`, dim `16`, train size `24`
- chart 4: medoid `(sigma=1.000, B=0.900, D=1.050)`, dim `16`, train size `12`
- chart 5: medoid `(sigma=1.500, B=0.700, D=1.025)`, dim `24`, train size `28`
- chart 6: medoid `(sigma=2.000, B=0.700, D=1.100)`, dim `20`, train size `64`

## Best router on expanded atlas

- family: `knn_error`
- feature set: `geo`
- k: `9`

Validation vs `N=200`:

- expanded router: `62.58 / 21.11 bp`
- expanded oracle floor: `62.58 / 16.84 bp`
- prior `E7` router: `133.30 / 21.29 bp`

Stress vs `N=200`:

- expanded router: `30.68 / 9.16 bp`
- expanded oracle floor: `30.68 / 9.16 bp`
- prior `E7` router: `56.64 / 30.68 bp`

## Trust / fallback

Best trust rule selected on VALIDATION:

- score: `pred_err`
- threshold: `68.47 bp`
- hybrid validation: `62.58 / 21.11 bp`
- atlas coverage on VALIDATION: `100.0%`

Applied to STRESS:

- hybrid stress: `30.68 / 9.16 bp`
- atlas coverage on STRESS: `100.0%`
- prior `E7` hybrid stress: `56.64 / 30.68 bp`

## Residual validation spikes

- `(sigma=2.25, B=0.65, D=1.0375)`: router `3`, oracle `3`, `|q|=62.58 bp`, oracle floor `62.58 bp`
- `(sigma=2.25, B=0.55, D=1.0225)`: router `5`, oracle `4`, `|q|=58.16 bp`, oracle floor `27.66 bp`
- `(sigma=2.25, B=0.75, D=1.0225)`: router `3`, oracle `5`, `|q|=53.07 bp`, oracle floor `37.09 bp`
- `(sigma=1.75, B=0.65, D=1.0375)`: router `3`, oracle `3`, `|q|=51.60 bp`, oracle floor `51.60 bp`
- `(sigma=1.75, B=0.75, D=1.0225)`: router `3`, oracle `5`, `|q|=49.59 bp`, oracle floor `35.08 bp`
- `(sigma=1.35, B=0.65, D=1.075)`: router `6`, oracle `4`, `|q|=48.42 bp`, oracle floor `5.76 bp`

## Engineering read

- average atlas-path estimated total CU is about `295,714`
- total reduced payload across the 7 charts is about `594,432` bytes
- only one chart is still active online, so the extra charts mainly affect storage and router metadata

## Verdict

`ALIVE` as the new lead research architecture and the first plausible universal accelerator in front of direct ZL-SVD.

Why alive:

- the added edge charts cut validation worst-case from `133.30` to `62.58 bp`
- they cut stress worst-case from `56.64` to `30.68 bp`
- the best geometry-error router again reaches the oracle floor on both validation and stress
- the trust rule accepts `100%` of validation and stress, so the enlarged atlas now dominates the direct-`N=50` fallback proxy everywhere tested

What still blocks pure replacement:

- `62.58 bp` worst validation error is still far from sub-5-bp universal quoting
- storage rises to about `594 KB`, materially larger than the direct-SVD keeper payload
- this branch still needs a real Rust/Anchor benchmark before it can be called a production candidate rather than a research winner

Simplest explanation:

The missing structure was not a new nonlinear latent model. It was a missing set of local charts near the production strip and the thin/wide autocall-gap edge. Once those geometries are explicitly present, the truth-aware geometry router can expose the low-dimensional value manifold much more faithfully.

## 19. Local Leg Residuals + Top-2 Routing (2026-04)


## Setup

This branch keeps the expanded `E9` atlas and fixed router configuration exactly:

- chart family: the 7-chart supplemental atlas from `E9`
- router family: `knn_error`
- router features: registered geometry only
- router k: `9`

New ingredients:

1. chart-local affine residual corrections for `V` and `U` on the two hardest charts;
2. top-2 chart evaluation with inverse-predicted-error blending when the router gap is small.

The residual correction is fit only on training-side geometry, never on VALIDATION or STRESS.

## Hard charts corrected

- hardest charts by `E9` oracle validation floor: `3,5`

- chart 3: train size `37`, train corrected worst / median `9.62 / 2.44 bp`
- chart 5: train size `46`, train corrected worst / median `20.10 / 5.25 bp`

## Results

Baseline `E9`:

- validation: `62.58 / 21.11 bp`
- stress: `30.68 / 9.16 bp`

Correction only, top-1 router:

- validation: `48.42 / 10.46 bp`
- stress: `142.71 / 2.78 bp`

Correction + ambiguity-triggered top-2:

- gap threshold: `8.75 bp`
- validation: `39.48 / 9.40 bp`
- stress: `71.89 / 4.69 bp`
- top-2 trigger rate: `23.3%` on validation and `69.2%` on stress
- estimated total CU: avg/max `541122 / 710611` on stress

## Residual validation spikes

- `(sigma=1.75, B=0.65, D=1.075)`: pred `2`, top2 `3`, `|q|=39.48 bp`, oracle `11.92 bp`, top2=`1`
- `(sigma=2.25, B=0.85, D=1.0375)`: pred `6`, top2 `2`, `|q|=35.34 bp`, oracle `35.34 bp`, top2=`0`
- `(sigma=1.35, B=0.65, D=1.075)`: pred `6`, top2 `2`, `|q|=35.16 bp`, oracle `5.76 bp`, top2=`1`
- `(sigma=1.75, B=0.85, D=1.0375)`: pred `6`, top2 `2`, `|q|=33.78 bp`, oracle `33.78 bp`, top2=`0`
- `(sigma=1.35, B=0.85, D=1.0375)`: pred `6`, top2 `2`, `|q|=31.55 bp`, oracle `31.55 bp`, top2=`0`
- `(sigma=2.25, B=0.85, D=1.0225)`: pred `3`, top2 `2`, `|q|=31.15 bp`, oracle `31.15 bp`, top2=`0`

## Engineering read

- total reduced payload of the `E9` atlas is unchanged; the residual correctors add only a few dozen scalars
- the cost increase comes only from occasional 2-chart evaluation, not a larger atlas

## Verdict

`DEAD` as the next universal accelerator architecture, though still useful as a falsification.

Why dead:

- chart-local affine leg correction does improve in-domain validation, from `62.58` to `48.42 bp` on top-1 and to `39.48 bp` with top-2 blending
- but the same correction is not robust out of sample: STRESS blows out from `30.68 bp` in `E9` to `142.71 bp` on top-1 and only recovers to `71.89 bp` with top-2
- the top-2 trigger rate jumps from `23.3%` on VALIDATION to `69.2%` on STRESS, so the ambiguity rule selected on VALIDATION does not transfer

Mathematical reason:

The hard-chart residual is not a stable smooth field over the full cell. A tiny affine corrector can remove local `V/U` bias near the fitted anchors, but outside that support it extrapolates in the wrong direction. Because the quote is the ratio `q=(1-V)/U`, small coupled errors in `V` and especially `U` are amplified. Blending an over-corrected chart with a good chart near the router boundary then converts a local bias fix into a larger fair-coupon error.

The next move should stay with the `E9` atlas and attack the hard cells directly: chart-local trust/fallback, explicit hard-cell chart refinement, or stronger truth-trained local charts. The residual idea should not ship without its own trust gate.

## 20. Trust-Gated Atlas vs Fallback (2026-04)


## Setup

This branch keeps the `E9` atlas and its fixed best router:

- chart family: expanded 7-chart supplemental atlas
- router: `knn_error` / `geo` / `k=9`
- truth target: dense `N=200`
- fallback proxy: dense `N=50` vs dense `N=200`

New ingredients:

1. a row-level `E9` miss ledger with chart/support diagnostics;
2. a chart-aware atlas-vs-fallback chooser trained only on VALIDATION;
3. no blending and no residual correction.

## E9 miss ledger

Validation miss classes:

- clean: count `4`, worst / median routed `1.29 / 0.54 bp`, median oracle `0.54 bp`, fallback-better `0`
- router_boundary: count `4`, worst / median routed `48.42 / 27.94 bp`, median oracle `1.55 bp`, fallback-better `4`
- mixed_router_floor: count `15`, worst / median routed `58.16 / 26.91 bp`, median oracle `18.74 bp`, fallback-better `12`
- chart_floor: count `37`, worst / median routed `62.58 / 19.76 bp`, median oracle `19.76 bp`, fallback-better `22`

Stress miss classes:

- clean: count `3`, worst / median routed `3.39 / 3.36 bp`, median oracle `3.36 bp`, fallback-better `0`
- chart_floor: count `10`, worst / median routed `30.68 / 18.19 bp`, median oracle `18.19 bp`, fallback-better `4`

The structural split is now explicit:

- STRESS is already oracle-floor-limited under `E9`; there are no router/oracle switches in the stress set
- VALIDATION still contains both router-boundary misses and true chart-floor misses

## Trust gate search

The frontier splits into three regimes.

Conservative coverage-max rule:

- family: `global_chart_knn`
- feature set: `geo_diag`
- k: `7`
- fallback std multiplier: `0.0`
- margin: `25.84 bp`
- validation: `58.16 / 20.33 bp` at `98.3%` atlas coverage
- stress: `30.68 / 9.16 bp` at `100.0%` atlas coverage

This policy is safe but timid. It only routes one validation point to fallback and leaves STRESS unchanged.

Practical high-coverage rule:

- family: `chart_local_knn`
- feature set: `geo_diag`
- k: `4`
- fallback std multiplier: `0.0`
- margin: `13.02 bp`
- validation: `48.42 / 16.93 bp` at `85.0%` atlas coverage
- stress: `19.47 / 7.32 bp` at `76.9%` atlas coverage

This is the useful operating point. It still keeps most of VALIDATION on the atlas path, but it materially improves both the validation and stress worst-case envelopes against dense `N=200`.

Absolute best VALIDATION frontier point:

- worst / median: `35.52 / 16.03 bp`
- atlas coverage: `60.0%`
- config: `global_chart_knn` / `geo_diag` / `k=5` / `std=1.0` / `margin=0.82`
- applied to STRESS: `144.24 / 8.14 bp` at `53.8%` atlas coverage

This kills the naive “just pick the lowest validation worst-case” rule. The most aggressive global chooser overfits validation and explodes on STRESS.

## Largest remaining hybrid misses

Validation, practical rule:

- `(sigma=1.35, B=0.65, D=1.075)`: path `atlas`, routed/oracle/fallback `48.42 / 5.76 / 18.54 bp`, class `router_boundary`, gap `6.55`
- `(sigma=2.25, B=0.55, D=1.075)`: path `atlas`, routed/oracle/fallback `39.67 / 39.67 / 33.69 bp`, class `chart_floor`, gap `8.56`
- `(sigma=2.25, B=0.55, D=1.0375)`: path `atlas`, routed/oracle/fallback `36.84 / 36.84 / 34.92 bp`, class `chart_floor`, gap `10.02`
- `(sigma=2.25, B=0.55, D=1.0225)`: path `fallback`, routed/oracle/fallback `58.16 / 27.66 / 35.52 bp`, class `mixed_router_floor`, gap `10.32`
- `(sigma=2.25, B=0.85, D=1.0375)`: path `atlas`, routed/oracle/fallback `35.34 / 35.34 / 98.26 bp`, class `chart_floor`, gap `9.57`
- `(sigma=2.25, B=0.75, D=1.0225)`: path `fallback`, routed/oracle/fallback `53.07 / 37.09 / 31.96 bp`, class `mixed_router_floor`, gap `24.60`

Stress, practical rule:

- `(sigma=1.5, B=0.7, D=1.0255)`: path `atlas`, routed/oracle/fallback `19.47 / 19.47 / 19.82 bp`, class `chart_floor`, gap `13.18`
- `(sigma=1.5, B=0.7, D=1.0245)`: path `atlas`, routed/oracle/fallback `18.69 / 18.69 / 19.85 bp`, class `chart_floor`, gap `13.88`
- `(sigma=2.5, B=0.5, D=1.1)`: path `atlas`, routed/oracle/fallback `17.69 / 17.69 / 38.24 bp`, class `chart_floor`, gap `2.45`
- `(sigma=1.17, B=0.6, D=1.05)`: path `atlas`, routed/oracle/fallback `9.16 / 9.16 / 8.91 bp`, class `chart_floor`, gap `19.93`
- `(sigma=1.17, B=0.7005, D=1.025)`: path `atlas`, routed/oracle/fallback `8.25 / 8.25 / 17.68 bp`, class `chart_floor`, gap `13.79`
- `(sigma=1.17, B=0.6995, D=1.025)`: path `atlas`, routed/oracle/fallback `8.14 / 8.14 / 17.65 bp`, class `chart_floor`, gap `13.85`
- `(sigma=1.17, B=0.8, D=1.025)`: path `fallback`, routed/oracle/fallback `30.68 / 30.68 / 7.32 bp`, class `chart_floor`, gap `23.54`
- `(sigma=2.0, B=0.7, D=1.0251)`: path `fallback`, routed/oracle/fallback `21.69 / 21.69 / 6.30 bp`, class `chart_floor`, gap `11.64`
- `(sigma=2.0, B=0.7, D=1.0249)`: path `fallback`, routed/oracle/fallback `21.37 / 21.37 / 6.30 bp`, class `chart_floor`, gap `11.66`

## Engineering read

- average one-chart atlas-path estimate is still about `295,714` total CU
- atlas payload remains about `594,432` bytes across all chart accounts
- this trust layer adds only router metadata plus a small chooser, but the repo still does not have an exact Rust/Anchor benchmark for the expanded atlas router + chart-deserialization path

## Verdict

`ALIVE` as the next research architecture in front of direct ZL-SVD.

Why alive:

- the row-level ledger confirms the structural split the earlier summaries only hinted at: STRESS is chart-floor-limited, while VALIDATION still mixes router-boundary and chart-floor misses
- a practical chart-local trust gate materially improves both strong-truth envelopes without collapsing atlas usage:
  - validation improves from `62.58 / 21.11 bp` to `48.42 / 16.93 bp`
  - stress improves from `30.68 / 9.16 bp` to `19.47 / 7.32 bp`
  - atlas coverage stays at `85.0%` on VALIDATION and `76.9%` on STRESS
- the most aggressive global chooser is explicitly falsified by STRESS, so the transferable object is a local trust rule, not a universal global error surrogate

What this means:

The next production-minded architecture is no longer plain `E9`. It is `E9` plus a chart-local trust/fallback chooser. The trust layer is doing real work: it hands the chart-3 floor pocket and the production-strip stress pocket back to fallback, while leaving genuinely bad fallback zones on the atlas path.

What remains:

- the remaining worst hybrid validation miss is now the router-boundary pocket around `(sigma=1.35, B=0.65, D=1.075)`
- chart-floor points near `(sigma≈2.25, B≈0.55, D∈{1.0225,1.0375,1.075})` still remain on the atlas path when fallback is only marginally better
- the exact Rust/Anchor benchmark for router + chart deserialization + trust branch is still missing

So `E12` should now split into two goals:

1. if the objective is lower hybrid worst-case, target the remaining `D≈1.075` router-boundary strip;
2. if the objective is higher atlas coverage at the same guard, target the chart-3 / chart-5 floor pockets that the trust layer is currently offloading to fallback.

## E12 Boundary-Chart Refinement on Top of E9 + Trust Gate

I ran the next literal step after `E11`: add one more `N=200` local chart on top of `E9`, then rerun the trust gate. The objective was explicit: lower the practical hybrid worst-case while staying in a one-chart / one-transaction regime.

Three chart families were tested:

1. `E12_boundary_bridge`: bridge the low-`B` `D≈1.0375..1.075` strip;
2. `E12_low_b_full_strip`: cover the low-`B` strip from the thin-gap production edge through `D≈1.075`;
3. `E12_d1075_focus`: focus only on the wide-gap `D=1.075` regime.

All three were trained with off-validation anchors and evaluated against dense `N=200`.

The raw result looked ambiguous because the branch scripts re-searched the practical gate and hit a validation tie. So I re-applied the published `E11` practical policy directly:

- fixed gate: `chart_local_knn / geo_diag / k=4 / std=0 / margin=13.02252`
- honest `E11` baseline: validation `48.42 / 16.93 bp`, stress `19.47 / 7.32 bp`

Under that honest gate:

- `E12_boundary_bridge`: validation `56.30 / 18.64 bp`, stress `17.73 / 7.32 bp`
- `E12_low_b_full_strip`: worse than the bridge branch and not competitive
- `E12_d1075_focus`: not selected even before the honest comparison; it worsened the practical overall worst-case to `50.04 bp`

The decisive failure mode was structural, not accidental:

- the new chart medoids collapsed back to old base-cloud points like `(1.17, 0.50, 1.05)` instead of the hard strip
- local dimension curves were catastrophic, with local worst errors still in the `500-700 bp` range
- the winning `E12` chart created a new low-`B`, thin-gap floor:
  - `(sigma=2.25, B=0.55, D=1.0225)` jumped to routed `56.30 bp`
  - `(sigma=2.25, B=0.65, D=1.0225)` became a new chart-floor point at `46.56 bp`
- meanwhile the original hybrid max never moved:
  - `(sigma=1.35, B=0.65, D=1.075)` stayed at `48.42 bp`
  - oracle chart remained `4`, not the new chart

So `E12` is **DEAD** for the stated objective.

The mathematical reason is simple: this was not a missing local basis around the hybrid max. The new chart family did not lower the oracle floor there. It just added another chart with a badly centered registration map and created fresh low-`B` chart floors.

## E13 Fixed-Reference Boundary Charts

Because `E12` failed by collapsing the chart-specific reference geometry back to the base cloud, I ran the natural repair:

1. keep the same support clouds;
2. force the new chart to register around explicit hard-strip references;
3. rerun the same dense `N=200` evaluation.

Two fixed-reference charts were tested:

- `E13_boundary_bridge_ref`: forced reference `(sigma=1.50, B=0.62, D=1.0625)`
- `E13_low_b_strip_ref`: forced reference `(sigma=1.50, B=0.62, D=1.0265)`

This did improve the *local* dimension curves materially:

- `E12_boundary_bridge` local worst `510.87 bp` -> `E13_boundary_bridge_ref` `301.64 bp`
- `E12_low_b_full_strip` local worst `661.54 bp` -> `E13_low_b_strip_ref` `557.73 bp`

But the honest hybrid comparison still killed the branch.

Under the same fixed published `E11` practical gate:

- `E13_boundary_bridge_ref`: validation `48.42 / 17.59 bp`, stress `22.45 / 8.14 bp`
- `E13_low_b_strip_ref`: validation `48.55 / 16.58 bp`, stress `19.47 / 7.32 bp`

So:

- no `E13` branch improved the validation worst-case below `48.42 bp`
- the bridge chart worsened stress to `22.45 bp`
- the low-`B` strip chart slightly worsened validation to `48.55 bp`
- the remaining hybrid max still did not move:
  - `(sigma=1.35, B=0.65, D=1.075)` stayed at `48.42 bp`
  - oracle chart still remained `4`

What *did* move was a stress oracle floor:

- `E13_boundary_bridge_ref` cut stress oracle worst from `30.68 bp` to `22.45 bp`

But that is not the objective I asked this branch to solve. The branch did not reduce the practical hybrid max, and it worsened the honest stress envelope. So `E13` is also **DEAD** as a next step for lower hybrid worst-case.

## Updated Reading

The surviving object is now clearer:

- the remaining hybrid max is not a missing chart-floor patch
- it is a routing residual on the existing chart-`4` / chart-`6` boundary
- new local charts do not help because the oracle at that point is still the old chart family

The best concise statement is:

> value-manifold localization is still useful for atlas coverage, but it is no longer the live direction for lowering the hybrid worst-case.

The live next direction is now router-side, not chart-side:

1. topology-first / geometry-second router override on the `chart 4` vs `chart 6` ambiguity strip;
2. nearest-chart or medoid-aware boundary override diagnostics;
3. only accept a router refinement if it lowers the honest hybrid worst-case under the published `E11` practical gate.

## E14 Router Boundary Override

I then ran that router-side branch exactly as framed:

- keep the `E9` atlas fixed
- keep the published `E11` practical trust gate fixed:
  - `chart_local_knn / geo_diag / k=4 / std=0 / margin=13.02252`
- only change the chart assignment on the remaining `chart 6 -> chart 4` ambiguity strip

The search space was deliberately narrow and topology-first:

1. only touch points with base route `6`
2. require nearest-chart geometry to point at chart `4`
3. require the wide-gap cell `D >= 1.075`
4. override target is always chart `4`

Inside that strip I searched:

- raw strip rules in `(sigma, B, router gap)`
- registered-geometry rectangles in `(u1, u2)` with optional sigma floor

The honest baseline under the fixed gate is:

- validation `48.42 / 16.93 bp`
- stress `19.47 / 7.32 bp`
- overall worst `48.42 bp`

The branch is **ALIVE**.

Best rule by the scripted tie-break was the geometry rectangle:

- `geom_u1_u2 / u1<=-0.659 / u2<=-0.517`

That rule overrides exactly one validation point and zero stress points:

- `(sigma=1.35, B=0.65, D=1.075)` moves from chart `6` to chart `4`
- routed atlas error drops from `48.42 bp` to `5.76 bp`
- fallback there is `18.54 bp`, so the fixed trust gate keeps the atlas path

Result under the same fixed gate:

- validation `39.67 / 16.77 bp`
- stress `19.47 / 7.32 bp`
- overall worst `39.67 bp`

So the honest hybrid worst-case improves:

- `48.42 -> 39.67 bp`

with:

- no new charts
- no new payload
- no change to the one-chart online solve
- average atlas-path estimate still about `295,714` total CU
- atlas payload still `594,432` bytes

There is also a production-simpler equivalent rule on the tested box:

- `raw_sigma_b / s>=1.35 / B<=0.65`

It produces the same strong-truth result because it hits the same single point. The geometry rule won only by tie-break because the branch was explicitly framed as topology-first / geometry-second. For implementation simplicity, the raw strip override is the easier thing to ship if future tests keep it equivalent.

Most importantly, this answers the structural question cleanly:

- the surviving `48.42 bp` max really was a router-boundary error on the existing chart family
- it was **not** a missing local chart

After this override, the new hybrid worst validation miss is:

- `(sigma=2.25, B=0.55, D=1.075)` at `39.67 bp`

and that point is chart-floor-limited, not router-limited.

So the state of the research now is:

1. `E9` chart-local trust-gated atlas remains the live accelerator
2. chart-side refinement for lower hybrid worst-case is dead
3. a one-sided `4`/`6` router override is alive and materially improves the honest envelope
4. the next lower-hybrid objective, if pursued, is no longer the `4`/`6` boundary; it is the high-sigma low-`B` chart-floor region around chart `3` / fallback competition

## E15 Exact BPF Gate For The Hybrid Shell

After `E14`, the next unresolved question was no longer statistical. It was engineering:

> does the actual one-transaction hybrid shell still fit on BPF once the router, trust gate, and one-sided `4`/`6` override are part of the instruction path?

This needed a precise answer because the research winner up to `E14` is still a Python-side object built around `run_projected_rom`, which touches full-space operators and is therefore **not yet a deployable serialized chart payload**. So I measured the **deployable shell**, not the non-deployable Python operator path:

- router metadata
- chart/trust decision
- one-sided `4`/`6` override
- then exactly one online branch:
  - pure reduced split-ROM chart solve, or
  - direct `N=50` low-rank SVD fallback

Implementation:

- added four new Anchor bench instructions in `programs/autocall-bench/src/lib.rs`
  - `bench_hybrid_router_only`
  - `bench_hybrid_atlas_c24`
  - `bench_hybrid_override_c16`
  - `bench_hybrid_fallback_r17`
- wired them into `programs/autocall-bench/bench_cu.ts`
- rebuilt with `anchor build -p autocall_bench`
- deployed with `anchor deploy -p autocall_bench`
- measured on localnet (`solana-test-validator 2.3.0`)

Measured totals:

| Instruction | Path | Total CU | Headroom vs 1.4M |
|---|---:|---:|---:|
| `bench_hybrid_router_only` | metadata only | `2,760` | `1,397,240` |
| `bench_hybrid_atlas_c24` | atlas hit, chart `5`, `d=24` | `535,204` | `864,796` |
| `bench_hybrid_override_c16` | atlas hit after `4/6` override, chart `4`, `d=16` | `257,323` | `1,142,677` |
| `bench_hybrid_fallback_r17` | trust-gated fallback, direct SVD `r=17` | `1,351,849` | `48,151` |

Reference baselines from the same run:

| Instruction | Meaning | Total CU |
|---|---:|---:|
| `bench_split_rom_d24` | bare pure reduced atlas-core proxy | `534,811` |
| `bench_backward_lr_n50_r17` | bare direct-SVD fallback | `1,351,217` |

So the important engineering fact is now pinned down:

- the **worst hybrid branch is the fallback branch**
- it costs `1,351,849` total CU
- it still fits under the `1.4M` ceiling
- remaining headroom is about `48.2K` CU

The shell overhead itself is tiny:

- atlas `d=24` shell vs bare split-ROM proxy:
  - `535,204 - 534,811 = 393` total CU
- fallback shell vs bare direct-SVD fallback:
  - `1,351,849 - 1,351,217 = 632` total CU
- router/trust metadata alone:
  - `2,760` total CU

So the practical conclusion is:

1. the **one-transaction hybrid shell is viable on BPF**
2. `E14` is now alive not only as a strong-truth research result, but as a **deployable compute envelope**
3. the remaining blocker is **not** compute
4. the remaining blocker is still the math/serialization gap between the Python research atlas and a true pure-reduced chart payload

This means the next production-minded question is no longer:

> can router + trust + fallback fit in one transaction?

That answer is now **yes**.

The next real question is:

> can the surviving chart-local accuracy gains be recast into a pure reduced chart format without reintroducing the old chart-floor failures?

That is a different research problem from CU budgeting.

## E16 Pure Reduced Chart Payload

I ran that next problem directly.

The question was:

> can the surviving `E14` chart-local gains be rewritten as a **true serialized reduced payload** instead of a Python-side `run_projected_rom` path that still touches full-space operators online?

To test that honestly, I kept the published `E14` route/gate fixed and changed only the atlas-path solver.

Branch design:

1. build the same expanded `E9` / `E14` chart family;
2. for each active hard chart, extract the exact reduced staged factors
   `(term, G_red, A_red, b_red)` against dense `N=200`;
3. stack those factors into one chart-local factor vector per training anchor;
4. compress the factor family with SVD inside the chart;
5. interpolate the factor coefficients from chart geometry features with local IDW kNN;
6. solve the quote from the reconstructed reduced factors only.

This is the first branch that actually tries to turn the research atlas into a serializable pure reduced payload.

Artifacts:

- script: `research/E16_pure_reduced_chart_payload.py`
- report: `research/pure_reduced_chart_payload_report.md`
- summaries:
  - `research/results/pure_reduced_chart_payload_summary.csv`
  - `research/results/pure_reduced_chart_payload_charts.csv`
  - `research/results/pure_reduced_chart_payload_pointwise.csv`
  - `research/results/pure_reduced_chart_payload_baseline_compare.csv`
- cached chart-local payload models:
  - `research/results/pure_payload_chart_3.npz`
  - `research/results/pure_payload_chart_4.npz`
  - `research/results/pure_payload_chart_5.npz`
  - `research/results/pure_payload_chart_6.npz`

To keep the run feasible, the prototype payload replacement was applied only to the hard active charts `3/4/5/6`; cold charts `0/2` were left untouched on their original projected solver. So the comparison is still the honest fixed-`E14` envelope with only the hard atlas cells replaced.

Result: **DEAD**.

Under the same fixed `E14` route/gate:

- baseline `E14`:
  - validation `39.67 / 16.77 bp`
  - stress `19.47 / 7.32 bp`

- pure reduced payload:
  - validation `902.44 / 111.15 bp`
  - stress `265.52 / 42.84 bp`

Payload-only inflation on atlas-path points:

- validation `915.39 / 179.82 bp`
- stress `283.22 / 91.48 bp`

The failure is not marginal. It is structural.

Per hard chart:

- chart `3`: local validation payload error `656.76 / 253.45 bp`
- chart `4`: local validation payload error `915.39 / 407.75 bp`
- chart `5`: local validation payload error `248.37 / 133.20 bp`
- chart `6`: local validation payload error `443.05 / 83.24 bp`

Representative blow-ups:

- `(sigma=0.9, B=0.55, D=1.075)` on chart `4`:
  baseline `12.95 bp` -> pure payload `902.44 bp`
- `(sigma=1.35, B=0.65, D=1.075)` on chart `4`:
  baseline `5.76 bp` -> pure payload `474.23 bp`
- `(sigma=2.25, B=0.85, D=1.0225)` on chart `3`:
  baseline `24.00 bp` -> pure payload `632.76 bp`
- `(sigma=1.35, B=0.85, D=1.0375)` on chart `6`:
  baseline `31.55 bp` -> pure payload `474.60 bp`

The mathematical reason is now clearer than before:

- the chart-local **value** manifold is lower-dimensional than the full operator,
  but the chart-local family of **reduced staged factors** is still too warped
  across the hard geometry cells;
- once I stop recomputing those factors from the exact full-space map and try to
  interpolate them as a serialized payload, the hard charts immediately blow up;
- so the surviving low-dimensional object is **not** “chart-local staged factors
  with a simple geometry interpolant”.

This is a valuable kill:

1. it means the BPF shell can ship, but only if the chart factors themselves are
   built concretely offline, not regenerated from a tiny local factor model;
2. it means the next live math question is not “how do I serialize the current
   chart factors with a small IDW/SVD model?”;
3. it is instead one of:
   - much smaller charts / stronger topology partitioning,
   - a different payload target (for example local `U0/V0` correction instead of
     full staged factors),
   - or accepting that the atlas remains a front-end/router around direct SVD
     rather than becoming a standalone serialized quote engine.

So the updated state is:

- `E14` hybrid shell on BPF: **ALIVE**
- pure serialized chart-factor interpolation on the hard cells: **DEAD**

This kills the simplest “serialize the current atlas factors directly” path.

## E17 Chart-Local U/V Residual Correction

After `E16` killed direct serialization of the staged factors, I ran the next obvious experiment:

> keep the fixed `E14` hybrid architecture, but add tiny chart-local truth corrections on the **legs**,
> not on fair coupon directly.

Setup:

- same expanded chart family
- same one-sided `4`/`6` boundary override
- same trust/fallback decisions as `E14`
- only change atlas-path quotes

For each hard active chart `3/4/5/6`, I built train-only residual datasets:

- `ΔV = V_truth - V_atlas`
- `ΔU = U_truth - U_atlas`

using dense `N=200` truth and the current projected chart solver.

Per chart I searched small correction families with leave-one-out fair-coupon error on the chart training anchors:

- zero correction
- constant mean
- affine in geometry features
- diagonal quadratic
- local IDW kNN

with features:

- `geo = (u1,u2,u3,rho1,rho2,kappa)`
- `geo_sigma = (u1,u2,u3,rho1,rho2,kappa,sigma)`

and optional support guards that shut the correction off outside the chart cloud.

Selected models:

- chart `3`: `V:idw3 / U:idw3` on `geo`
- chart `4`: `V:idw3 / U:idw3` on `geo`, with guard `1.0`
- chart `5`: `V:affine / U:affine` on `geo`
- chart `6`: `V:idw3 / U:idw3` on `geo`

Raw branch result:

- baseline `E14`:
  - validation `39.67 / 16.77 bp`
  - stress `19.47 / 7.32 bp`

- corrected hybrid:
  - validation `207.40 / 14.76 bp`
  - stress `10.07 / 6.30 bp`

So the branch is **not** a clean win.

It does do real work on some pockets:

- `(1.35, 0.85, 1.0375)` on chart `6`: `31.55 -> 0.08 bp`
- `(1.75, 0.75, 1.075)` on chart `6`: `26.74 -> 0.54 bp`
- stress `(1.5, 0.7, 1.0255)` on chart `5`: `19.47 -> 0.47 bp`

But it also creates new large misses:

- `(2.25, 0.85, 1.0225)` on chart `3`: `24.00 -> 207.40 bp`
- `(2.25, 0.75, 1.0375)` on chart `3`: `25.35 -> 180.77 bp`
- `(1.75, 0.55, 1.0225)` on chart `5`: `35.25 -> 117.74 bp`

So as an always-on correction layer this branch is **DEAD** for the actual objective, which is lower hybrid worst-case.

I then ran the immediate subset diagnostic on top of the fitted corrections: enable the correction only on a subset of charts.

Best validation-first subset:

- chart `4` only

That gives:

- validation worst unchanged at `39.67 bp`
- validation median improved to `16.03 bp`
- stress unchanged at `19.47 / 7.32 bp`

This is useful evidence:

- the leg-residual idea is **partly real**
- it is safe and mildly helpful on chart `4`
- it is strongly helpful on parts of chart `6`
- but charts `3` and `5` remain too support-sensitive for an always-on local residual model

So the correct reading is:

1. chart-local leg correction is not fake
2. but it is not yet a global hybrid upgrade
3. the remaining hard cells still need stronger localization or a different target entirely

For the stated objective:

- pure staged-factor serialization (`E16`): **DEAD**
- global chart-local leg correction (`E17`): **DEAD**

The live line is now narrower:

- keep the `E14` hybrid shell
- only deploy additional local correction where it is chart-wise demonstrably safe
- or change the approximation target again, rather than force one residual layer across the hard atlas cells

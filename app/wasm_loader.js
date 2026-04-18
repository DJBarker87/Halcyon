/* global window */
// Halcyon — WASM loader.
//
// Loads the solmath-core WASM shim and exposes it on `window.HalcyonMath.wasm`.
// pricing.js probes for `HalcyonMath.wasm` at each call and uses it when
// available, falling back to the JS approximations otherwise.
//
// The WASM binary is built from /crates/halcyon-wasm against solmath-core's
// `bs + transcendental + barrier` feature set. All numeric values at the
// boundary are f64; the shim converts to 1e12 fixed-point internally.

(function () {
  const WASM_URL = 'halcyon_wasm.wasm';

  // Initialize facade synchronously so JS callers can probe without a race.
  window.HalcyonMath = window.HalcyonMath || {};
  window.HalcyonMath.wasmReady = false;

  const t0 = performance.now();

  WebAssembly.instantiateStreaming(fetch(WASM_URL), {})
    .catch(err => {
      // Older Safari / file:// fallback: fetch + instantiate.
      console.warn('[halcyon-wasm] streaming failed, retrying with fetch:', err);
      return fetch(WASM_URL)
        .then(r => r.arrayBuffer())
        .then(buf => WebAssembly.instantiate(buf, {}));
    })
    .then(result => {
      const exports = result.instance.exports;
      const scale = exports.sm_scale();
      if (scale !== 1e12) {
        console.error('[halcyon-wasm] unexpected SCALE:', scale, 'expected 1e12');
        return;
      }
      window.HalcyonMath.wasm = {
        // ---- Primitives (solmath-core) ----
        bs_call:         (s, k, r, sigma, t)       => exports.sm_bs_call(s, k, r, sigma, t),
        bs_put:          (s, k, r, sigma, t)       => exports.sm_bs_put(s, k, r, sigma, t),
        implied_vol:     (mp, s, k, r, t)          => exports.sm_implied_vol(mp, s, k, r, t),
        norm_cdf:        x                         => exports.sm_norm_cdf(x),
        norm_pdf:        x                         => exports.sm_norm_pdf(x),
        erf:             x                         => exports.sm_erf(x),
        compute_il:      (w, x)                    => exports.sm_compute_il(w, x),
        barrier_hit_prob:(spot, b, sigma, t, isUp) => exports.sm_barrier_hit_prob(spot, b, sigma, t, isUp),
        barrier_option:  (s, k, h, r, sigma, t, isCall, kind) =>
                         exports.sm_barrier_option(s, k, h, r, sigma, t, isCall, kind),
        // ---- Product pricers (halcyon-quote) ----
        // IL European NIG premium via 5-point Gauss-Legendre quadrature (i64/SCALE_6).
        // Returns premium as a fraction of insured value; caller applies ×1.10 load.
        il_fair_premium: (sigmaAnn, days, ded, cap, alpha, beta) =>
                         exports.sm_il_fair_premium(sigmaAnn, days, ded, cap, alpha, beta),
        // SOL autocall fair coupon per observation (decimal).
        // Primary: POD-DEIM E11 live operator (σ ∈ [0.50, 2.50]).
        // Fallback: gated Richardson CTMC (N1=10, N2=15).
        sol_fair_coupon:    sigmaAnn               => exports.sm_sol_fair_coupon(sigmaAnn),
        sol_pricing_engine: sigmaAnn               => exports.sm_sol_pricing_engine(sigmaAnn),
        // Worst-of-3 (SPY/QQQ/IWM) 18m autocall — projected c1 filter at K=12
        // + K=12 correction, exact on-chain path. σ ∈ [0.08, 0.80].
        // Returns fair_coupon_bps per quarterly observation (1 bp = 0.0001).
        worst_of_k12_coupon_bps:     sigmaAnn => exports.sm_worst_of_k12_coupon_bps(sigmaAnn),
        worst_of_k12_knock_in_rate:  sigmaAnn => exports.sm_worst_of_k12_knock_in_rate(sigmaAnn),
        worst_of_k12_autocall_rate:  sigmaAnn => exports.sm_worst_of_k12_autocall_rate(sigmaAnn),
        _scale: scale,
      };
      window.HalcyonMath.wasmReady = true;
      const dt = (performance.now() - t0).toFixed(1);
      console.log(
        `[halcyon-wasm] solmath-core loaded (${dt}ms). `
        + `Sanity: BS(100,100,0.05,0.2,1) call = `
        + `${exports.sm_bs_call(100, 100, 0.05, 0.2, 1).toFixed(6)}, `
        + `IL(w=0.5, x=1.5) = `
        + `${exports.sm_compute_il(0.5, 1.5).toFixed(6)}`
      );
      window.dispatchEvent(new CustomEvent('halcyon-wasm-ready'));
    })
    .catch(err => {
      console.error('[halcyon-wasm] load failed, staying on JS fallback:', err);
    });
})();

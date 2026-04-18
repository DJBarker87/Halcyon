/* global window */
// Halcyon — pricing math.
//
// Every helper here that has a solmath-core analog prefers the WASM path when
// `window.HalcyonMath.wasm` is loaded. The JS implementations stay as the
// first-paint fallback (before WASM has finished streaming in) and as the
// "WASM failed" fallback. All WASM probes are lazy — resolved at call time,
// not at module-evaluation time.

// ---------- WASM availability probe ----------
function wasmFn(name) {
  const w = window.HalcyonMath && window.HalcyonMath.wasm;
  return w && w[name];
}

// ---------- Black-Scholes helpers ----------
// Abramowitz-Stegun 7.1.26 fallback. Matches solmath-core's norm-cdf_poly to
// ~5 decimals; the WASM path is the i128 fixed-point original.
function erfJS(x) {
  const a1 = 0.254829592, a2 = -0.284496736, a3 = 1.421413741;
  const a4 = -1.453152027, a5 = 1.061405429, p = 0.3275911;
  const sign = x < 0 ? -1 : 1;
  x = Math.abs(x);
  const t = 1 / (1 + p * x);
  const y = 1 - (((((a5*t + a4)*t) + a3)*t + a2)*t + a1)*t * Math.exp(-x*x);
  return sign * y;
}
const erf     = x => { const f = wasmFn('erf');      return f ? f(x) : erfJS(x); };
const N       = x => { const f = wasmFn('norm_cdf'); return f ? f(x) : 0.5 * (1 + erfJS(x / Math.SQRT2)); };
const normPdf = x => { const f = wasmFn('norm_pdf'); return f ? f(x) : Math.exp(-x*x/2) / Math.sqrt(2 * Math.PI); };

// ---------- Black-Scholes call/put (via WASM when available) ----------
function bsCall(s, k, r, sigma, t) {
  const f = wasmFn('bs_call');
  if (f) return f(s, k, r, sigma, t);
  // JS fallback: standard BS closed form.
  if (t <= 0 || sigma <= 0) return Math.max(0, s - k);
  const d1 = (Math.log(s / k) + (r + 0.5 * sigma * sigma) * t) / (sigma * Math.sqrt(t));
  const d2 = d1 - sigma * Math.sqrt(t);
  return s * N(d1) - k * Math.exp(-r * t) * N(d2);
}
function bsPut(s, k, r, sigma, t) {
  const f = wasmFn('bs_put');
  if (f) return f(s, k, r, sigma, t);
  if (t <= 0 || sigma <= 0) return Math.max(0, k - s);
  const d1 = (Math.log(s / k) + (r + 0.5 * sigma * sigma) * t) / (sigma * Math.sqrt(t));
  const d2 = d1 - sigma * Math.sqrt(t);
  return k * Math.exp(-r * t) * N(-d2) - s * N(-d1);
}

// ---------- Implied volatility (WASM only — JS Newton is tedious) ----------
// Returns NaN if WASM is unavailable or the solver fails.
function impliedVol(marketPrice, s, k, r, t) {
  const f = wasmFn('implied_vol');
  return f ? f(marketPrice, s, k, r, t) : NaN;
}

// ---------- Barrier primitives (WASM only — no JS closed form) ----------
// First-passage probability under r=0 GBM drift. `isUpper`: true if barrier > spot.
function barrierHitProb({ spot, barrier, sigma, tenorYears, isUpper }) {
  const f = wasmFn('barrier_hit_prob');
  return f ? f(spot, barrier, sigma, tenorYears, isUpper ? 1 : 0) : NaN;
}

// European barrier option price. `kind`: 'DO' (down-and-out), 'DI', 'UO', 'UI'.
const BARRIER_KIND = { DO: 0, DI: 1, UO: 2, UI: 3 };
function barrierOptionPrice({ s, k, h, r, sigma, t, isCall, kind }) {
  const f = wasmFn('barrier_option');
  if (!f) return NaN;
  const code = BARRIER_KIND[kind];
  if (code === undefined) return NaN;
  return f(s, k, h, r, sigma, t, isCall ? 1 : 0, code);
}

// ---------- Convenience: autocall KI probability ----------
// For a single-asset autocall with daily knock-in monitoring over `tenorDays`.
// `entrySpot = 1.0` means use normalized terms; barrier is the KI level (e.g. 0.70).
function autocallKiProbability({ knockIn, sigma, tenorDays }) {
  const tenorYears = tenorDays / 365;
  // Knock-in at a level below spot → down-barrier, so isUpper = false.
  return barrierHitProb({
    spot: 1.0, barrier: knockIn, sigma, tenorYears, isUpper: false,
  });
}

// ---------- Worst-of-3 autocall payoff ----------
// Payoff shape for visualization (piecewise, not a WASM candidate):
//   - If min(S_T/S_0) ≥ autocallBar: principal + coupon_accrued
//   - If min(S_T/S_0) ≥ knockIn    : principal (no loss)
//   - Otherwise                     : principal × min(S_T/S_0) (1-for-1 downside)
function worstOfAutocallPayoff({ worstPerf, autocallBar = 1.0, knockIn = 0.70, coupon = 0.08 }) {
  if (worstPerf >= autocallBar) return 1 + coupon;
  if (worstPerf >= knockIn)     return 1;
  return worstPerf;
}
function worstOfAutocallCurve(opts = {}) {
  const xs = [];
  for (let x = 0.3; x <= 1.3; x += 0.01) xs.push(x);
  return xs.map(x => ({ x, y: worstOfAutocallPayoff({ worstPerf: x, ...opts }) }));
}

// ---------- IL Protection payoff ----------
// Synthetic IL contract on a 50/50 pool. Loss fraction of current-hold value
// derived from solmath-core's compute_il (WASM) when available, else the
// analytic closed form `1 − 2·√r/(1+r)`.
function ilPayoff({ r, deductible = 0.02, cap = 0.30 }) {
  let loss;
  const wasmIL = wasmFn('compute_il');
  if (wasmIL) {
    // Solmath returns IL as a fraction of initial portfolio value (w·x + (1-w) - x^w).
    // Rescale to fraction-of-current-hold to match the downstream payoff convention.
    const ilAbs   = wasmIL(0.5, r);
    const holdNow = 0.5 * r + 0.5;
    loss = ilAbs / holdNow;
  } else {
    loss = 1 - 2 * Math.sqrt(r) / (1 + r);
  }
  return Math.max(0, Math.min(cap, loss - deductible));
}
function ilPayoffCurve() {
  const xs = [];
  for (let r = 0.25; r <= 4.0; r += 0.02) xs.push(r);
  return xs.map(r => ({ x: r, y: ilPayoff({ r }) }));
}

// ---------- SOL autocall (single asset) ----------
function solAutocallPayoff({ perf, autocallBar = 1.0, knockIn = 0.65, coupon = 0.12 }) {
  if (perf >= autocallBar) return 1 + coupon;
  if (perf >= knockIn)     return 1;
  return perf;
}
function solAutocallCurve(opts = {}) {
  const xs = [];
  for (let x = 0.3; x <= 1.4; x += 0.01) xs.push(x);
  return xs.map(x => ({ x, y: solAutocallPayoff({ perf: x, ...opts }) }));
}

// ============================================================================
// VISUALIZATION HELPERS — NOT PRICING
// ============================================================================
// The functions below are pure-JS toys kept for display surfaces (stylised
// distribution histograms, mock historical price series). They use a
// linear-congruential RNG and a plain Brownian walk — no NIG, no fat tails,
// no halcyon-quote, no WASM. They exist because some design surfaces want a
// shape to draw, not a priced number.
//
// Real pricing lives in halcyon-quote via the WASM shim:
//   - IL fair premium:  HalcyonMath.wasm.il_fair_premium  (nig_european_il_premium)
//   - SOL fair coupon:  HalcyonMath.wasm.sol_fair_coupon  (POD-DEIM E11 / Richardson)
//
// If you see a UI number that traces back to one of these helpers, either
// the UI is wrong or the helper is misused — it should never sit on the
// critical pricing path.
// ============================================================================

// Stylised max-drawdown histogram. Pure-JS Box-Muller GBM, LCG seed, Gaussian
// returns. **Not an IL pricer.** The IL pricer is sm_il_fair_premium.
function visualMockDrawdownHistogram({ vol = 0.60, drift = -0.02, days = 30, samples = 2000, seed = 42 }) {
  let s = seed;
  const rand = () => { s = (s * 9301 + 49297) % 233280; return s / 233280; };
  const boxMuller = () => {
    const u = Math.max(1e-9, rand()), v = rand();
    return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
  };
  const losses = [];
  for (let i = 0; i < samples; i++) {
    let logS = 0;
    let minLogS = 0;
    const dt = 1 / 365;
    for (let d = 0; d < days; d++) {
      logS += drift * dt + vol * Math.sqrt(dt) * boxMuller();
      if (logS < minLogS) minLogS = logS;
    }
    const maxDraw = 1 - Math.exp(minLogS);
    losses.push(Math.max(0, maxDraw));
  }
  losses.sort((a, b) => a - b);
  const nBins = 40;
  const max = Math.min(1.0, losses[Math.floor(samples * 0.999)]);
  const bins = new Array(nBins).fill(0);
  losses.forEach(l => {
    const idx = Math.min(nBins - 1, Math.floor((l / max) * nBins));
    bins[idx]++;
  });
  const density = bins.map((c, i) => ({
    x: (i + 0.5) / nBins * max,
    y: c / samples
  }));
  const p = q => losses[Math.floor(samples * q)];
  return {
    density,
    max,
    p50: p(0.5), p90: p(0.9), p95: p(0.95), p99: p(0.99),
    mean: losses.reduce((a,b)=>a+b, 0) / samples,
  };
}

// Stylised actuarial formula on top of the mock histogram above.
// **Not an IL pricer.** The real IL premium is `HalcyonMath.wasm.il_fair_premium`.
function visualMockActuarialPremium({ dist, riskLoad = 0.25 }) {
  const eLoss = dist.mean;
  const var95 = dist.p95;
  return eLoss + (var95 - eLoss) * riskLoad;
}

// Stylised price series for chart rendering. **Not real market data.**
// Live spot comes from `HalcyonOracles.subscribeSpot` (Pyth Hermes).
function mockSeries({ days = 90, start = 100, vol = 0.05, drift = 0.001, seed = 7 }) {
  let s = seed;
  const rand = () => { s = (s * 9301 + 49297) % 233280; return s / 233280; };
  const bm = () => Math.sqrt(-2 * Math.log(Math.max(1e-9, rand()))) * Math.cos(2 * Math.PI * rand());
  const out = [{ t: 0, v: start }];
  let v = start;
  for (let i = 1; i <= days; i++) {
    v = v * Math.exp(drift + vol * bm());
    out.push({ t: i, v });
  }
  return out;
}

// Merge into any HalcyonMath namespace wasm_loader.js may have seeded so the
// async-loaded WASM facade survives this synchronous assignment.
window.HalcyonMath = Object.assign(window.HalcyonMath || {}, {
  // Primitives
  erf, N, normPdf,
  bsCall, bsPut, impliedVol,
  barrierHitProb, barrierOptionPrice, autocallKiProbability,
  // Payoff shapes
  worstOfAutocallPayoff, worstOfAutocallCurve,
  ilPayoff, ilPayoffCurve,
  solAutocallPayoff, solAutocallCurve,
  // Visualization helpers — NOT pricers (see loud header block above)
  visualMockDrawdownHistogram, visualMockActuarialPremium, mockSeries,
});

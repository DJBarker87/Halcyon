/* global window */
// Halcyon — oracle layer.
//
// Two data sources, both public / no-auth / CORS-enabled:
//
//   1. Pyth Hermes — current spot for SOL/USD. Polls every few seconds.
//      Endpoint: https://hermes.pyth.network/api/latest_price_feeds?ids[]=<hex>
//      SOL/USD feed ID: 0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d
//
//   2. Binance klines — last 46 daily closes for SOLUSDT. Refreshed once per
//      UTC day and used to compute the 45-day EWMA variance → annualized σ.
//      Stored in localStorage keyed by YYYY-MM-DD so page reloads hit cache.
//
// Both expose React-friendly subscriber APIs on `window.HalcyonOracles`:
//   - HalcyonOracles.subscribeSpot(symbol, fn)       → unsubscribe fn
//   - HalcyonOracles.subscribeEwma(symbol, fn)       → unsubscribe fn
//   - HalcyonOracles.getSpot(symbol)                 → { price, publishTime } | null
//   - HalcyonOracles.getEwma(symbol)                 → { sigmaAnn, lastUpdated, cached } | null
//
// Pages render immediately with `null` and re-render when data arrives.

(function () {
  const HERMES_BASE = 'https://hermes.pyth.network/api/latest_price_feeds';
  const BINANCE_KLINES = 'https://api.binance.com/api/v3/klines';
  const RAYDIUM_POOLS = 'https://api-v3.raydium.io/pools/info/ids';
  const POLL_MS = 5_000;
  const POOL_POLL_MS = 60_000; // Raydium APR refreshes slowly — 1/min is plenty
  const EWMA_TAU = 45;                         // days
  const EWMA_LAMBDA = Math.exp(-1 / EWMA_TAU); // ≈ 0.9780

  // Pyth mainnet feed IDs. Extend as we add products.
  const FEEDS = {
    SOL: 'ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d',
  };

  // Raydium mainnet pool IDs. The SOL/USDC Standard AMM V4 pool is the deepest
  // constant-product (x·y=k) SOL/USDC pool with a fungible LP mint — 0.25%
  // fee, ~$7M+ TVL. This is what the IL Protection product is priced against;
  // Raydium's Concentrated (CLMM) pools use position NFTs and have different
  // IL dynamics, so they are out of scope for v1. See lp_detection.js.
  const POOLS = {
    'RAYDIUM_SOL_USDC': '58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2',
  };

  // State + subscriber tables
  const spotState = {};
  const spotSubs  = {};
  const ewmaState = {};
  const ewmaSubs  = {};
  const poolState = {};   // { RAYDIUM_SOL_USDC: { dayApr, weekApr, monthApr, tvl, feeRate, price, source } }
  const poolSubs  = {};

  function notify(subs, sym, payload) {
    (subs[sym] || []).forEach(fn => { try { fn(payload); } catch (e) { console.error('[halcyon-oracles] subscriber threw', e); } });
  }
  function subscribe(subs, state, sym, fn) {
    (subs[sym] = subs[sym] || []).push(fn);
    if (state[sym]) fn(state[sym]);  // fire immediately with current value
    return () => {
      const arr = subs[sym] || [];
      const i = arr.indexOf(fn);
      if (i >= 0) arr.splice(i, 1);
    };
  }

  // ---------- Pyth Hermes spot ----------

  async function fetchSpot(sym) {
    const feedId = FEEDS[sym];
    if (!feedId) return;
    const url = `${HERMES_BASE}?ids[]=${feedId}`;
    try {
      const res = await fetch(url, { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const arr = await res.json();
      if (!Array.isArray(arr) || !arr[0]) throw new Error('empty response');
      const feed = arr[0];
      // Hermes returns price × 10^expo (expo is negative for crypto)
      const priceRaw = Number(feed.price.price);
      const expo     = Number(feed.price.expo);
      const price    = priceRaw * Math.pow(10, expo);
      const publishTime = Number(feed.price.publish_time);
      const payload = { price, publishTime, source: 'pyth-hermes' };
      spotState[sym] = payload;
      notify(spotSubs, sym, payload);
    } catch (e) {
      console.warn(`[halcyon-oracles] ${sym} spot fetch failed:`, e.message);
    }
  }

  function startSpotPoller(sym) {
    fetchSpot(sym);
    setInterval(() => fetchSpot(sym), POLL_MS);
  }

  // ---------- Binance EWMA45 ----------

  async function loadEwma(sym) {
    const today = new Date().toISOString().slice(0, 10); // YYYY-MM-DD (UTC)
    const cacheKey = `halcyon.ewma45.${sym}`;

    // Cache hit
    try {
      const raw = localStorage.getItem(cacheKey);
      if (raw) {
        const cached = JSON.parse(raw);
        if (cached.lastUpdated === today) {
          const payload = { ...cached, cached: true };
          ewmaState[sym] = payload;
          notify(ewmaSubs, sym, payload);
          return;
        }
      }
    } catch (e) { /* stale/corrupt cache — refetch */ }

    // Miss — fetch 46 daily closes (need 45 returns)
    const symbol = sym === 'SOL' ? 'SOLUSDT' : null;
    if (!symbol) return;
    const url = `${BINANCE_KLINES}?symbol=${symbol}&interval=1d&limit=46`;
    try {
      const res = await fetch(url, { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const klines = await res.json();
      if (!Array.isArray(klines) || klines.length < 46) throw new Error(`got ${klines.length} klines`);
      const closes = klines.map(k => parseFloat(k[4]));

      // 45 daily log returns
      const rets = [];
      for (let i = 1; i < closes.length; i++) {
        rets.push(Math.log(closes[i] / closes[i - 1]));
      }

      // Forward EWMA: variance_new = λ·variance_old + (1−λ)·r²
      // Seed from the first return so the filter warms smoothly.
      let variance = rets[0] * rets[0];
      for (let i = 1; i < rets.length; i++) {
        variance = EWMA_LAMBDA * variance + (1 - EWMA_LAMBDA) * rets[i] * rets[i];
      }
      const sigmaAnn = Math.sqrt(variance * 365);

      const payload = {
        sigmaAnn, lastUpdated: today, cached: false, source: 'binance-klines',
        sampleCount: rets.length, spotLatest: closes[closes.length - 1],
      };
      localStorage.setItem(cacheKey, JSON.stringify(payload));
      ewmaState[sym] = payload;
      notify(ewmaSubs, sym, payload);
    } catch (e) {
      console.warn(`[halcyon-oracles] ${sym} EWMA fetch failed:`, e.message);
    }
  }

  // ---------- Raydium pool APR ----------

  async function fetchPool(key) {
    const poolId = POOLS[key];
    if (!poolId) return;
    try {
      const res = await fetch(`${RAYDIUM_POOLS}?ids=${poolId}`, { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body = await res.json();
      const p = body.data && body.data[0];
      if (!p) throw new Error('pool not found in response');
      const payload = {
        poolId,
        tvl:       Number(p.tvl),
        feeRate:   Number(p.feeRate),    // decimal (0.0025 = 0.25%)
        price:     Number(p.price),
        dayApr:    Number(p.day.feeApr)   / 100, // API returns percent
        weekApr:   Number(p.week.feeApr)  / 100,
        monthApr:  Number(p.month.feeApr) / 100,
        dayVol:    Number(p.day.volume),
        source:    'raydium-v3',
        fetchedAt: Date.now(),
      };
      poolState[key] = payload;
      notify(poolSubs, key, payload);
    } catch (e) {
      console.warn(`[halcyon-oracles] pool ${key} fetch failed:`, e.message);
    }
  }

  function startPoolPoller(key) {
    fetchPool(key);
    setInterval(() => fetchPool(key), POOL_POLL_MS);
  }

  // ---------- Public API ----------

  window.HalcyonOracles = {
    subscribeSpot: (sym, fn) => subscribe(spotSubs, spotState, sym, fn),
    subscribeEwma: (sym, fn) => subscribe(ewmaSubs, ewmaState, sym, fn),
    subscribePool: (key, fn) => subscribe(poolSubs, poolState, key, fn),
    getSpot:       sym       => spotState[sym] || null,
    getEwma:       sym       => ewmaState[sym] || null,
    getPool:       key       => poolState[key] || null,
    // Diagnostics / forced refresh
    _refetchEwma:  sym       => { localStorage.removeItem(`halcyon.ewma45.${sym}`); return loadEwma(sym); },
    _refetchSpot:  sym       => fetchSpot(sym),
    _refetchPool:  key       => fetchPool(key),
  };

  // Kick off fetches. Add more symbols/pools here as products expand.
  startSpotPoller('SOL');
  loadEwma('SOL');
  startPoolPoller('RAYDIUM_SOL_USDC');
})();

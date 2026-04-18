/* global window */
// Halcyon — Raydium SOL/USDC LP position detection.
//
// ---------------------------------------------------------------------------
// Why this specific pool:
// This pool is the Raydium Standard AMM V4 (constant-product x·y=k). It has
// a fungible LP mint, which is why we can detect positions via SPL token
// balance. Raydium CLMM pools use position NFTs and would require different
// detection logic; they are out of scope for v1 because our IL pricer assumes
// constant-product math.
// ---------------------------------------------------------------------------
//
// Public API on window.HalcyonLP:
//   POOL                   — verified pool constants (id, lpMint, programId, tokens).
//   detectPosition(pubkey) — async; returns
//                              { hasPosition, lpAmount, underlyingSol,
//                                underlyingUsdc, valueUsdc, fetchedAt }
//                            on success, or
//                              { hasPosition: false, error? }
//                            otherwise.
//
// Pool state is read from window.HalcyonOracles.getPool('RAYDIUM_SOL_USDC')
// — oracles.js pre-fetches on page mount, so detection is usually one RPC
// call (the user's LP token accounts) once the wallet connects.

(function () {
  // TODO(production): swap to a project-owned Helius / QuickNode endpoint.
  // The public mainnet RPC rate-limits aggressively but is fine for demo
  // traffic.
  const RPC_URL = 'https://api.mainnet-beta.solana.com';

  const POOL = {
    id:        '58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2',
    lpMint:    '8HoQnePLqPj4M7PUDzfw8e3Ymdwgc7NLGnaTUapubyvu',
    programId: '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8', // Raydium Standard AMM V4
    tokenA: { mint: 'So11111111111111111111111111111111111111112', symbol: 'SOL',  decimals: 9 },
    tokenB: { mint: 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', symbol: 'USDC', decimals: 6 },
  };

  async function rpc(method, params) {
    const res = await fetch(RPC_URL, {
      method:  'POST',
      headers: { 'Content-Type': 'application/json' },
      body:    JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
    });
    if (!res.ok) throw new Error(`RPC HTTP ${res.status}`);
    const body = await res.json();
    if (body.error) throw new Error(body.error.message || 'RPC error');
    return body.result;
  }

  async function fetchPoolDirect() {
    const res = await fetch(
      `https://api-v3.raydium.io/pools/info/ids?ids=${POOL.id}`,
      { cache: 'no-store' }
    );
    if (!res.ok) throw new Error(`pool HTTP ${res.status}`);
    const body = await res.json();
    const p = body.data && body.data[0];
    if (!p) throw new Error('pool not found in Raydium response');
    return {
      mintAmountA: Number(p.mintAmountA),
      mintAmountB: Number(p.mintAmountB),
      lpAmount:    Number(p.lpAmount),
      price:       Number(p.price),
      tvl:         Number(p.tvl),
    };
  }

  async function detectPosition(walletPublicKey) {
    try {
      if (!walletPublicKey) return { hasPosition: false, error: 'no wallet pubkey' };
      const pubkey = typeof walletPublicKey === 'string'
        ? walletPublicKey
        : walletPublicKey.toBase58 ? walletPublicKey.toBase58() : String(walletPublicKey);

      // 1. All token accounts the wallet owns that hold the LP mint.
      //    A wallet may have multiple LP accounts for the same mint (e.g.
      //    post-farm unwrap). Sum them.
      const accounts = await rpc('getTokenAccountsByOwner', [
        pubkey,
        { mint: POOL.lpMint },
        { encoding: 'jsonParsed' },
      ]);
      let lpAmountRaw = 0n;
      for (const it of (accounts && accounts.value) || []) {
        const info = it.account.data.parsed.info.tokenAmount;
        lpAmountRaw += BigInt(info.amount);
      }
      if (lpAmountRaw === 0n) {
        return { hasPosition: false, lpAmount: 0n, valueUsdc: 0 };
      }

      // 2. Pool reserves + LP supply. Prefer the cached oracle state (already
      //    polling on 60s cadence); fall back to a direct Raydium API fetch
      //    if oracles haven't warmed up yet.
      let p = null;
      const cached = window.HalcyonOracles && window.HalcyonOracles.getPool('RAYDIUM_SOL_USDC');
      if (cached && cached.mintAmountA == null) {
        // oracles.js doesn't currently cache the reserves shape we need, so
        // reserves/supply come from the direct fetch. (cached still gives us
        // tvl and apr — we just need the additional fields here.)
      }
      try {
        p = await fetchPoolDirect();
      } catch (e) {
        // Last-resort: if the direct fetch fails and we have a cached pool
        // with a price, bail gracefully.
        if (!cached) throw e;
        return { hasPosition: false, error: `pool state unavailable: ${e.message}` };
      }

      const lpAmount     = Number(lpAmountRaw) / 1e9; // LP mint has 9 decimals
      if (p.lpAmount <= 0) throw new Error('pool reports zero LP supply');
      const share          = lpAmount / p.lpAmount;
      const underlyingSol  = share * p.mintAmountA;
      const underlyingUsdc = share * p.mintAmountB;

      // SOL price: prefer live Pyth spot (already polled by oracles.js).
      const pythSpot = window.HalcyonOracles && window.HalcyonOracles.getSpot('SOL');
      const solPrice = pythSpot && pythSpot.price ? pythSpot.price : p.price;

      const valueUsdc = underlyingUsdc + underlyingSol * solPrice;

      return {
        hasPosition: true,
        lpAmount, underlyingSol, underlyingUsdc, valueUsdc,
        solPrice, source: 'raydium-v3+rpc',
        fetchedAt: Date.now(),
      };
    } catch (e) {
      console.warn('[halcyon-lp] detectPosition failed:', e.message);
      return { hasPosition: false, error: e.message };
    }
  }

  window.HalcyonLP = { POOL, detectPosition, RPC_URL };
})();

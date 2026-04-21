/**
 * Halcyon — Raydium SOL/USDC LP position detection.
 *
 * Ported from `app/lp_detection.js`.
 *
 * Why this specific pool: it is the Raydium Standard AMM V4 (constant-product
 * x·y=k) with a fungible LP mint, so we can detect positions via SPL token
 * balance. Raydium CLMM pools use position NFTs and require different
 * detection logic; out of scope because our IL pricer assumes
 * constant-product math.
 *
 * LP positions live on mainnet only, regardless of which cluster the
 * user currently has selected for issuance. Detection always queries
 * mainnet.
 */

import { PublicKey } from "@solana/web3.js";

/**
 * Raydium's public v3 API endpoint. Used to read pool reserves + LP supply.
 * Same endpoint the Raydium UI uses; rate-limited but fine for demo traffic.
 */
const RAYDIUM_POOL_INFO_URL =
  "https://api-v3.raydium.io/pools/info/ids?ids=58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2";

/**
 * Mainnet RPC used for the token-account lookup. LP positions only exist on
 * mainnet; we detect against mainnet even when the user is configured for
 * devnet. Can be overridden with `NEXT_PUBLIC_LP_DETECTION_RPC`.
 */
const LP_DETECTION_RPC =
  (typeof process !== "undefined" && process.env.NEXT_PUBLIC_LP_DETECTION_RPC) ||
  "https://api.mainnet-beta.solana.com";

export const RAYDIUM_SOL_USDC_POOL = {
  id: "58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2",
  lpMint: "8HoQnePLqPj4M7PUDzfw8e3Ymdwgc7NLGnaTUapubyvu",
  programId: "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
  tokenA: { symbol: "SOL" as const, decimals: 9 },
  tokenB: { symbol: "USDC" as const, decimals: 6 },
};

export type LpDetectionSuccess = {
  hasPosition: true;
  lpAmount: number;
  underlyingSol: number;
  underlyingUsdc: number;
  valueUsdc: number;
  solPrice: number;
  fetchedAt: number;
};

export type LpDetectionMiss = {
  hasPosition: false;
  lpAmount: number;
  valueUsdc: number;
};

export type LpDetectionError = {
  hasPosition: false;
  error: string;
};

export type LpDetectionResult = LpDetectionSuccess | LpDetectionMiss | LpDetectionError;

async function rpc<T>(method: string, params: unknown[]): Promise<T> {
  const res = await fetch(LP_DETECTION_RPC, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  if (!res.ok) throw new Error(`RPC HTTP ${res.status}`);
  const body = (await res.json()) as { result?: T; error?: { message: string } };
  if (body.error) throw new Error(body.error.message || "RPC error");
  return body.result as T;
}

type RaydiumPoolInfo = {
  mintAmountA: number;
  mintAmountB: number;
  lpAmount: number;
  price: number;
  tvl: number;
};

async function fetchPoolReserves(): Promise<RaydiumPoolInfo> {
  const res = await fetch(RAYDIUM_POOL_INFO_URL, { cache: "no-store" });
  if (!res.ok) throw new Error(`pool HTTP ${res.status}`);
  const body = await res.json();
  const p = body?.data?.[0];
  if (!p) throw new Error("pool not found in Raydium response");
  return {
    mintAmountA: Number(p.mintAmountA),
    mintAmountB: Number(p.mintAmountB),
    lpAmount: Number(p.lpAmount),
    price: Number(p.price),
    tvl: Number(p.tvl),
  };
}

type ParsedTokenAccountsResponse = {
  value: Array<{
    account: {
      data: {
        parsed: {
          info: {
            tokenAmount: { amount: string; decimals: number };
          };
        };
      };
    };
  }>;
};

/**
 * Return the user's Raydium SOL/USDC LP position, if any, valued in USDC.
 *
 * On success returns `{ hasPosition: true, valueUsdc, underlyingSol, ... }`.
 * On no-position returns `{ hasPosition: false, lpAmount: 0, valueUsdc: 0 }`.
 * On RPC / Raydium-API failure returns `{ hasPosition: false, error }` —
 * callers surface the error without treating it as "no position" silently.
 */
export async function detectLpPosition(
  walletPubkey: PublicKey | string | null | undefined,
): Promise<LpDetectionResult> {
  try {
    if (!walletPubkey) {
      return { hasPosition: false, error: "wallet not connected" };
    }
    const pubkey =
      typeof walletPubkey === "string" ? walletPubkey : walletPubkey.toBase58();

    const accounts = await rpc<ParsedTokenAccountsResponse>(
      "getTokenAccountsByOwner",
      [pubkey, { mint: RAYDIUM_SOL_USDC_POOL.lpMint }, { encoding: "jsonParsed" }],
    );

    let lpAmountRaw = 0n;
    for (const item of accounts?.value ?? []) {
      const amount = item.account.data.parsed.info.tokenAmount.amount;
      lpAmountRaw += BigInt(amount);
    }
    if (lpAmountRaw === 0n) {
      return { hasPosition: false, lpAmount: 0, valueUsdc: 0 };
    }

    const pool = await fetchPoolReserves();
    if (pool.lpAmount <= 0) throw new Error("pool reports zero LP supply");

    // LP mint has 9 decimals on this specific Raydium pool.
    const lpAmount = Number(lpAmountRaw) / 1e9;
    const share = lpAmount / pool.lpAmount;
    const underlyingSol = share * pool.mintAmountA;
    const underlyingUsdc = share * pool.mintAmountB;

    // Raydium's `price` field is the SOL/USDC marginal price at the pool's
    // current reserve ratio — good enough for a notional estimate. In
    // production a Pyth SOL spot would be marginally more accurate but the
    // difference on a well-arb'd pool is <0.1%.
    const solPrice = pool.price;
    const valueUsdc = underlyingUsdc + underlyingSol * solPrice;

    return {
      hasPosition: true,
      lpAmount,
      underlyingSol,
      underlyingUsdc,
      valueUsdc,
      solPrice,
      fetchedAt: Date.now(),
    };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { hasPosition: false, error: message };
  }
}

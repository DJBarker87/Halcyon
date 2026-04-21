"use client";

import { useEffect, useState } from "react";
import { useWallet } from "@solana/wallet-adapter-react";

import {
  detectLpPosition,
  type LpDetectionResult,
} from "@/lib/lp-detection";

export type LpPositionStatus =
  | { kind: "disconnected" }
  | { kind: "loading" }
  | { kind: "detected"; data: Extract<LpDetectionResult, { hasPosition: true }> }
  | { kind: "none"; lpAmount: number }
  | { kind: "error"; error: string };

/**
 * Fetches the connected wallet's Raydium SOL/USDC LP position once the
 * wallet connects, and re-runs on explicit refresh. The detection uses a
 * mainnet RPC regardless of the selected cluster because LP positions
 * only exist on mainnet.
 *
 * Returns a discriminated union so the caller can render the four states
 * (disconnected, loading, detected, none, error) without prop-drilling
 * raw RPC errors.
 */
export function useLpPosition() {
  const { publicKey, connected } = useWallet();
  const [status, setStatus] = useState<LpPositionStatus>({
    kind: connected && publicKey ? "loading" : "disconnected",
  });
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    if (!connected || !publicKey) {
      setStatus({ kind: "disconnected" });
      return;
    }
    let cancelled = false;
    setStatus({ kind: "loading" });
    detectLpPosition(publicKey)
      .then((result) => {
        if (cancelled) return;
        if ("error" in result) {
          setStatus({ kind: "error", error: result.error });
          return;
        }
        if (result.hasPosition) {
          setStatus({ kind: "detected", data: result });
          return;
        }
        setStatus({ kind: "none", lpAmount: result.lpAmount });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = err instanceof Error ? err.message : String(err);
        setStatus({ kind: "error", error: message });
      });
    return () => {
      cancelled = true;
    };
  }, [connected, publicKey, nonce]);

  return { status, refresh: () => setNonce((n) => n + 1) };
}

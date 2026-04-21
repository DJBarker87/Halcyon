"use client";

import { useState } from "react";
import { AlertTriangle, Loader2 } from "lucide-react";

import { IssuancePage } from "@/components/issuance-page";
import { DetectedFlow } from "@/components/il-protection/detected-flow";
import { NoLpChoice } from "@/components/il-protection/no-lp-choice";
import { useLpPosition } from "@/hooks/use-lp-position";

/**
 * IL Protection — three-state UX driven by wallet + LP detection.
 *
 *   State A — connected, Raydium SOL/USDC LP detected → one-click insure
 *             (DetectedFlow pre-fills notional = LP value)
 *   State B — connected, no LP → choice between LP+Insure (soon) and
 *             Synthetic cover (NoLpChoice)
 *   State C — disconnected or user picked Synthetic → notional input flow
 *
 * LP detection always hits mainnet RPC regardless of the currently
 * selected cluster, because LP positions only exist on mainnet.
 */
export default function IlProtectionPage() {
  const { status, refresh } = useLpPosition();
  const [forceSynthetic, setForceSynthetic] = useState(false);

  if (forceSynthetic) {
    return (
      <div className="mx-auto max-w-5xl space-y-6">
        <button
          type="button"
          onClick={() => setForceSynthetic(false)}
          className="text-sm font-medium text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
        >
          ← Back to LP detection
        </button>
        <IssuancePage kind="ilProtection" />
      </div>
    );
  }

  if (status.kind === "loading") {
    return (
      <div className="mx-auto flex max-w-4xl items-center justify-center rounded-md border border-border bg-card p-16">
        <div className="flex items-center gap-3 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          Checking your wallet for a Raydium SOL/USDC LP position…
        </div>
      </div>
    );
  }

  if (status.kind === "detected") {
    return (
      <DetectedFlow
        data={status.data}
        onRefresh={refresh}
        onPickSynthetic={() => setForceSynthetic(true)}
      />
    );
  }

  if (status.kind === "error") {
    return (
      <div className="mx-auto max-w-4xl space-y-6">
        <div className="rounded-md border border-warning-700/30 bg-warning-50 p-4">
          <div className="flex items-start gap-3">
            <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-warning-700" aria-hidden="true" />
            <div>
              <h2 className="text-sm font-semibold text-ink">
                Couldn't check for an LP position.
              </h2>
              <p className="mt-1 text-sm leading-6 text-muted-foreground">
                {status.error}. You can still buy a synthetic cover without an
                LP token.
              </p>
            </div>
          </div>
        </div>
        <NoLpChoice walletConnected onPickSynthetic={() => setForceSynthetic(true)} />
      </div>
    );
  }

  // disconnected OR connected-with-no-LP
  return (
    <NoLpChoice
      walletConnected={status.kind === "none"}
      onPickSynthetic={() => setForceSynthetic(true)}
    />
  );
}

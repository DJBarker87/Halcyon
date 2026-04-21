"use client";

import { useEffect } from "react";
import { AlertTriangle } from "lucide-react";

import { findAllowedConfig } from "@/lib/allowed-configs";
import { useRuntimeConfig } from "@/lib/runtime-config";

/**
 * Explicit re-consent modal for cluster changes (audit F3).
 *
 * A one-click cluster switch from mainnet to devnet (or vice versa)
 * silently changes the RPC endpoint, program IDs, and oracle accounts
 * the wallet signs against. The prompt is deliberate friction — the
 * user acknowledges they understand the implications before the switch
 * takes effect.
 */
export function ClusterSwitchModal() {
  const {
    cluster,
    pendingClusterChange,
    confirmClusterChange,
    cancelClusterChange,
  } = useRuntimeConfig();

  useEffect(() => {
    if (!pendingClusterChange) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") cancelClusterChange();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [pendingClusterChange, cancelClusterChange]);

  if (!pendingClusterChange) return null;

  const from = findAllowedConfig(cluster);
  const to = findAllowedConfig(pendingClusterChange);
  if (!to) {
    cancelClusterChange();
    return null;
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Confirm cluster change"
      className="fixed inset-0 z-[60] flex items-center justify-center bg-ink/40 p-4 backdrop-blur-sm"
    >
      <div className="w-full max-w-[480px] rounded-lg border border-border bg-card shadow-2xl">
        <div className="flex items-start gap-3 border-b border-border px-5 py-4">
          <AlertTriangle className="mt-0.5 h-5 w-5 text-warning-700" aria-hidden="true" />
          <div>
            <h2 className="text-base font-semibold text-foreground">
              Switch to {to.label}?
            </h2>
            <p className="mt-1 text-sm text-muted-foreground">
              This changes every RPC endpoint, program ID, and oracle account the wallet signs against.
            </p>
          </div>
        </div>
        <div className="space-y-3 px-5 py-4 text-sm text-foreground">
          <div>
            <span className="text-muted-foreground">From: </span>
            <strong>{from?.label ?? cluster}</strong>
          </div>
          <div>
            <span className="text-muted-foreground">To: </span>
            <strong>{to.label}</strong>
          </div>
          <p className="leading-6 text-muted-foreground">{to.description}</p>
        </div>
        <div className="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
          <button
            type="button"
            onClick={cancelClusterChange}
            className="inline-flex min-h-10 items-center rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={confirmClusterChange}
            className="inline-flex min-h-10 items-center rounded-md bg-primary px-3 text-sm font-semibold text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            Switch to {to.label}
          </button>
        </div>
      </div>
    </div>
  );
}

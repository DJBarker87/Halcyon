"use client";

import { useEffect } from "react";
import { CheckCircle2, ShieldCheck, X } from "lucide-react";

import { READ_ONLY_DISPLAY_FIELDS } from "@/lib/allowed-configs";
import { cn } from "@/lib/format";
import { useRuntimeConfig } from "@/lib/runtime-config";
import type { ClusterId } from "@/lib/types";

interface SettingsPanelProps {
  open: boolean;
  onClose: () => void;
}

export function SettingsPanel({ open, onClose }: SettingsPanelProps) {
  const {
    cluster,
    current,
    allowedConfigs,
    requestClusterChange,
    genesisCheck,
  } = useRuntimeConfig();

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50">
      <button
        type="button"
        aria-label="Close runtime config"
        onClick={onClose}
        className="absolute inset-0 bg-ink/40 backdrop-blur-sm"
      />

      <aside
        role="dialog"
        aria-modal="true"
        aria-label="Runtime configuration"
        className="absolute inset-y-0 right-0 flex w-full max-w-[560px] flex-col border-l border-border bg-card shadow-2xl"
      >
        <div className="flex items-start justify-between gap-4 border-b border-border px-5 py-5">
          <div>
            <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
              Runtime Config
            </div>
            <h2 className="mt-1 text-xl font-semibold text-foreground">Cluster selection</h2>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              Program IDs and oracle accounts are pinned to the build and cannot be changed
              in-browser. Switching cluster loads the allowlisted wiring for that target.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="inline-flex min-h-10 min-w-10 items-center justify-center rounded-md border border-border bg-background text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>

        <div className="overflow-y-auto px-5 py-5">
          <section className="space-y-3">
            <h3 className="text-sm font-semibold text-foreground">Cluster</h3>
            <div
              role="radiogroup"
              aria-label="Cluster"
              className="flex flex-wrap gap-2"
            >
              {allowedConfigs.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  role="radio"
                  aria-checked={cluster === entry.id}
                  onClick={() => requestClusterChange(entry.id as ClusterId)}
                  className={cn(
                    "inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                    cluster === entry.id
                      ? "border-primary/30 bg-primary/10 text-foreground"
                      : "border-border bg-background text-muted-foreground hover:bg-secondary hover:text-foreground",
                  )}
                >
                  {entry.label}
                </button>
              ))}
            </div>
          </section>

          <section className="mt-6 space-y-3 border-t border-border pt-6">
            <h3 className="text-sm font-semibold text-foreground">Cluster integrity</h3>
            <div
              role="status"
              className={cn(
                "rounded-md border p-3 text-sm",
                genesisCheck.status === "ok" &&
                  "border-success-700/30 bg-success-50 text-success-700",
                genesisCheck.status === "error" &&
                  "border-error-500/40 bg-error-50 text-error-700",
                genesisCheck.status === "pending" &&
                  "border-border bg-n-50 text-muted-foreground",
                genesisCheck.status === "skipped" &&
                  "border-border bg-n-50 text-muted-foreground",
              )}
            >
              {genesisCheck.status === "ok" && (
                <span className="inline-flex items-center gap-2">
                  <CheckCircle2 className="h-4 w-4 text-success-700" aria-hidden="true" />
                  RPC genesis matches the {current.id} cluster.
                </span>
              )}
              {genesisCheck.status === "pending" && "Verifying RPC cluster…"}
              {genesisCheck.status === "skipped" && `Genesis check skipped (${genesisCheck.reason}).`}
              {genesisCheck.status === "error" && (
                <span className="inline-flex items-start gap-2">
                  <ShieldCheck className="mt-0.5 h-4 w-4 text-error-700" aria-hidden="true" />
                  {genesisCheck.reason}
                </span>
              )}
            </div>
          </section>

          <section className="mt-6 space-y-3 border-t border-border pt-6">
            <h3 className="text-sm font-semibold text-foreground">Pinned wiring</h3>
            <p className="text-sm leading-6 text-muted-foreground">
              All values below are baked into this build. To change them, rebuild the frontend
              with different <code className="rounded bg-secondary px-1">NEXT_PUBLIC_*</code> env
              values.
            </p>
            <dl className="grid grid-cols-1 gap-3">
              {READ_ONLY_DISPLAY_FIELDS.map(({ key, label }) => {
                const value = current[key];
                return (
                  <div key={key} className="space-y-1">
                    <dt className="text-xs uppercase tracking-[0.1em] text-muted-foreground">
                      {label}
                    </dt>
                    <dd className="break-all rounded-md border border-border bg-background px-3 py-2 font-mono text-[12px] text-foreground">
                      {value ? value : <span className="text-muted-foreground">not set</span>}
                    </dd>
                  </div>
                );
              })}
            </dl>
          </section>
        </div>
      </aside>
    </div>
  );
}

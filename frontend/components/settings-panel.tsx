"use client";

import { useEffect } from "react";
import { RotateCcw, X } from "lucide-react";

import { cn } from "@/lib/format";
import { useRuntimeConfig } from "@/lib/runtime-config";
import { isFieldLocked as fieldLockedForCluster } from "@/lib/runtime-config-schema";
import type { ClusterConfig, ClusterId } from "@/lib/types";

interface SettingsPanelProps {
  open: boolean;
  onClose: () => void;
}

const CLUSTERS: ClusterId[] = ["localnet", "devnet", "mainnet"];

const GENERAL_FIELDS: Array<{ key: keyof ClusterConfig; label: string; placeholder?: string }> = [
  { key: "rpcUrl", label: "RPC URL", placeholder: "https://api.devnet.solana.com" },
  { key: "kernelProgramId", label: "Kernel program" },
];

const PRODUCT_FIELDS: Array<{
  title: string;
  fields: Array<{ key: keyof ClusterConfig; label: string }>;
}> = [
  {
    title: "Programs",
    fields: [
      { key: "flagshipProgramId", label: "Flagship program" },
      { key: "ilProtectionProgramId", label: "IL Protection program" },
      { key: "solAutocallProgramId", label: "SOL Autocall program" },
    ],
  },
  {
    title: "Pyth / oracle accounts",
    fields: [
      { key: "pythSpy", label: "SPY price account" },
      { key: "pythQqq", label: "QQQ price account" },
      { key: "pythIwm", label: "IWM price account" },
      { key: "pythSol", label: "SOL price account" },
      { key: "pythUsdc", label: "USDC price account" },
    ],
  },
];

function ConfigField({
  label,
  value,
  placeholder,
  disabled = false,
  onChange,
}: {
  label: string;
  value: string;
  placeholder?: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const id = `config-${label.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  return (
    <div className="space-y-2">
      <label htmlFor={id} className="field-label">
        {label}
      </label>
      <input
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        disabled={disabled}
        className={cn(
          "field font-mono text-[12px]",
          disabled && "cursor-not-allowed opacity-60",
        )}
      />
    </div>
  );
}

export function SettingsPanel({ open, onClose }: SettingsPanelProps) {
  const { cluster, setCluster, current, currentHasOverrides, updateCurrent, resetCurrent } =
    useRuntimeConfig();

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
        className="absolute inset-0 bg-background/80 backdrop-blur-sm"
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
            <h2 className="mt-1 text-xl font-semibold text-foreground">Cluster and account wiring</h2>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              Browser-local overrides stay visible in the shell. Mainnet only allows an RPC override here;
              program IDs and oracle accounts stay pinned to the environment.
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
          <div className="flex flex-wrap gap-2">
            {CLUSTERS.map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => setCluster(value)}
                className={cn(
                  "inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                  cluster === value
                    ? "border-primary/30 bg-primary/10 text-foreground"
                    : "border-border bg-background text-muted-foreground hover:bg-secondary hover:text-foreground",
                )}
              >
                {value}
              </button>
            ))}

            <button
              type="button"
              onClick={resetCurrent}
              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              <RotateCcw className="h-4 w-4" aria-hidden="true" />
              Reset {cluster}
            </button>
          </div>

          <section className="mt-6 space-y-4 border-b border-border pb-6">
            {currentHasOverrides && (
              <div className="rounded-md border border-amber-400/30 bg-amber-400/10 p-4 text-sm text-foreground">
                Browser-local overrides are active for this cluster. Review them before signing.
              </div>
            )}

            {cluster === "mainnet" && (
              <div className="rounded-md border border-border bg-background/70 p-4 text-sm text-muted-foreground">
                Mainnet program and oracle IDs are locked to environment values. Only the RPC URL can be
                overridden in-browser.
              </div>
            )}

            <div>
              <h3 className="text-sm font-semibold text-foreground">Core</h3>
              <p className="mt-1 text-sm leading-6 text-muted-foreground">
                The RPC and kernel are required on every page. Product pages add their own program and feed
                requirements.
              </p>
            </div>

            {GENERAL_FIELDS.map((field) => (
              <ConfigField
                key={field.key}
                label={field.label}
                value={current[field.key]}
                placeholder={field.placeholder}
                disabled={fieldLockedForCluster(cluster, field.key)}
                onChange={(value) => updateCurrent(field.key, value)}
              />
            ))}
          </section>

          {PRODUCT_FIELDS.map((group) => (
            <section key={group.title} className="space-y-4 border-b border-border py-6 last:border-b-0 last:pb-0">
              <h3 className="text-sm font-semibold text-foreground">{group.title}</h3>
              {group.fields.map((field) => (
                <ConfigField
                  key={field.key}
                  label={field.label}
                  value={current[field.key]}
                  disabled={fieldLockedForCluster(cluster, field.key)}
                  onChange={(value) => updateCurrent(field.key, value)}
                />
              ))}
            </section>
          ))}
        </div>
      </aside>
    </div>
  );
}

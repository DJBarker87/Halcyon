"use client";

import Image from "next/image";
import { useEffect, useMemo, useState } from "react";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { AlertCircle, RefreshCcw, Settings2, Wallet } from "lucide-react";

import { fetchPortfolio, type PortfolioEntry } from "@/lib/halcyon";
import { cn, formatUsdcBaseUnits, shortAddress } from "@/lib/format";
import { openRuntimeConfigPanel } from "@/lib/runtime-panel";
import { useRuntimeConfig } from "@/lib/runtime-config";
import type { ProductKind } from "@/lib/types";

type Filter = "all" | "active" | ProductKind;

const FILTERS: Array<{ value: Filter; label: string }> = [
  { value: "all", label: "All" },
  { value: "active", label: "Active" },
  { value: "flagship", label: "Flagship" },
  { value: "solAutocall", label: "SOL" },
  { value: "ilProtection", label: "IL" },
];

function formatTimestamp(value: number) {
  if (!value) return "Not set";
  return new Date(value * 1000).toLocaleString();
}

function productLabel(kind: ProductKind) {
  if (kind === "flagship") return "Flagship";
  if (kind === "solAutocall") return "SOL Autocall";
  return "IL Protection";
}

function statusTone(status: string) {
  const value = status.toLowerCase();
  if (value === "active" || value === "observed")
    return "border-halcyonBlue-300 bg-halcyonBlue-50 text-halcyonBlue-700";
  if (value === "settled" || value === "autocalled") {
    return "border-success-700/30 bg-success-50 text-success-700";
  }
  return "border-border bg-n-50 text-muted-foreground";
}

function missingPortfolioConfig(current: ReturnType<typeof useRuntimeConfig>["current"]) {
  const missing: string[] = [];
  if (!current.rpcUrl.trim()) missing.push("RPC URL");
  if (!current.kernelProgramId.trim()) missing.push("Kernel program");
  if (!current.flagshipProgramId.trim()) missing.push("Flagship program");
  if (!current.solAutocallProgramId.trim()) missing.push("SOL Autocall program");
  if (!current.ilProtectionProgramId.trim()) missing.push("IL Protection program");
  return missing;
}

export function PortfolioView() {
  const { connection } = useConnection();
  const { publicKey, connected } = useWallet();
  const { current } = useRuntimeConfig();

  const [entries, setEntries] = useState<PortfolioEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("all");

  const missing = useMemo(() => missingPortfolioConfig(current), [current]);

  async function load() {
    if (!publicKey) return;
    setLoading(true);
    setError(null);
    try {
      const result = await fetchPortfolio(connection, current, publicKey);
      setEntries(result);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Portfolio load failed");
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!connected || !publicKey || missing.length > 0) {
      setEntries([]);
      return;
    }
    void load();
  }, [connected, publicKey, current, connection, missing.length]);

  const filtered = useMemo(() => {
    return entries.filter((entry) => {
      if (filter === "all") return true;
      if (filter === "active") return entry.status.toLowerCase() === "active";
      return entry.productKind === filter;
    });
  }, [entries, filter]);

  const stats = useMemo(() => {
    const active = entries.filter((entry) => entry.status.toLowerCase() === "active");
    return {
      activePolicies: active.length,
      activeNotional: active.reduce((sum, entry) => sum + entry.notional, 0),
      premiumPaid: active.reduce((sum, entry) => sum + entry.premiumPaid, 0),
      maxLiability: active.reduce((sum, entry) => sum + entry.maxLiability, 0),
    };
  }, [entries]);

  return (
    <div className="space-y-8">
      <section className="relative overflow-hidden rounded-md border border-border">
        <Image
          src="https://images.unsplash.com/photo-1518186285589-2f7649de83e0?auto=format&fit=crop&w=1400&q=80"
          alt="Portfolio overview dashboard"
          width={1600}
          height={700}
          className="h-[240px] w-full object-cover sm:h-[280px]"
          priority
        />
        <div className="absolute inset-0 bg-gradient-to-r from-background via-background/90 to-background/40" />
        <div className="absolute inset-0 flex items-end px-5 py-5 sm:px-8 sm:py-8">
          <div className="max-w-3xl">
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              Portfolio
            </div>
            <h1 className="mt-2 text-3xl font-semibold tracking-tight text-foreground sm:text-4xl">
              Wallet policies across every product
            </h1>
            <p className="mt-4 text-sm leading-6 text-foreground/90 sm:text-base">
              This page reads `PolicyHeader` accounts directly from the kernel program with the same memcmp
              filters described in the build doc.
            </p>
          </div>
        </div>
      </section>

      {!connected && (
        <section className="surface p-6">
          <div className="flex items-start gap-3">
            <Wallet className="mt-0.5 h-5 w-5 text-muted-foreground" aria-hidden="true" />
            <div>
              <h2 className="text-lg font-semibold text-foreground">Connect a wallet</h2>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">
                Portfolio queries are scoped to the connected owner pubkey.
              </p>
            </div>
          </div>
        </section>
      )}

      {connected && missing.length > 0 && (
        <section className="surface p-6">
          <div className="flex items-start gap-3">
            <AlertCircle className="mt-0.5 h-5 w-5 text-destructive" aria-hidden="true" />
            <div>
              <h2 className="text-lg font-semibold text-foreground">Portfolio config is incomplete</h2>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">
                Add the remaining runtime values before reading live policy accounts.
              </p>
              <ul className="mt-3 space-y-1 text-sm text-foreground">
                {missing.map((item) => (
                  <li key={item}>• {item}</li>
                ))}
              </ul>
              <button
                type="button"
                onClick={openRuntimeConfigPanel}
                className="mt-4 inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                <Settings2 className="h-4 w-4" aria-hidden="true" />
                Open runtime config
              </button>
            </div>
          </div>
        </section>
      )}

      {connected && missing.length === 0 && (
        <>
          <section className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
            <div className="surface p-5">
              <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                Active policies
              </div>
              <div className="mt-3 text-3xl font-semibold text-foreground">{stats.activePolicies}</div>
            </div>
            <div className="surface p-5">
              <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                Active notional
              </div>
              <div className="mt-3 text-3xl font-semibold text-foreground">
                {formatUsdcBaseUnits(stats.activeNotional)}
              </div>
            </div>
            <div className="surface p-5">
              <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                Premium paid
              </div>
              <div className="mt-3 text-3xl font-semibold text-foreground">
                {formatUsdcBaseUnits(stats.premiumPaid)}
              </div>
            </div>
            <div className="surface p-5">
              <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                Max liability
              </div>
              <div className="mt-3 text-3xl font-semibold text-foreground">
                {formatUsdcBaseUnits(stats.maxLiability)}
              </div>
            </div>
          </section>

          <section className="surface p-5 sm:p-6">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div>
                <h2 className="text-xl font-semibold text-foreground">Policies</h2>
                <p className="mt-2 text-sm leading-6 text-muted-foreground">
                  Owner {publicKey ? shortAddress(publicKey, 6) : "Not connected"}
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                {FILTERS.map((item) => (
                  <button
                    key={item.value}
                    type="button"
                    onClick={() => setFilter(item.value)}
                    className={cn(
                      "inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                      filter === item.value
                        ? "border-primary/30 bg-primary/10 text-foreground"
                        : "border-border bg-background text-muted-foreground hover:bg-secondary hover:text-foreground",
                    )}
                  >
                    {item.label}
                  </button>
                ))}

                <button
                  type="button"
                  onClick={load}
                  className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                >
                  <RefreshCcw className={cn("h-4 w-4", loading && "motion-safe:animate-spin")} aria-hidden="true" />
                  Refresh
                </button>
              </div>
            </div>

            {loading && entries.length === 0 && (
              <div className="mt-6 grid gap-3">
                {Array.from({ length: 4 }).map((_, index) => (
                  <div
                    key={index}
                    className="h-24 rounded-md border border-border bg-card motion-safe:animate-pulse"
                  />
                ))}
              </div>
            )}

            {error && (
              <div className="mt-6 rounded-md border border-destructive/30 bg-destructive/10 p-4">
                <div className="text-sm font-medium text-foreground">Load failed</div>
                <p className="mt-2 text-sm leading-6 text-muted-foreground">{error}</p>
              </div>
            )}

            {!loading && !error && filtered.length === 0 && (
              <div className="mt-6 rounded-md border border-border bg-card p-5">
                <div className="text-lg font-semibold text-foreground">No policies found</div>
                <p className="mt-2 text-sm leading-6 text-muted-foreground">
                  Issue a product from the new frontend pages, then refresh this wallet view.
                </p>
              </div>
            )}

            {filtered.length > 0 && (
              <div className="mt-6 overflow-x-auto rounded-md border border-border">
                <table className="min-w-full divide-y divide-border text-left text-sm">
                  <thead className="bg-n-50 text-muted-foreground">
                    <tr>
                      <th className="px-4 py-3 font-medium">Policy</th>
                      <th className="px-4 py-3 font-medium">Product</th>
                      <th className="px-4 py-3 font-medium">Status</th>
                      <th className="px-4 py-3 font-medium">Notional</th>
                      <th className="px-4 py-3 font-medium">Premium</th>
                      <th className="px-4 py-3 font-medium">Max liability</th>
                      <th className="px-4 py-3 font-medium">Issued</th>
                      <th className="px-4 py-3 font-medium">Expiry</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border bg-card">
                    {filtered.map((entry) => (
                      <tr key={entry.policyAddress} className="align-top">
                        <td className="px-4 py-4">
                          <div className="font-mono text-[12px] text-foreground">
                            {shortAddress(entry.policyAddress, 6)}
                          </div>
                          <div className="mt-2 text-xs text-muted-foreground">
                            Terms {shortAddress(entry.productTermsAddress, 6)}
                          </div>
                        </td>
                        <td className="px-4 py-4">
                          <div className="font-medium text-foreground">{productLabel(entry.productKind)}</div>
                          <div className="mt-2 text-xs leading-5 text-muted-foreground">
                            {Object.entries(entry.details)
                              .map(([key, value]) => `${key}: ${value}`)
                              .join(" · ")}
                          </div>
                        </td>
                        <td className="px-4 py-4">
                          <span
                            className={cn(
                              "inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium",
                              statusTone(entry.status),
                            )}
                          >
                            {entry.status}
                          </span>
                        </td>
                        <td className="px-4 py-4 font-medium text-foreground">
                          {formatUsdcBaseUnits(entry.notional)}
                        </td>
                        <td className="px-4 py-4 text-foreground">{formatUsdcBaseUnits(entry.premiumPaid)}</td>
                        <td className="px-4 py-4 text-foreground">{formatUsdcBaseUnits(entry.maxLiability)}</td>
                        <td className="px-4 py-4 text-muted-foreground">{formatTimestamp(entry.issuedAt)}</td>
                        <td className="px-4 py-4 text-muted-foreground">{formatTimestamp(entry.expiryTs)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
}

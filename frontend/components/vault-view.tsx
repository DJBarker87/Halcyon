"use client";

import Image from "next/image";
import { useEffect, useMemo, useState } from "react";
import { useConnection } from "@solana/wallet-adapter-react";
import { AlertCircle, RefreshCcw, Settings2 } from "lucide-react";

import { fetchVaultOverview, type VaultOverview } from "@/lib/halcyon";
import { cn, field, formatPercentFromBpsS6, formatUsdcBaseUnits, shortAddress, toNumber } from "@/lib/format";
import { openRuntimeConfigPanel } from "@/lib/runtime-panel";
import { useRuntimeConfig } from "@/lib/runtime-config";
import type { ProductKind } from "@/lib/types";

function productLabel(kind: ProductKind) {
  if (kind === "flagship") return "Flagship";
  if (kind === "solAutocall") return "SOL Autocall";
  return "IL Protection";
}

function missingVaultConfig(current: ReturnType<typeof useRuntimeConfig>["current"]) {
  const missing: string[] = [];
  if (!current.rpcUrl.trim()) missing.push("RPC URL");
  if (!current.kernelProgramId.trim()) missing.push("Kernel program");
  if (!current.flagshipProgramId.trim()) missing.push("Flagship program");
  if (!current.solAutocallProgramId.trim()) missing.push("SOL Autocall program");
  if (!current.ilProtectionProgramId.trim()) missing.push("IL Protection program");
  return missing;
}

function formatUnix(value: unknown) {
  const numeric = toNumber(value);
  if (!numeric) return "Not set";
  return new Date(numeric * 1000).toLocaleString();
}

export function VaultView() {
  const { connection } = useConnection();
  const { current, cluster } = useRuntimeConfig();

  const [overview, setOverview] = useState<VaultOverview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const missing = useMemo(() => missingVaultConfig(current), [current]);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const result = await fetchVaultOverview(connection, current);
      setOverview(result);
    } catch (cause) {
      setOverview(null);
      setError(cause instanceof Error ? cause.message : "Vault load failed");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (missing.length > 0) {
      setOverview(null);
      return;
    }
    void load();
  }, [connection, current, missing.length]);

  const vaultState = overview?.vaultState ?? null;
  const protocolConfig = overview?.protocolConfig ?? null;

  return (
    <div className="space-y-8">
      <section className="relative overflow-hidden rounded-md border border-border">
        <Image
          src="https://images.unsplash.com/photo-1520607162513-77705c0f0d4a?auto=format&fit=crop&w=1400&q=80"
          alt="Capital vault and market structure"
          width={1600}
          height={700}
          className="h-[240px] w-full object-cover sm:h-[280px]"
          priority
        />
        <div className="absolute inset-0 bg-gradient-to-r from-background via-background/88 to-background/35" />
        <div className="absolute inset-0 flex items-end px-5 py-5 sm:px-8 sm:py-8">
          <div className="max-w-3xl">
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              Vault
            </div>
            <h1 className="mt-2 text-3xl font-semibold tracking-tight text-foreground sm:text-4xl">
              Shared kernel capital state
            </h1>
            <p className="mt-4 text-sm leading-6 text-foreground/90 sm:text-base">
              Read-only kernel overview: singleton state, fee ledger, keeper registry, and per-product reserve
              footprints for the current cluster.
            </p>
          </div>
        </div>
      </section>

      {missing.length > 0 && (
        <section className="surface p-6">
          <div className="flex items-start gap-3">
            <AlertCircle className="mt-0.5 h-5 w-5 text-destructive" aria-hidden="true" />
            <div>
              <h2 className="text-lg font-semibold text-foreground">Vault config is incomplete</h2>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">
                The read-only kernel view still needs the following runtime values.
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

      {missing.length === 0 && (
        <>
          <div className="flex justify-end">
            <button
              type="button"
              onClick={load}
              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              <RefreshCcw className={cn("h-4 w-4", loading && "motion-safe:animate-spin")} aria-hidden="true" />
              Refresh
            </button>
          </div>

          {loading && !overview && (
            <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
              {Array.from({ length: 4 }).map((_, index) => (
                <div
                  key={index}
                  className="h-28 rounded-md border border-border bg-background/70 motion-safe:animate-pulse"
                />
              ))}
            </div>
          )}

          {error && (
            <section className="surface p-6">
              <h2 className="text-lg font-semibold text-foreground">Load failed</h2>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">{error}</p>
            </section>
          )}

          {overview && (
            <>
              <section className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                <div className="surface p-5">
                  <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                    Senior capital
                  </div>
                  <div className="mt-3 text-3xl font-semibold text-foreground">
                    {formatUsdcBaseUnits(field(vaultState ?? {}, "totalSenior"))}
                  </div>
                </div>
                <div className="surface p-5">
                  <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                    Junior capital
                  </div>
                  <div className="mt-3 text-3xl font-semibold text-foreground">
                    {formatUsdcBaseUnits(field(vaultState ?? {}, "totalJunior"))}
                  </div>
                </div>
                <div className="surface p-5">
                  <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                    Reserved liability
                  </div>
                  <div className="mt-3 text-3xl font-semibold text-foreground">
                    {formatUsdcBaseUnits(field(vaultState ?? {}, "totalReservedLiability"))}
                  </div>
                </div>
                <div className="surface p-5">
                  <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                    Treasury balance
                  </div>
                  <div className="mt-3 text-3xl font-semibold text-foreground">
                    {formatUsdcBaseUnits(field(overview.feeLedger ?? {}, "treasuryBalance"))}
                  </div>
                </div>
              </section>

              <div className="grid gap-6 xl:grid-cols-[minmax(0,1.2fr)_360px]">
                <section className="surface p-5 sm:p-6">
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div>
                      <h2 className="text-xl font-semibold text-foreground">Product reserve map</h2>
                      <p className="mt-2 text-sm leading-6 text-muted-foreground">
                        Cluster {cluster} · kernel {current.kernelProgramId ? shortAddress(current.kernelProgramId, 6) : "Not set"}
                      </p>
                    </div>
                  </div>

                  <div className="mt-6 overflow-x-auto rounded-md border border-border">
                    <table className="min-w-full divide-y divide-border text-left text-sm">
                      <thead className="bg-background/80 text-muted-foreground">
                        <tr>
                          <th className="px-4 py-3 font-medium">Product</th>
                          <th className="px-4 py-3 font-medium">Registry active</th>
                          <th className="px-4 py-3 font-medium">Per-policy cap</th>
                          <th className="px-4 py-3 font-medium">Global cap</th>
                          <th className="px-4 py-3 font-medium">Reserved</th>
                          <th className="px-4 py-3 font-medium">Coupon vault</th>
                          <th className="px-4 py-3 font-medium">Hedge sleeve</th>
                          <th className="px-4 py-3 font-medium">Policies</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-border bg-card/30">
                        {overview.productSummaries.map((summary) => (
                          <tr key={summary.kind}>
                            <td className="px-4 py-4 font-medium text-foreground">{productLabel(summary.kind)}</td>
                            <td className="px-4 py-4 text-foreground">
                              {field(summary.registry, "active") ? "Yes" : "No"}
                            </td>
                            <td className="px-4 py-4 text-foreground">
                              {formatUsdcBaseUnits(field(summary.registry, "perPolicyRiskCap"))}
                            </td>
                            <td className="px-4 py-4 text-foreground">
                              {formatUsdcBaseUnits(field(summary.registry, "globalRiskCap"))}
                            </td>
                            <td className="px-4 py-4 text-foreground">
                              {formatUsdcBaseUnits(field(summary.registry, "totalReserved"))}
                            </td>
                            <td className="px-4 py-4 text-foreground">
                              {summary.couponVaultBalance === null
                                ? "Not initialized"
                                : formatUsdcBaseUnits(summary.couponVaultBalance)}
                            </td>
                            <td className="px-4 py-4 text-foreground">
                              {summary.hedgeReserve === null
                                ? "Not initialized"
                                : formatUsdcBaseUnits(summary.hedgeReserve)}
                            </td>
                            <td className="px-4 py-4 text-foreground">
                              {summary.activePolicyCount} active / {summary.settledPolicyCount} settled
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </section>

                <aside className="space-y-6">
                  <section className="surface p-5">
                    <h2 className="text-lg font-semibold text-foreground">Protocol config</h2>
                    <dl className="mt-4 space-y-4 text-sm">
                      <div>
                        <dt className="text-muted-foreground">Utilization cap</dt>
                        <dd className="mt-1 text-foreground">
                          {formatPercentFromBpsS6(toNumber(field(protocolConfig ?? {}, "utilizationCapBps")) * 100)}
                        </dd>
                      </div>
                      <div>
                        <dt className="text-muted-foreground">Quote TTL</dt>
                        <dd className="mt-1 text-foreground">{toNumber(field(protocolConfig ?? {}, "quoteTtlSecs"))} secs</dd>
                      </div>
                      <div>
                        <dt className="text-muted-foreground">Senior cooldown</dt>
                        <dd className="mt-1 text-foreground">
                          {toNumber(field(protocolConfig ?? {}, "seniorCooldownSecs"))} secs
                        </dd>
                      </div>
                      <div>
                        <dt className="text-muted-foreground">Last update</dt>
                        <dd className="mt-1 text-foreground">{formatUnix(field(protocolConfig ?? {}, "lastUpdateTs"))}</dd>
                      </div>
                    </dl>
                  </section>

                  <section className="surface p-5">
                    <h2 className="text-lg font-semibold text-foreground">Keeper state</h2>
                    <div className="mt-4 space-y-3 text-sm text-muted-foreground">
                      <div className="rounded-md border border-border bg-background/70 p-3">
                        Keeper registry {overview.keeperRegistry ? "present" : "not initialized"}
                      </div>
                      <div className="rounded-md border border-border bg-background/70 p-3">
                        Lifetime premium received {formatUsdcBaseUnits(field(vaultState ?? {}, "lifetimePremiumReceived"))}
                      </div>
                      <div className="rounded-md border border-border bg-background/70 p-3">
                        Vault last update {formatUnix(field(vaultState ?? {}, "lastUpdateTs"))}
                      </div>
                    </div>
                  </section>
                </aside>
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}

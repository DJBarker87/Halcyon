"use client";

import { useEffect, useMemo, useState } from "react";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { AlertCircle, ArrowUpRight, CheckCircle2, Loader2, RefreshCcw, Settings2 } from "lucide-react";
import type { BN } from "@coral-xyz/anchor";

import {
  buildBuyTransaction,
  missingFieldsForKind,
  simulatePreview,
  type ProductPreviewResult,
} from "@/lib/halcyon";
import {
  cn,
  enumTag,
  field,
  formatPercentFromBpsS6,
  formatPercentFromS6,
  formatUsdcBaseUnits,
  shortAddress,
  toBaseUnits,
  toNumber,
  toStringValue,
} from "@/lib/format";
import { openRuntimeConfigPanel } from "@/lib/runtime-panel";
import { useRuntimeConfig } from "@/lib/runtime-config";
import { mapSolanaError, type MappedError } from "@/lib/tx-errors";
import type { ProductKind } from "@/lib/types";

type ProductContent = {
  eyebrow: string;
  title: string;
  subtitle: string;
  summary: string;
  image: string;
  imageAlt: string;
  presets: number[];
  defaultAmount: string;
  chips: string[];
  metrics: (data: Record<string, unknown>) => Array<{ label: string; value: string }>;
  notes: string[];
};

const PRODUCT_CONTENT: Record<ProductKind, ProductContent> = {
  flagship: {
    eyebrow: "Equity Autocall",
    title: "SPY · QQQ · IWM coupon note",
    subtitle: "Worst-of-3 autocallable · 18-month tenor",
    summary:
      "Earn a monthly coupon on a basket of the three largest US equity ETFs. The note calls early every quarter if the worst performer stays above its entry level. Your principal is protected unless the worst performer falls past a 20% knock-in barrier at expiry.",
    image: "/img/hero-equity.svg",
    imageAlt: "",
    presets: [25000, 50000, 100000, 250000],
    defaultAmount: "100000",
    chips: ["18-month maturity", "Monthly coupons", "Quarterly autocall", "20% downside cushion"],
    metrics: (data) => [
      { label: "Your premium", value: formatUsdcBaseUnits(field(data, "premium")) },
      { label: "Max payout if triggered", value: formatUsdcBaseUnits(field(data, "maxLiability")) },
      {
        label: "Coupon rate (annualised)",
        value: formatPercentFromBpsS6(field(data, "offeredCouponBpsS6"), 4),
      },
      {
        label: "Implied volatility",
        value: formatPercentFromS6(field(data, "sigmaPricingS6")),
      },
    ],
    notes: [
      "Every quote is computed by a Solana program, not by an off-chain pricing service.",
      "The per-note delta breakdown is committed on-chain by Merkle root, so any auditor can recover the hedge that backs your position.",
      "The coupon rate you see is the coupon you get — your wallet signs the same numbers the program returned.",
    ],
  },
  solAutocall: {
    eyebrow: "SOL Autocall",
    title: "Principal-backed SOL note",
    subtitle: "16-day tenor · 8 observations",
    summary:
      "A short-tenor autocallable on SOL with full principal protection. Earn a coupon every two days while SOL stays above the call level; your USDC principal is escrowed and returned at expiry if the note doesn't auto-call first.",
    image: "/img/hero-sol.svg",
    imageAlt: "",
    presets: [1000, 5000, 10000, 50000],
    defaultAmount: "5000",
    chips: ["16-day maturity", "Coupon every 2 days", "Principal protected", "On-chain pricing"],
    metrics: (data) => [
      { label: "Your premium", value: formatUsdcBaseUnits(field(data, "premium")) },
      { label: "Max payout if triggered", value: formatUsdcBaseUnits(field(data, "maxLiability")) },
      {
        label: "Coupon per observation",
        value: formatPercentFromBpsS6(field(data, "offeredCouponBpsS6")),
      },
      {
        label: "Annualised coupon",
        value: formatPercentFromBpsS6(field(data, "offeredCouponBpsS6"), 182.5),
      },
    ],
    notes: [
      "A zero-coupon quote means the program couldn't price the note under current conditions — try a smaller notional or wait for volatility to settle.",
      "Your USDC principal is held in a program-controlled vault until the note expires or auto-calls.",
    ],
  },
  ilProtection: {
    eyebrow: "IL Protection",
    title: "Impermanent-loss cover",
    subtitle: "Raydium SOL/USDC pools · 30-day tenor",
    summary:
      "Buy a 30-day cover that pays you if SOL/USDC impermanent loss on your Raydium LP position exceeds a threshold. No principal is locked — you pay a premium and get a payout at expiry.",
    image: "/img/hero-il.svg",
    imageAlt: "",
    presets: [5000, 10000, 25000, 50000],
    defaultAmount: "10000",
    chips: ["30-day cover", "Premium-only", "Volatility-aware pricing", "Raydium SOL/USDC"],
    metrics: (data) => [
      { label: "Your premium", value: formatUsdcBaseUnits(field(data, "premium")) },
      { label: "Max payout", value: formatUsdcBaseUnits(field(data, "maxLiability")) },
      {
        label: "Premium (% of LP)",
        value: formatPercentFromS6(field(data, "loadedPremiumFractionS6")),
      },
      {
        label: "Implied volatility",
        value: formatPercentFromS6(field(data, "sigmaPricingS6")),
      },
    ],
    notes: [
      "Premium-only means you don't lock your LP position — you pay a premium, we pay the IL if it exceeds the cover threshold at expiry.",
      "The volatility input used to price your cover is visible on-chain; nothing is taken on trust.",
    ],
  },
};

function formatTimestamp(value: unknown) {
  const numeric = toNumber(value);
  if (!numeric) return "Not set";
  return new Date(numeric * 1000).toLocaleString();
}

function formatValue(value: unknown, key: string) {
  if (value === null || value === undefined) return "Not set";
  if (key.toLowerCase().includes("premium") || key.toLowerCase().includes("liability")) {
    return formatUsdcBaseUnits(value);
  }
  if (key.toLowerCase().includes("expiry") || key.toLowerCase().endsWith("ts")) {
    return formatTimestamp(value);
  }
  if (key.toLowerCase().includes("coupon_bps_s6") || key.toLowerCase().includes("fair_coupon_bps_s6")) {
    return formatPercentFromBpsS6(value);
  }
  if (key.toLowerCase().includes("fraction_s6") || key.toLowerCase().includes("sigma")) {
    return formatPercentFromS6(value);
  }
  if (key.toLowerCase().includes("price_s6")) {
    return `$${(toNumber(value) / 1_000_000).toFixed(4)}`;
  }
  if (typeof value === "boolean") return value ? "Yes" : "No";
  if (typeof value === "object") return enumTag(value);
  return toStringValue(value) || String(value);
}

function titleFromKey(value: string) {
  return value
    .replace(/_/g, " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

function explorerLink(cluster: "localnet" | "devnet" | "mainnet", signature: string) {
  if (!signature || cluster === "localnet") return "";
  const network = cluster === "mainnet" ? "mainnet-beta" : cluster;
  return `https://explorer.solana.com/tx/${signature}?cluster=${network}`;
}

function PreviewFields({ preview }: { preview: ProductPreviewResult }) {
  const entries = Object.entries(preview.data);

  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {entries.map(([key, value]) => (
        <div key={key} className="rounded-md border border-border bg-card p-3">
          <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
            {titleFromKey(key)}
          </div>
          <div className="mt-2 break-all text-sm text-foreground">{formatValue(value, key)}</div>
        </div>
      ))}
    </div>
  );
}

interface IssuancePageProps {
  kind: ProductKind;
  /**
   * Optional override for the starting notional. The detected-LP flow
   * passes the buyer's actual LP value; omitting it falls back to
   * `PRODUCT_CONTENT[kind].defaultAmount`.
   */
  defaultNotional?: string;
}

export function IssuancePage({ kind, defaultNotional }: IssuancePageProps) {
  const product = PRODUCT_CONTENT[kind];
  const { current, cluster } = useRuntimeConfig();
  const { connection } = useConnection();
  const { connected, publicKey, sendTransaction } = useWallet();

  const [amountInput, setAmountInput] = useState(defaultNotional ?? product.defaultAmount);
  const [slippageBps, setSlippageBps] = useState("50");
  const [maxQuoteSlotDelta, setMaxQuoteSlotDelta] = useState("150");
  const [maxEntryPriceDeviationBps, setMaxEntryPriceDeviationBps] = useState("100");
  const [maxExpiryDeltaSecs, setMaxExpiryDeltaSecs] = useState("30");
  const [preview, setPreview] = useState<ProductPreviewResult | null>(null);
  const [previewError, setPreviewError] = useState<MappedError | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [issueLoading, setIssueLoading] = useState(false);
  const [issueError, setIssueError] = useState<MappedError | null>(null);
  const [signature, setSignature] = useState<string | null>(null);

  const amountBaseUnits = useMemo(() => toBaseUnits(amountInput), [amountInput]);
  const missing = useMemo(() => missingFieldsForKind(kind, current), [current, kind]);
  const canPreview = missing.length === 0 && toNumber(amountInput) >= 100;
  const previewUrl = signature ? explorerLink(cluster, signature) : "";

  useEffect(() => {
    setPreview(null);
    setPreviewError(null);
    setSignature(null);
    setIssueError(null);
  }, [amountInput, slippageBps, maxQuoteSlotDelta, maxEntryPriceDeviationBps, maxExpiryDeltaSecs, cluster, current]);

  async function handlePreview() {
    setPreviewLoading(true);
    setPreviewError(null);
    setIssueError(null);
    setSignature(null);
    try {
      const result = await simulatePreview(connection, current, kind, amountBaseUnits);
      setPreview(result);
    } catch (error) {
      setPreview(null);
      setPreviewError(mapSolanaError(error));
      if (error instanceof Error) console.error("preview failed:", error);
    } finally {
      setPreviewLoading(false);
    }
  }

  async function handleIssue() {
    if (!publicKey || !preview) return;

    setIssueLoading(true);
    setIssueError(null);
    setSignature(null);
    try {
      const { transaction } = await buildBuyTransaction(
        connection,
        current,
        kind,
        publicKey,
        preview,
        amountBaseUnits,
        {
          slippageBps: Math.max(0, Number(slippageBps) || 0),
          maxQuoteSlotDelta: Math.max(0, Number(maxQuoteSlotDelta) || 0),
          maxEntryPriceDeviationBps: Math.max(0, Number(maxEntryPriceDeviationBps) || 0),
          maxExpiryDeltaSecs: Math.max(0, Number(maxExpiryDeltaSecs) || 0),
        },
      );

      const txSignature = await sendTransaction(transaction, connection, {
        preflightCommitment: "confirmed",
      });
      await connection.confirmTransaction(txSignature, "confirmed");
      setSignature(txSignature);
    } catch (error) {
      setIssueError(mapSolanaError(error));
      if (error instanceof Error) console.error("issuance failed:", error);
    } finally {
      setIssueLoading(false);
    }
  }

  return (
    <div className="space-y-8">
      <section className="relative overflow-hidden rounded-md border border-border bg-gradient-to-br from-paper via-paper to-halcyonBlue-50">
        <div className="absolute inset-0 opacity-[0.06]" style={{
          backgroundImage: "radial-gradient(circle at 80% 20%, var(--blue-300) 0%, transparent 55%)",
        }} />
        <div className="relative px-5 py-8 sm:px-8 sm:py-10">
          <div className="max-w-3xl">
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              {product.eyebrow}
            </div>
            <h1 className="mt-2 font-serif text-4xl leading-tight text-foreground sm:text-5xl">
              {product.title}
            </h1>
            <p className="mt-2 text-lg text-muted-foreground">{product.subtitle}</p>
            <p className="mt-4 max-w-2xl text-base leading-7 text-foreground/90">{product.summary}</p>

            <div className="mt-5 flex flex-wrap gap-2">
              {product.chips.map((chip) => (
                <span
                  key={chip}
                  className="inline-flex min-h-10 items-center rounded-md border border-border bg-card px-3 text-sm text-foreground"
                >
                  {chip}
                </span>
              ))}
            </div>
          </div>
        </div>
      </section>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.4fr)_360px]">
        <section className="space-y-6">
          <div className="surface p-5 sm:p-6">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div>
                <h2 className="text-xl font-semibold text-foreground">Get a quote</h2>
                <p className="mt-2 text-sm leading-6 text-muted-foreground">
                  Enter how much you want to buy. We'll ask the on-chain program for a live quote and show you
                  the exact coupon and premium before you sign.
                </p>
              </div>
              <button
                type="button"
                aria-label="Network settings"
                onClick={openRuntimeConfigPanel}
                className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                <Settings2 className="h-4 w-4" aria-hidden="true" />
                Network
              </button>
            </div>

            <div className="mt-6 grid gap-5 lg:grid-cols-2">
              <div className="space-y-4">
                <div className="space-y-2">
                  <label htmlFor={`${kind}-amount`} className="field-label">
                    Notional
                  </label>
                  <div className="relative">
                    <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-sm text-muted-foreground">
                      $
                    </span>
                    <input
                      id={`${kind}-amount`}
                      type="number"
                      min={100}
                      step={100}
                      inputMode="decimal"
                      autoComplete="off"
                      value={amountInput}
                      onChange={(event) => setAmountInput(event.target.value)}
                      className="field pl-7"
                    />
                    <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-sm text-muted-foreground">
                      USDC
                    </span>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {product.presets.map((preset) => (
                      <button
                        key={preset}
                        type="button"
                        onClick={() => setAmountInput(String(preset))}
                        className={cn(
                          "inline-flex min-h-10 items-center rounded-md border px-3 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                          amountInput === String(preset)
                            ? "border-primary/30 bg-primary/10 text-foreground"
                            : "border-border bg-background text-muted-foreground hover:bg-secondary hover:text-foreground",
                        )}
                      >
                        {preset >= 1000 ? `$${(preset / 1000).toFixed(0)}k` : `$${preset}`}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="space-y-2">
                  <label htmlFor={`${kind}-slippage`} className="field-label">
                    Slippage tolerance
                  </label>
                  <div className="relative">
                    <input
                      id={`${kind}-slippage`}
                      type="number"
                      min={0}
                      step={1}
                      inputMode="numeric"
                      autoComplete="off"
                      value={slippageBps}
                      onChange={(event) => setSlippageBps(event.target.value)}
                      className="field pr-12"
                    />
                    <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-sm text-muted-foreground">
                      bps
                    </span>
                  </div>
                </div>
              </div>

              <div className="space-y-4">
                <details className="rounded-md border border-border bg-card p-4">
                  <summary className="cursor-pointer list-none text-sm font-medium text-foreground">
                    Advanced safeguards
                  </summary>
                  <p className="mt-2 text-sm leading-6 text-muted-foreground">
                    These are the tolerances your wallet enforces against the on-chain program. The defaults are
                    safe; lower them only if you want a tighter fill.
                  </p>

                  <div className="mt-4 grid gap-4">
                    <div className="space-y-2">
                      <label htmlFor={`${kind}-slot-delta`} className="field-label">
                        Quote freshness (max slots)
                      </label>
                      <input
                        id={`${kind}-slot-delta`}
                        type="number"
                        min={0}
                        step={1}
                        inputMode="numeric"
                        autoComplete="off"
                        value={maxQuoteSlotDelta}
                        onChange={(event) => setMaxQuoteSlotDelta(event.target.value)}
                        className="field"
                      />
                    </div>
                    <div className="space-y-2">
                      <label htmlFor={`${kind}-entry-deviation`} className="field-label">
                        Entry-price drift
                      </label>
                      <div className="relative">
                        <input
                          id={`${kind}-entry-deviation`}
                          type="number"
                          min={0}
                          step={1}
                          inputMode="numeric"
                          autoComplete="off"
                          value={maxEntryPriceDeviationBps}
                          onChange={(event) => setMaxEntryPriceDeviationBps(event.target.value)}
                          className="field pr-12"
                        />
                        <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-sm text-muted-foreground">
                          bps
                        </span>
                      </div>
                    </div>
                    <div className="space-y-2">
                      <label htmlFor={`${kind}-expiry-delta`} className="field-label">
                        Expiry-time drift
                      </label>
                      <div className="relative">
                        <input
                          id={`${kind}-expiry-delta`}
                          type="number"
                          min={0}
                          step={1}
                          inputMode="numeric"
                          autoComplete="off"
                          value={maxExpiryDeltaSecs}
                          onChange={(event) => setMaxExpiryDeltaSecs(event.target.value)}
                          className="field pr-16"
                        />
                        <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-sm text-muted-foreground">
                          secs
                        </span>
                      </div>
                    </div>
                  </div>
                </details>

                <div className="rounded-md border border-border bg-card p-4">
                  <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                    Selected cluster
                  </div>
                  <div className="mt-2 text-sm font-medium text-foreground">{cluster}</div>
                  <div className="mt-2 space-y-1 text-sm text-muted-foreground">
                    <div>RPC: {current.rpcUrl || "Not set"}</div>
                    <div>Kernel: {current.kernelProgramId ? shortAddress(current.kernelProgramId, 6) : "Not set"}</div>
                  </div>
                </div>
              </div>
            </div>

            {missing.length > 0 && (
              <div className="mt-6 rounded-md border border-destructive/30 bg-destructive/10 p-4">
                <div className="flex items-start gap-3">
                  <AlertCircle className="mt-0.5 h-5 w-5 text-destructive" aria-hidden="true" />
                  <div>
                    <div className="text-sm font-medium text-foreground">Network not fully configured</div>
                    <p className="mt-1 text-sm text-muted-foreground">
                      The current cluster is missing a few addresses needed to price this note. Pick a different
                      cluster or contact the operator to have them pinned.
                    </p>
                    <ul className="mt-3 space-y-1 text-sm text-foreground">
                      {missing.map((item) => (
                        <li key={item.key}>• {item.label}</li>
                      ))}
                    </ul>
                    <button
                      type="button"
                      onClick={openRuntimeConfigPanel}
                      className="mt-4 inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                    >
                      <Settings2 className="h-4 w-4" aria-hidden="true" />
                      Open network settings
                    </button>
                  </div>
                </div>
              </div>
            )}

            <div className="mt-6 flex flex-wrap gap-3">
              <button
                type="button"
                onClick={handlePreview}
                disabled={!canPreview || previewLoading}
                className="inline-flex min-h-11 items-center justify-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50"
              >
                {previewLoading ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> : null}
                Preview quote
              </button>

              <button
                type="button"
                onClick={handleIssue}
                disabled={!connected || !preview || issueLoading}
                className="inline-flex min-h-11 items-center justify-center gap-2 rounded-md border border-border bg-background px-4 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50"
              >
                {issueLoading ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> : null}
                Sign and issue
              </button>

              {!connected && (
                <div className="inline-flex min-h-11 items-center text-sm text-muted-foreground">
                  Connect a wallet to buy the note.
                </div>
              )}
            </div>
          </div>

          {previewLoading && !preview && (
            <div className="surface grid gap-4 p-5 sm:grid-cols-2 sm:p-6">
              {Array.from({ length: 4 }).map((_, index) => (
                <div
                  key={index}
                  className="h-24 rounded-md border border-border bg-n-50 motion-safe:animate-pulse"
                />
              ))}
            </div>
          )}

          {previewError && (
            <ErrorBlock
              error={previewError}
              onRetry={previewError.retryable ? handlePreview : undefined}
            />
          )}

          {preview && (
            <div className="space-y-6">
              <section className="surface p-5 sm:p-6">
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div>
                    <h2 className="text-xl font-semibold text-foreground">Your quote</h2>
                    <p className="mt-2 text-sm leading-6 text-muted-foreground">
                      Priced by the on-chain program right now. Sign to lock these numbers in.
                    </p>
                  </div>
                  <div className="text-sm text-muted-foreground">
                    Quoted at slot {toNumber(field(preview.data, "quoteSlot")) || "—"}
                  </div>
                </div>

                <div className="mt-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                  {product.metrics(preview.data).map((metric) => (
                    <div key={metric.label} className="rounded-md border border-border bg-card p-4">
                      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                        {metric.label}
                      </div>
                      <div className="mt-3 text-2xl font-semibold text-foreground">{metric.value}</div>
                    </div>
                  ))}
                </div>
              </section>

              <section className="surface p-5 sm:p-6">
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div>
                    <h2 className="text-xl font-semibold text-foreground">Full quote breakdown</h2>
                    <p className="mt-2 text-sm leading-6 text-muted-foreground">
                      Everything the on-chain program returned — visible before you sign, and identical to what
                      your wallet will enforce when it does.
                    </p>
                  </div>
                  <div className="text-sm text-muted-foreground">
                    Expires {formatTimestamp(field(preview.data, "expiryTs"))}
                  </div>
                </div>
                <div className="mt-6">
                  <PreviewFields preview={preview} />
                </div>
              </section>
            </div>
          )}

          {issueError && (
            <ErrorBlock
              error={issueError}
              onRetry={issueError.retryable ? handleIssue : undefined}
            />
          )}

          {signature && (
            <div className="surface p-5 sm:p-6">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="flex items-start gap-3">
                  <CheckCircle2 className="mt-0.5 h-5 w-5 text-success-700" aria-hidden="true" />
                  <div>
                    <h2 className="text-lg font-semibold text-foreground">You've bought the note.</h2>
                    <p className="mt-2 break-all text-sm leading-6 text-muted-foreground">{signature}</p>
                  </div>
                </div>

                {previewUrl ? (
                  <a
                    href={previewUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                  >
                    View in explorer
                    <ArrowUpRight className="h-4 w-4" aria-hidden="true" />
                  </a>
                ) : null}
              </div>
            </div>
          )}
        </section>

        <aside className="space-y-6">
          <section className="surface p-5">
            <h2 className="text-lg font-semibold text-foreground">How this quote was priced</h2>
            <ul className="mt-4 space-y-3 text-sm leading-6 text-muted-foreground">
              {product.notes.map((note) => (
                <li key={note}>• {note}</li>
              ))}
            </ul>
          </section>

          <section className="surface p-5">
            <h2 className="text-lg font-semibold text-foreground">What "on-chain pricing" means here</h2>
            <dl className="mt-4 space-y-4 text-sm">
              <div>
                <dt className="text-muted-foreground">Who computes the coupon</dt>
                <dd className="mt-1 text-foreground">A Rust program on Solana. No off-chain pricing service.</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Can you verify it</dt>
                <dd className="mt-1 text-foreground">Yes — every quote is reproducible from the open-source pricer plus the on-chain state at its slot.</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Who backs the note</dt>
                <dd className="mt-1 text-foreground">A shared underwriting vault governed by the protocol's kernel program.</dd>
              </div>
            </dl>
          </section>

          <section className="surface p-5">
            <h2 className="text-lg font-semibold text-foreground">Before you sign</h2>
            <div className="mt-4 space-y-3 text-sm text-muted-foreground">
              <div className="rounded-md border border-border bg-card p-3">
                Minimum ticket is $100.
              </div>
              <div className="rounded-md border border-border bg-card p-3">
                Quotes expire — refresh after changing the notional or the advanced safeguards.
              </div>
              <div className="rounded-md border border-border bg-card p-3">
                Your wallet enforces the same tolerances shown here. If the on-chain price drifts outside them, the
                transaction fails safely without charging you.
              </div>
            </div>
          </section>
        </aside>
      </div>
    </div>
  );
}

function ErrorBlock({ error, onRetry }: { error: MappedError; onRetry?: () => void }) {
  return (
    <div className="surface p-5 sm:p-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex items-start gap-3">
          <AlertCircle className="mt-0.5 h-5 w-5 shrink-0 text-warning-700" aria-hidden="true" />
          <div className="min-w-0">
            <h2 className="text-lg font-semibold text-foreground">{error.title}</h2>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">{error.body}</p>
            {error.detail && (
              <details className="mt-3">
                <summary className="cursor-pointer text-xs text-muted-foreground hover:text-foreground">
                  Technical detail
                </summary>
                <pre className="mt-2 whitespace-pre-wrap break-all rounded-md border border-border bg-card p-3 font-mono text-[11px] text-muted-foreground">
                  {error.detail}
                </pre>
              </details>
            )}
          </div>
        </div>
        {onRetry && (
          <button
            type="button"
            onClick={onRetry}
            className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <RefreshCcw className="h-4 w-4" aria-hidden="true" />
            Try again
          </button>
        )}
      </div>
    </div>
  );
}

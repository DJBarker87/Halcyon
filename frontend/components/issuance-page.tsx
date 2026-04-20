"use client";

import Image from "next/image";
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
    eyebrow: "Product 01",
    title: "Flagship Worst-of Equity Autocall",
    subtitle: "SPY / QQQ / IWM · on-chain quote path",
    summary:
      "Preview runs against the live flagship program and returns the real premium, liability, coupon, and entry levels from the current kernel state.",
    image:
      "https://images.unsplash.com/photo-1611974789855-9c2a0a7236a3?auto=format&fit=crop&w=1400&q=80",
    imageAlt: "Trading screens with equity charts",
    presets: [25000, 50000, 100000, 250000],
    defaultAmount: "100000",
    chips: ["18-month tenor", "Monthly coupons", "Quarterly autocall checks", "80% KI barrier"],
    metrics: (data) => [
      { label: "Premium", value: formatUsdcBaseUnits(field(data, "premium")) },
      { label: "Max liability", value: formatUsdcBaseUnits(field(data, "maxLiability")) },
      {
        label: "Offered coupon",
        value: formatPercentFromBpsS6(field(data, "offeredCouponBpsS6"), 4),
      },
      {
        label: "Pricing sigma",
        value: formatPercentFromS6(field(data, "sigmaPricingS6")),
      },
    ],
    notes: [
      "Preview uses the product program's `preview_quote` handler via `simulateTransaction`.",
      "Issuance builds a v0 transaction and requires the flagship ALT registry to be populated.",
      "Flagship hedge state uses an analytical delta engine rather than a heuristic estimate, with per-note outputs committed by Merkle root for audit.",
    ],
  },
  solAutocall: {
    eyebrow: "Product 02",
    title: "SOL Autocall",
    subtitle: "Principal-backed SOL note · live kernel issuance",
    summary:
      "This copy keeps the old product page shape but moves the pricing and issuance path onto the real SOL autocall program and kernel PDAs.",
    image:
      "https://images.unsplash.com/photo-1639762681485-074b7f938ba0?auto=format&fit=crop&w=1400&q=80",
    imageAlt: "Abstract digital coin and market backdrop",
    presets: [1000, 5000, 10000, 50000],
    defaultAmount: "5000",
    chips: ["16-day tenor", "8 observations", "Principal escrow", "ALT-backed v0 tx"],
    metrics: (data) => [
      { label: "Premium", value: formatUsdcBaseUnits(field(data, "premium")) },
      { label: "Max liability", value: formatUsdcBaseUnits(field(data, "maxLiability")) },
      {
        label: "Coupon / observation",
        value: formatPercentFromBpsS6(field(data, "offeredCouponBpsS6")),
      },
      {
        label: "Headline annualized",
        value: formatPercentFromBpsS6(field(data, "offeredCouponBpsS6"), 182.5),
      },
    ],
    notes: [
      "The preview can legitimately return a zero coupon in no-quote conditions.",
      "Issuance escrows principal plus premium vault share, matching the product registry requirements.",
    ],
  },
  ilProtection: {
    eyebrow: "Product 03",
    title: "IL Protection",
    subtitle: "SOL / USDC synthetic cover",
    summary:
      "This is the synthetic issuance path from the architecture doc: quote on-chain, pay premium, and reserve shared vault capital without principal escrow.",
    image:
      "https://images.unsplash.com/photo-1551288049-bebda4e38f71?auto=format&fit=crop&w=1400&q=80",
    imageAlt: "Analytics dashboard with risk metrics",
    presets: [5000, 10000, 25000, 50000],
    defaultAmount: "10000",
    chips: ["30-day tenor", "Synthetic-only", "Regime-aware sigma", "Raydium SOL/USDC"],
    metrics: (data) => [
      { label: "Premium", value: formatUsdcBaseUnits(field(data, "premium")) },
      { label: "Max liability", value: formatUsdcBaseUnits(field(data, "maxLiability")) },
      {
        label: "Loaded premium",
        value: formatPercentFromS6(field(data, "loadedPremiumFractionS6")),
      },
      {
        label: "Pricing sigma",
        value: formatPercentFromS6(field(data, "sigmaPricingS6")),
      },
    ],
    notes: [
      "Synthetic cover means the buyer only pays the premium path that the kernel accepts for non-principal-backed products.",
      "The preview surfaces regime and volatility inputs directly from the on-chain signals.",
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
        <div key={key} className="rounded-md border border-border bg-background/70 p-3">
          <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
            {titleFromKey(key)}
          </div>
          <div className="mt-2 break-all text-sm text-foreground">{formatValue(value, key)}</div>
        </div>
      ))}
    </div>
  );
}

export function IssuancePage({ kind }: { kind: ProductKind }) {
  const product = PRODUCT_CONTENT[kind];
  const { current, cluster } = useRuntimeConfig();
  const { connection } = useConnection();
  const { connected, publicKey, sendTransaction } = useWallet();

  const [amountInput, setAmountInput] = useState(product.defaultAmount);
  const [slippageBps, setSlippageBps] = useState("50");
  const [maxQuoteSlotDelta, setMaxQuoteSlotDelta] = useState("150");
  const [maxEntryPriceDeviationBps, setMaxEntryPriceDeviationBps] = useState("100");
  const [maxExpiryDeltaSecs, setMaxExpiryDeltaSecs] = useState("30");
  const [preview, setPreview] = useState<ProductPreviewResult | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [issueLoading, setIssueLoading] = useState(false);
  const [issueError, setIssueError] = useState<string | null>(null);
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
      setPreviewError(error instanceof Error ? error.message : "Preview failed");
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
      setIssueError(error instanceof Error ? error.message : "Issuance failed");
    } finally {
      setIssueLoading(false);
    }
  }

  return (
    <div className="space-y-8">
      <section className="relative overflow-hidden rounded-md border border-border">
        <Image
          src={product.image}
          alt={product.imageAlt}
          width={1600}
          height={720}
          className="h-[260px] w-full object-cover object-center sm:h-[320px]"
          priority
        />
        <div className="absolute inset-0 bg-gradient-to-r from-background via-background/88 to-background/30" />
        <div className="absolute inset-x-0 bottom-0 top-0 flex items-end px-5 py-5 sm:px-8 sm:py-8">
          <div className="max-w-3xl">
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              {product.eyebrow}
            </div>
            <h1 className="mt-2 text-3xl font-semibold tracking-tight text-foreground sm:text-4xl">
              {product.title}
            </h1>
            <p className="mt-2 text-lg text-muted-foreground">{product.subtitle}</p>
            <p className="mt-4 max-w-2xl text-sm leading-6 text-foreground/90 sm:text-base">{product.summary}</p>

            <div className="mt-5 flex flex-wrap gap-2">
              {product.chips.map((chip) => (
                <span
                  key={chip}
                  className="inline-flex min-h-10 items-center rounded-md border border-border bg-background/70 px-3 text-sm text-foreground"
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
                <h2 className="text-xl font-semibold text-foreground">Issue new policy</h2>
                <p className="mt-2 text-sm leading-6 text-muted-foreground">
                  This path replaces the browser-side pricer for {product.title.toLowerCase()} and sends the
                  real preview and accept flow through the selected cluster.
                </p>
              </div>
              <button
                type="button"
                onClick={openRuntimeConfigPanel}
                className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                <Settings2 className="h-4 w-4" aria-hidden="true" />
                Runtime Config
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
                <details className="rounded-md border border-border bg-background/70 p-4">
                  <summary className="cursor-pointer list-none text-sm font-medium text-foreground">
                    Acceptance bounds
                  </summary>
                  <p className="mt-2 text-sm leading-6 text-muted-foreground">
                    These map directly to the on-chain `accept_quote` guardrails.
                  </p>

                  <div className="mt-4 grid gap-4">
                    <div className="space-y-2">
                      <label htmlFor={`${kind}-slot-delta`} className="field-label">
                        Max quote slot delta
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
                        Max entry price deviation
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
                        Max expiry delta
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

                <div className="rounded-md border border-border bg-background/70 p-4">
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
                    <div className="text-sm font-medium text-foreground">Missing runtime values</div>
                    <p className="mt-1 text-sm text-muted-foreground">
                      Add the required program and oracle accounts before previewing this product.
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
                      Open runtime config
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
                  Connect a wallet to submit issuance.
                </div>
              )}
            </div>
          </div>

          {previewLoading && !preview && (
            <div className="surface grid gap-4 p-5 sm:grid-cols-2 sm:p-6">
              {Array.from({ length: 4 }).map((_, index) => (
                <div
                  key={index}
                  className="h-24 rounded-md border border-border bg-background/60 motion-safe:animate-pulse"
                />
              ))}
            </div>
          )}

          {previewError && (
            <div className="surface p-5 sm:p-6">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div>
                  <h2 className="text-lg font-semibold text-foreground">Preview failed</h2>
                  <p className="mt-2 text-sm leading-6 text-muted-foreground">{previewError}</p>
                </div>
                <button
                  type="button"
                  onClick={handlePreview}
                  className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                >
                  <RefreshCcw className="h-4 w-4" aria-hidden="true" />
                  Retry
                </button>
              </div>
            </div>
          )}

          {preview && (
            <div className="space-y-6">
              <section className="surface p-5 sm:p-6">
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div>
                    <h2 className="text-xl font-semibold text-foreground">Live quote</h2>
                    <p className="mt-2 text-sm leading-6 text-muted-foreground">
                      Decoded from Anchor return data on the selected cluster.
                    </p>
                  </div>
                  <div className="text-sm text-muted-foreground">
                    Quote slot {toNumber(field(preview.data, "quoteSlot")) || "Not set"}
                  </div>
                </div>

                <div className="mt-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                  {product.metrics(preview.data).map((metric) => (
                    <div key={metric.label} className="rounded-md border border-border bg-background/70 p-4">
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
                    <h2 className="text-xl font-semibold text-foreground">Decoded preview payload</h2>
                    <p className="mt-2 text-sm leading-6 text-muted-foreground">
                      Raw fields from the product `QuotePreview` struct.
                    </p>
                  </div>
                  <div className="text-sm text-muted-foreground">
                    Expiry {formatTimestamp(field(preview.data, "expiryTs"))}
                  </div>
                </div>
                <div className="mt-6">
                  <PreviewFields preview={preview} />
                </div>
              </section>
            </div>
          )}

          {issueError && (
            <div className="surface p-5 sm:p-6">
              <h2 className="text-lg font-semibold text-foreground">Issuance failed</h2>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">{issueError}</p>
            </div>
          )}

          {signature && (
            <div className="surface p-5 sm:p-6">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="flex items-start gap-3">
                  <CheckCircle2 className="mt-0.5 h-5 w-5 text-emerald-400" aria-hidden="true" />
                  <div>
                    <h2 className="text-lg font-semibold text-foreground">Transaction submitted</h2>
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
            <h2 className="text-lg font-semibold text-foreground">Issuance notes</h2>
            <ul className="mt-4 space-y-3 text-sm leading-6 text-muted-foreground">
              {product.notes.map((note) => (
                <li key={note}>• {note}</li>
              ))}
            </ul>
          </section>

          <section className="surface p-5">
            <h2 className="text-lg font-semibold text-foreground">What this page uses</h2>
            <dl className="mt-4 space-y-4 text-sm">
              <div>
                <dt className="text-muted-foreground">Quote source</dt>
                <dd className="mt-1 text-foreground">`simulateTransaction` + Anchor return data</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Transaction format</dt>
                <dd className="mt-1 text-foreground">Versioned transaction with product lookup tables</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Kernel path</dt>
                <dd className="mt-1 text-foreground">ProtocolConfig, ProductRegistryEntry, VaultState, FeeLedger</dd>
              </div>
            </dl>
          </section>

          <section className="surface p-5">
            <h2 className="text-lg font-semibold text-foreground">Validation</h2>
            <div className="mt-4 space-y-3 text-sm text-muted-foreground">
              <div className="rounded-md border border-border bg-background/70 p-3">
                Minimum ticket is $100.
              </div>
              <div className="rounded-md border border-border bg-background/70 p-3">
                Quote preview must be refreshed after any notional or guardrail change.
              </div>
              <div className="rounded-md border border-border bg-background/70 p-3">
                Wallet issuance will fail cleanly if ALT registry data is missing on the selected cluster.
              </div>
            </div>
          </section>
        </aside>
      </div>
    </div>
  );
}

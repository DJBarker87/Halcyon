"use client";

import Link from "next/link";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { BN } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import {
  ArrowRight,
  CheckCircle2,
  ExternalLink,
  HandCoins,
  Loader2,
  PanelRightOpen,
  Play,
  ReceiptText,
  RefreshCw,
  ShieldCheck,
  X,
} from "lucide-react";

import {
  buildDemoPriceAndIssueLoanTransaction,
  missingFieldsForKind,
  simulatePreview,
} from "@/lib/halcyon";
import {
  cn,
  field,
  formatPercentFromBpsS6,
  formatPercentFromS6,
  shortAddress,
  toNumber,
  toStringValue,
} from "@/lib/format";
import { useRuntimeConfig } from "@/lib/runtime-config";
import { mapSolanaError } from "@/lib/tx-errors";
import type { ProductKind } from "@/lib/types";

type ProductKey = "equity" | "sol" | "lp";
type BorrowStage = "idle" | "computing" | "ready" | "sending" | "confirmed" | "error";

type QuoteState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; data: Record<string, unknown>; quoteSlot: number; fetchedAt: number }
  | { status: "error"; error: string };

type ReceiptRow = {
  label: string;
  value: string;
  tone?: "primary" | "neutral";
  source: string;
};

type DetailTile = {
  symbol: string;
  value: string;
  source: string;
};

type BorrowQuote = {
  notionalBaseUnits: BN;
  fairValueBaseUnits: BN;
  lendingValueBaseUnits: BN;
  maxBorrowBaseUnits: BN;
  debtBaseUnits: BN;
  sourceSlot: BN;
};

type LastTx = {
  signature: string;
  slot: number | null;
  unitsConsumed: number | null;
};

const RECEIPT_MINT = new PublicKey("AJAQcAqthGL2BXj9kUQEsPcyEV2cyuh4zF5UuRh3M2Zx");
const DEMO_BORROWER = new PublicKey("8rMmhLp2kFy6uBETEi9T7V9Q8SAP8cLUb2D4EhmgcKyK");

function usdBase(value: number) {
  return new BN(Math.round(value * 1_000_000).toString());
}

const PRODUCTS = {
  equity: {
    kind: "flagship",
    label: "Equity Autocall",
    route: "/flagship",
    underlying: "SPY / QQQ / IWM",
    receipt: "Live devnet note receipt",
    notionalBaseUnits: usdBase(10_000),
  },
  sol: {
    kind: "solAutocall",
    label: "SOL Autocall",
    route: "/sol-autocall",
    underlying: "SOL",
    receipt: "Shipping product receipt",
    notionalBaseUnits: usdBase(5_000),
  },
  lp: {
    kind: "ilProtection",
    label: "LP Protection",
    route: "/il-protection",
    underlying: "SOL/USDC LP",
    receipt: "Protection quote receipt",
    notionalBaseUnits: usdBase(18_500),
  },
} satisfies Record<
  ProductKey,
  {
    kind: ProductKind;
    label: string;
    route: string;
    underlying: string;
    receipt: string;
    notionalBaseUnits: BN;
  }
>;

const INITIAL_QUOTES: Record<ProductKey, QuoteState> = {
  equity: { status: "idle" },
  sol: { status: "idle" },
  lp: { status: "idle" },
};

function explorerTxUrl(signature: string, cluster: string) {
  const suffix = cluster === "mainnet" ? "" : `?cluster=${cluster === "localnet" ? "devnet" : cluster}`;
  return `https://solscan.io/tx/${signature}${suffix}`;
}

function toIntegerBigInt(value: unknown) {
  const stringValue = toStringValue(value);
  if (/^-?\d+$/.test(stringValue)) return BigInt(stringValue);
  const numeric = toNumber(value);
  if (!Number.isFinite(numeric)) return 0n;
  return BigInt(Math.trunc(numeric));
}

function baseUnitsBn(value: unknown) {
  const stringValue = toStringValue(value);
  if (/^\d+$/.test(stringValue)) return new BN(stringValue);
  const numeric = toNumber(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return new BN(0);
  return new BN(Math.trunc(numeric).toString());
}

function formatUsdcBaseUnitsExact(value: unknown, minimumFractionDigits = 0) {
  const raw = toIntegerBigInt(value);
  const negative = raw < 0n;
  const absolute = negative ? -raw : raw;
  const whole = absolute / 1_000_000n;
  const fraction = absolute % 1_000_000n;
  const amount = Number(whole) + Number(fraction) / 1_000_000;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits,
    maximumFractionDigits: 2,
  }).format(negative ? -amount : amount);
}

function formatCouponCash(notionalBaseUnits: unknown, couponBpsS6: unknown) {
  const notional = toIntegerBigInt(notionalBaseUnits);
  const bpsS6 = toIntegerBigInt(couponBpsS6);
  if (notional <= 0n || bpsS6 <= 0n) return "$0";
  const couponBaseUnits = (notional * bpsS6) / 10_000n / 1_000_000n;
  return formatUsdcBaseUnitsExact(couponBaseUnits, 2);
}

function formatUsdPriceS6(value: unknown) {
  const price = toNumber(value) / 1_000_000;
  if (!Number.isFinite(price) || price <= 0) return "Unavailable";
  return `$${price.toFixed(price >= 100 ? 2 : 4)}`;
}

function formatSlot(value: unknown) {
  const slot = toNumber(value);
  return slot > 0 ? slot.toLocaleString() : "Unavailable";
}

function formatTimestamp(value: unknown) {
  const seconds = toNumber(value);
  if (!seconds) return "Unavailable";
  return new Date(seconds * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

function formatFlagshipEntryBasket(data: Record<string, unknown>) {
  return [
    ["SPY", field(data, "entrySpyPriceS6")],
    ["QQQ", field(data, "entryQqqPriceS6")],
    ["IWM", field(data, "entryIwmPriceS6")],
  ]
    .map(([symbol, price]) => `${symbol} ${formatUsdPriceS6(price)}`)
    .join(" / ");
}

function sourceSlotFor(state: QuoteState) {
  if (state.status !== "ready") return new BN(0);
  return new BN(String(Math.max(0, Math.trunc(state.quoteSlot))));
}

function deriveBorrowQuote(state: QuoteState) {
  if (state.status !== "ready") {
    throw new Error("Run a live equity preview before building the lending transaction.");
  }
  const fairValueBaseUnits = baseUnitsBn(field(state.data, "maxLiability"));
  if (fairValueBaseUnits.lte(new BN(0))) {
    throw new Error("The flagship program returned no liability to lend against.");
  }

  const lendingValueBaseUnits = fairValueBaseUnits.muln(70).divn(100);
  const maxBorrowBaseUnits = lendingValueBaseUnits.muln(80).divn(100);

  return {
    notionalBaseUnits: PRODUCTS.equity.notionalBaseUnits,
    fairValueBaseUnits,
    lendingValueBaseUnits,
    maxBorrowBaseUnits,
    debtBaseUnits: maxBorrowBaseUnits,
    sourceSlot: sourceSlotFor(state),
  } satisfies BorrowQuote;
}

function receiptRowsFor(product: ProductKey, quote: QuoteState): ReceiptRow[] {
  if (quote.status === "loading" || quote.status === "idle") {
    return [
      { label: "Quote status", value: "Loading live preview", tone: "primary", source: "on-chain simulateTransaction" },
      { label: "Program", value: PRODUCTS[product].kind, source: "configured deployment" },
      { label: "Data source", value: "No fallback values", source: "unavailable until RPC returns" },
    ];
  }
  if (quote.status === "error") {
    return [
      { label: "Quote status", value: "Unavailable", tone: "primary", source: "RPC/program error" },
      { label: "Reason", value: quote.error, source: "runtime error" },
      { label: "Fallback", value: "None", source: "synthetic data disabled" },
    ];
  }

  const data = quote.data;
  if (product === "sol") {
    return [
      {
        label: "Principal escrowed",
        value: formatUsdcBaseUnitsExact(field(data, "maxLiability")),
        tone: "primary",
        source: "sol_autocall.preview_quote",
      },
      {
        label: "Coupon if paid",
        value: formatCouponCash(field(data, "maxLiability"), field(data, "offeredCouponBpsS6")),
        source: "sol_autocall.preview_quote",
      },
      {
        label: "Entry SOL",
        value: formatUsdPriceS6(field(data, "entryPriceS6")),
        source: "Pyth account read by program",
      },
    ];
  }

  if (product === "lp") {
    return [
      {
        label: "30-day premium",
        value: formatUsdcBaseUnitsExact(field(data, "premium"), 2),
        tone: "primary",
        source: "il_protection.preview_quote",
      },
      {
        label: "Maximum cover",
        value: formatUsdcBaseUnitsExact(field(data, "maxLiability")),
        source: "il_protection.preview_quote",
      },
      {
        label: "Pricing volatility",
        value: formatPercentFromS6(field(data, "sigmaPricingS6")),
        source: "vault sigma state",
      },
    ];
  }

  return [
    {
      label: "Program mark",
      value: formatUsdcBaseUnitsExact(field(data, "maxLiability")),
      tone: "primary",
      source: "flagship.preview_quote",
    },
    {
      label: "Coupon if paid",
      value: formatCouponCash(field(data, "maxLiability"), field(data, "offeredCouponBpsS6")),
      source: "flagship.preview_quote",
    },
    {
      label: "Entry basket",
      value: formatFlagshipEntryBasket(data),
      source: "Pyth accounts read by program",
    },
  ];
}

function detailTilesFor(product: ProductKey, quote: QuoteState): DetailTile[] {
  if (quote.status !== "ready") {
    const source = quote.status === "error" ? "unavailable" : "waiting for RPC";
    return [
      { symbol: "Quote slot", value: "Unavailable", source },
      { symbol: "Engine", value: "Unavailable", source },
      { symbol: "Expiry", value: "Unavailable", source },
    ];
  }

  const data = quote.data;
  if (product === "equity") {
    return [
      { symbol: "SPY", value: formatUsdPriceS6(field(data, "entrySpyPriceS6")), source: "Pyth" },
      { symbol: "QQQ", value: formatUsdPriceS6(field(data, "entryQqqPriceS6")), source: "Pyth" },
      { symbol: "IWM", value: formatUsdPriceS6(field(data, "entryIwmPriceS6")), source: "Pyth" },
    ];
  }
  if (product === "sol") {
    return [
      { symbol: "Quote slot", value: formatSlot(quote.quoteSlot), source: "program return" },
      { symbol: "Coupon rate", value: formatPercentFromBpsS6(field(data, "offeredCouponBpsS6")), source: "program return" },
      { symbol: "Expiry", value: formatTimestamp(field(data, "expiryTs")), source: "program return" },
    ];
  }
  return [
    { symbol: "Entry SOL", value: formatUsdPriceS6(field(data, "entrySolPriceS6")), source: "Pyth" },
    { symbol: "Entry USDC", value: formatUsdPriceS6(field(data, "entryUsdcPriceS6")), source: "Pyth" },
    { symbol: "Premium rate", value: formatPercentFromS6(field(data, "loadedPremiumFractionS6")), source: "program return" },
  ];
}

function closeStatus(state: QuoteState) {
  if (state.status === "ready") return `preview_quote slot ${formatSlot(state.quoteSlot)}`;
  if (state.status === "loading") return "Loading live preview";
  if (state.status === "error") return "Unavailable on current RPC";
  return "Awaiting preview";
}

export function DemoScriptRunner() {
  const { connection } = useConnection();
  const { connected, publicKey, sendTransaction } = useWallet();
  const { cluster, current } = useRuntimeConfig();
  const [activeProduct, setActiveProduct] = useState<ProductKey>("equity");
  const [quotes, setQuotes] = useState<Record<ProductKey, QuoteState>>(INITIAL_QUOTES);
  const [modalOpen, setModalOpen] = useState(false);
  const [borrowStage, setBorrowStage] = useState<BorrowStage>("idle");
  const [borrowQuote, setBorrowQuote] = useState<BorrowQuote | null>(null);
  const [cuCounter, setCuCounter] = useState(0);
  const [lastTx, setLastTx] = useState<LastTx | null>(null);
  const [txError, setTxError] = useState<string | null>(null);

  const active = PRODUCTS[activeProduct];
  const activeQuote = quotes[activeProduct];

  const loadQuote = useCallback(
    async (product: ProductKey): Promise<QuoteState> => {
      const config = PRODUCTS[product];
      const missing = missingFieldsForKind(config.kind, current);
      if (missing.length > 0) {
        const state: QuoteState = {
          status: "error",
          error: `Missing ${missing.map((fieldInfo) => fieldInfo.label).join(", ")}`,
        };
        setQuotes((existing) => ({ ...existing, [product]: state }));
        return state;
      }

      setQuotes((existing) => ({ ...existing, [product]: { status: "loading" } }));
      try {
        const preview = await simulatePreview(connection, current, config.kind, config.notionalBaseUnits);
        const quoteSlot = toNumber(field(preview.data, "quoteSlot")) || (await connection.getSlot("confirmed"));
        const state: QuoteState = {
          status: "ready",
          data: preview.data,
          quoteSlot,
          fetchedAt: Date.now(),
        };
        setQuotes((existing) => ({ ...existing, [product]: state }));
        return state;
      } catch (cause) {
        const mapped = mapSolanaError(cause);
        const state: QuoteState = {
          status: "error",
          error: `${mapped.title} ${mapped.body}`,
        };
        setQuotes((existing) => ({ ...existing, [product]: state }));
        return state;
      }
    },
    [connection, current],
  );

  useEffect(() => {
    void loadQuote(activeProduct);
  }, [activeProduct, loadQuote]);

  useEffect(() => {
    if (!modalOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setModalOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [modalOpen]);

  const receiptRows = useMemo(() => receiptRowsFor(activeProduct, activeQuote), [activeProduct, activeQuote]);
  const detailTiles = useMemo(() => detailTilesFor(activeProduct, activeQuote), [activeProduct, activeQuote]);

  const markSourceLabel =
    activeQuote.status === "ready"
      ? `${active.label} values came from ${active.kind}.preview_quote on ${cluster} at slot ${formatSlot(
          activeQuote.quoteSlot,
        )}. ${lastTx ? `Latest signed tx: ${shortAddress(lastTx.signature, 8)}.` : "No signed borrow tx yet."}`
      : activeQuote.status === "error"
        ? `No fallback mark is rendered. ${activeQuote.error}`
        : `Waiting for ${active.kind}.preview_quote on ${cluster}.`;

  async function prepareBorrowQuote() {
    setModalOpen(true);
    setBorrowStage("computing");
    setTxError(null);
    setCuCounter(0);

    try {
      const quoteState = quotes.equity.status === "ready" ? quotes.equity : await loadQuote("equity");
      const derived = deriveBorrowQuote(quoteState);
      setBorrowQuote(derived);
      setBorrowStage("ready");
      return derived;
    } catch (cause) {
      const mapped = mapSolanaError(cause);
      setBorrowStage("error");
      setTxError(`${mapped.title} ${mapped.body}`);
      return null;
    }
  }

  async function sendBorrow() {
    if (!connected || !publicKey) {
      setBorrowStage("error");
      setTxError("Connect a wallet first. Judges can use the Faucet page to get mockUSDC.");
      return;
    }
    if (!current.lendingConsumerProgramId.trim()) {
      setBorrowStage("error");
      setTxError("The lending-consumer program id is not configured for this cluster.");
      return;
    }

    const pricing = borrowQuote ?? (await prepareBorrowQuote());
    if (!pricing) return;

    setBorrowStage("sending");
    setTxError(null);
    try {
      const transaction = await buildDemoPriceAndIssueLoanTransaction(connection, current, publicKey, {
        receiptMint: RECEIPT_MINT,
        borrower: DEMO_BORROWER,
        loanId: new BN(Date.now().toString()),
        notionalBaseUnits: pricing.notionalBaseUnits,
        fairValueBaseUnits: pricing.fairValueBaseUnits,
        lendingValueBaseUnits: pricing.lendingValueBaseUnits,
        maxBorrowBaseUnits: pricing.maxBorrowBaseUnits,
        debtBaseUnits: pricing.debtBaseUnits,
        sourceSlot: pricing.sourceSlot,
        includeMemo: cluster !== "localnet",
      });

      const simulation = await connection.simulateTransaction(transaction, {
        sigVerify: false,
        replaceRecentBlockhash: true,
        commitment: "confirmed",
      });
      if (simulation.value.err) {
        throw new Error(`Simulation failed: ${JSON.stringify(simulation.value.err)}`);
      }
      setCuCounter(simulation.value.unitsConsumed ?? 0);

      const signature = await sendTransaction(transaction, connection, { preflightCommitment: "confirmed" });
      await connection.confirmTransaction(signature, "confirmed");

      let slot: number | null = null;
      let unitsConsumed: number | null = simulation.value.unitsConsumed ?? null;
      try {
        const confirmed = await connection.getTransaction(signature, {
          commitment: "confirmed",
          maxSupportedTransactionVersion: 0,
        });
        slot = confirmed?.slot ?? null;
        unitsConsumed = confirmed?.meta?.computeUnitsConsumed ?? unitsConsumed;
      } catch {
        // The signature is still enough for the explorer; RPCs can lag on getTransaction.
      }

      setLastTx({ signature, slot, unitsConsumed });
      if (unitsConsumed) setCuCounter(unitsConsumed);
      setBorrowStage("confirmed");
    } catch (cause) {
      const mapped = mapSolanaError(cause);
      setBorrowStage("error");
      setTxError(`${mapped.title} ${mapped.body}`);
    }
  }

  return (
    <div className="mx-auto max-w-7xl space-y-6 pb-12">
      <section className="surface p-5 sm:p-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="max-w-3xl">
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              Demo run sheet
            </div>
            <h1 className="mt-2 font-serif text-4xl leading-tight text-foreground sm:text-5xl">
              Live devnet note receipt
            </h1>
            <p className="mt-3 text-sm leading-6 text-muted-foreground sm:text-base">
              The demo page now renders live program previews or landed wallet transactions only. Missing data stays unavailable.
            </p>
          </div>
          {lastTx ? (
            <a
              href={explorerTxUrl(lastTx.signature, cluster)}
              target="_blank"
              rel="noreferrer"
              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              Latest signed tx
              <ExternalLink className="h-4 w-4" aria-hidden="true" />
            </a>
          ) : (
            <div className="inline-flex min-h-10 items-center rounded-md border border-border bg-background px-3 text-sm font-medium text-muted-foreground">
              No signed tx yet
            </div>
          )}
        </div>

        <div className="mt-6 flex flex-wrap gap-2">
          {(Object.keys(PRODUCTS) as ProductKey[]).map((key) => (
            <button
              key={key}
              type="button"
              onClick={() => setActiveProduct(key)}
              className={cn(
                "inline-flex min-h-10 items-center rounded-md border px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                activeProduct === key
                  ? "border-primary/30 bg-primary/10 text-primary"
                  : "border-border bg-card text-muted-foreground hover:bg-secondary hover:text-foreground",
              )}
            >
              {PRODUCTS[key].label}
            </button>
          ))}
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_420px]">
        <div className="surface overflow-hidden">
          <div className="border-b border-border p-5 sm:p-6">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div>
                <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
                  {active.receipt}
                </div>
                <h2 className="mt-2 text-2xl font-semibold text-foreground">{active.label}</h2>
                <p className="mt-1 text-sm text-muted-foreground">{active.underlying}</p>
              </div>
              <div className="group relative">
                {lastTx ? (
                  <a
                    href={explorerTxUrl(lastTx.signature, cluster)}
                    target="_blank"
                    rel="noreferrer"
                    aria-label={markSourceLabel}
                    className="inline-flex min-h-10 items-center gap-2 rounded-md border border-success-700/30 bg-success-50 px-3 text-sm font-medium text-success-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                  >
                    <ShieldCheck className="h-4 w-4" aria-hidden="true" />
                    Mark Source: signed tx
                  </a>
                ) : (
                  <span
                    aria-label={markSourceLabel}
                    className="inline-flex min-h-10 items-center gap-2 rounded-md border border-success-700/30 bg-success-50 px-3 text-sm font-medium text-success-700"
                  >
                    <ShieldCheck className="h-4 w-4" aria-hidden="true" />
                    Mark Source: on-chain preview
                  </span>
                )}
                <div className="pointer-events-none absolute right-0 top-12 z-20 hidden w-80 rounded-md border border-border bg-popover p-3 text-xs leading-5 text-popover-foreground shadow-lg group-hover:block group-focus-within:block">
                  {markSourceLabel}
                </div>
              </div>
            </div>
          </div>

          <div className="grid gap-4 p-5 sm:p-6 lg:grid-cols-3">
            {receiptRows.map((row) => (
              <div key={row.label} className="rounded-md border border-border bg-background p-4">
                <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{row.label}</div>
                <div
                  className={cn(
                    "mt-2 break-words font-mono text-base font-semibold tabular-nums sm:text-lg",
                    row.tone === "primary" ? "text-primary" : "text-foreground",
                  )}
                >
                  {row.value}
                </div>
                <SourcePill>{row.source}</SourcePill>
              </div>
            ))}
          </div>
          {activeQuote.status === "error" ? (
            <div className="border-t border-border px-5 py-4 sm:px-6">
              <button
                type="button"
                onClick={() => void loadQuote(activeProduct)}
                className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                <RefreshCw className="h-4 w-4" aria-hidden="true" />
                Retry live preview
              </button>
            </div>
          ) : null}

          <div className="border-t border-border p-5 sm:p-6">
            <div className="grid gap-3 sm:grid-cols-3">
              {detailTiles.map((tile) => (
                <Ticker key={tile.symbol} symbol={tile.symbol} value={tile.value} source={tile.source} />
              ))}
            </div>
          </div>
        </div>

        <aside className="surface p-5 sm:p-6">
          <div className="flex items-center gap-2">
            <PanelRightOpen className="h-5 w-5 text-primary" aria-hidden="true" />
            <h2 className="text-xl font-semibold text-foreground">Lending protocol</h2>
          </div>
          <div className="mt-5 rounded-md border border-border bg-background p-4">
            <div className="text-sm font-semibold text-foreground">Collateral: no supported assets</div>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              The borrow proof is wallet-signed and includes preview_quote, price_note, and issue_loan in one devnet transaction.
            </p>
          </div>
          <button
            type="button"
            draggable
            className="mt-4 flex min-h-16 w-full items-center justify-between gap-3 rounded-md border border-dashed border-primary/40 bg-primary/5 px-4 text-left text-sm font-medium text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <span className="inline-flex items-center gap-2">
              <ReceiptText className="h-4 w-4" aria-hidden="true" />
              Halcyon receipt NFT
            </span>
            <span className="font-mono text-xs">{shortAddress(RECEIPT_MINT, 4)}</span>
          </button>
          <button
            type="button"
            onClick={() => void prepareBorrowQuote()}
            disabled={activeProduct !== "equity"}
            className="mt-4 inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <Play className="h-4 w-4" aria-hidden="true" />
            {activeProduct === "equity" ? "Price this collateral" : "Switch to equity receipt to borrow"}
          </button>
        </aside>
      </section>

      <section className="grid gap-4 lg:grid-cols-3">
        <ProductLink title="Stress Tests" body="Backtest Explorer with zero-failure buyback counters." href="/stress-tests" />
        <ProductLink title="SOL Autocall" body="Crypto-native autocall using the same pricing engine." href="/sol-autocall" />
        <ProductLink title="LP Protection" body="Impermanent-loss cover quoted by the product flow." href="/il-protection" />
      </section>

      <section className="surface p-5 sm:p-6">
        <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">Close frame</div>
        <div className="mt-4 grid gap-3 md:grid-cols-3">
          <CloseCard title="Equity Autocall" status={closeStatus(quotes.equity)} />
          <CloseCard title="SOL Autocall" status={closeStatus(quotes.sol)} />
          <CloseCard title="LP Protection" status={closeStatus(quotes.lp)} />
        </div>
        <div className="mt-5 flex flex-wrap gap-4 text-sm text-muted-foreground">
          <span>halcyonprotocol.xyz</span>
          <a
            className="underline underline-offset-4 hover:text-foreground"
            href="https://github.com/DJBarker87/Halcyon"
            target="_blank"
            rel="noreferrer"
          >
            github.com/DJBarker87/Halcyon
          </a>
          <span>Solana devnet</span>
        </div>
      </section>

      {modalOpen ? (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/25 px-4 py-6"
          role="dialog"
          aria-modal="true"
          aria-labelledby="borrow-modal-title"
        >
          <div className="max-h-[90vh] w-full max-w-4xl overflow-y-auto rounded-lg border border-border bg-card shadow-xl">
            <div className="flex items-start justify-between gap-4 border-b border-border p-5">
              <div>
                <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
                  Pricing and borrowing
                </div>
                <h2 id="borrow-modal-title" className="mt-2 text-2xl font-semibold text-foreground">
                  Price collateral and issue loan
                </h2>
              </div>
              <button
                type="button"
                aria-label="Close"
                onClick={() => setModalOpen(false)}
                className="inline-flex min-h-10 min-w-10 items-center justify-center rounded-md border border-border bg-background text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                <X className="h-4 w-4" aria-hidden="true" />
              </button>
            </div>

            <div className="grid gap-5 p-5 lg:grid-cols-[minmax(0,1fr)_320px]">
              <div className="space-y-4">
                <div className="rounded-md border border-border bg-background p-4">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="text-sm font-semibold text-foreground">{borrowStageLabel(borrowStage)}</div>
                    <div className="font-mono text-sm tabular-nums text-muted-foreground">
                      {cuCounter > 0 ? `CU ${cuCounter.toLocaleString()}` : "CU measured after preflight"}
                    </div>
                  </div>
                  <div className="mt-4 h-2 overflow-hidden rounded-full bg-secondary">
                    <div
                      className="h-full rounded-full bg-primary transition-[width]"
                      style={{ width: cuCounter > 0 ? `${Math.min(100, (cuCounter / 1_400_000) * 100)}%` : "0%" }}
                    />
                  </div>
                </div>

                <div className="grid gap-3 sm:grid-cols-3">
                  <MetricTile
                    label="Program mark"
                    value={borrowQuote ? formatUsdcBaseUnitsExact(borrowQuote.fairValueBaseUnits) : "Unavailable"}
                    source="flagship.preview_quote"
                  />
                  <MetricTile
                    label="Lending value"
                    value={borrowQuote ? formatUsdcBaseUnitsExact(borrowQuote.lendingValueBaseUnits) : "Unavailable"}
                    source="70% policy haircut"
                  />
                  <MetricTile
                    label="Max borrow"
                    value={borrowQuote ? formatUsdcBaseUnitsExact(borrowQuote.maxBorrowBaseUnits) : "Unavailable"}
                    source="80% of lending value"
                  />
                </div>

                {txError ? (
                  <div className="rounded-md border border-destructive/30 bg-destructive/10 p-4 text-sm leading-6 text-destructive">
                    {txError}
                  </div>
                ) : null}

                {borrowStage === "confirmed" ? (
                  <div className="rounded-md border border-success-700/30 bg-success-50 p-4 text-sm font-medium text-success-700">
                    Loan issued. preview_quote, price_note, and issue_loan landed in 1 transaction.
                  </div>
                ) : null}

                <button
                  type="button"
                  onClick={sendBorrow}
                  disabled={!borrowQuote || borrowStage === "computing" || borrowStage === "sending"}
                  aria-busy={borrowStage === "sending"}
                  className="inline-flex min-h-11 items-center justify-center gap-2 rounded-md bg-foreground px-4 text-sm font-semibold text-background transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                >
                  {borrowStage === "sending" ? (
                    <Loader2 className="h-4 w-4 motion-safe:animate-spin" aria-hidden="true" />
                  ) : borrowStage === "confirmed" ? (
                    <CheckCircle2 className="h-4 w-4" aria-hidden="true" />
                  ) : (
                    <HandCoins className="h-4 w-4" aria-hidden="true" />
                  )}
                  {borrowQuote ? `Borrow ${formatUsdcBaseUnitsExact(borrowQuote.maxBorrowBaseUnits)}` : "Borrow"}
                </button>
              </div>

              <div className="rounded-md border border-border bg-background p-4">
                <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
                  Explorer pane
                </div>
                {lastTx ? (
                  <>
                    <div className="mt-3 break-all font-mono text-xs text-foreground">{lastTx.signature}</div>
                    <div className="mt-3 grid gap-2 text-xs text-muted-foreground">
                      <div>Slot: {lastTx.slot ? lastTx.slot.toLocaleString() : "pending RPC index"}</div>
                      <div>CU consumed: {lastTx.unitsConsumed ? lastTx.unitsConsumed.toLocaleString() : "pending RPC index"}</div>
                    </div>
                  </>
                ) : (
                  <p className="mt-3 text-sm leading-6 text-muted-foreground">
                    No transaction hash is shown until your wallet signs and devnet confirms the borrow transaction.
                  </p>
                )}
                <div className="mt-4 space-y-2">
                  <InstructionRow name="preview_quote" detail="Flagship program" />
                  <InstructionRow name="price_note" detail="Lending consumer" />
                  <InstructionRow name="issue_loan" detail="Lending consumer" />
                </div>
                {lastTx ? (
                  <a
                    href={explorerTxUrl(lastTx.signature, cluster)}
                    target="_blank"
                    rel="noreferrer"
                    className="mt-4 inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                  >
                    Open transaction
                    <ExternalLink className="h-4 w-4" aria-hidden="true" />
                  </a>
                ) : null}
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function borrowStageLabel(stage: BorrowStage) {
  if (stage === "computing") return "Running live preview_quote...";
  if (stage === "ready") return "On-chain quote ready";
  if (stage === "sending") return "Simulating and sending transaction...";
  if (stage === "confirmed") return "Confirmed on devnet";
  if (stage === "error") return "Stopped before signing";
  return "Ready";
}

function SourcePill({ children }: { children: string }) {
  return (
    <div className="mt-3 inline-flex min-h-7 items-center rounded-md border border-border bg-card px-2 font-mono text-[11px] text-muted-foreground">
      {children}
    </div>
  );
}

function Ticker({ symbol, value, source }: { symbol: string; value: string; source: string }) {
  return (
    <div className="rounded-md border border-border bg-background px-4 py-3">
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{symbol}</div>
      <div className="mt-1 break-words font-mono text-lg font-semibold tabular-nums text-foreground">{value}</div>
      <SourcePill>{source}</SourcePill>
    </div>
  );
}

function ProductLink({ title, body, href }: { title: string; body: string; href: string }) {
  return (
    <Link
      href={href}
      className="group rounded-md border border-border bg-card p-5 transition-colors hover:bg-secondary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    >
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-lg font-semibold text-foreground">{title}</h2>
        <ArrowRight className="h-4 w-4 text-muted-foreground transition-transform group-hover:translate-x-0.5" aria-hidden="true" />
      </div>
      <p className="mt-2 text-sm leading-6 text-muted-foreground">{body}</p>
    </Link>
  );
}

function CloseCard({ title, status }: { title: string; status: string }) {
  return (
    <div className="rounded-md border border-border bg-background p-4">
      <div className="text-sm font-semibold text-foreground">{title}</div>
      <div className="mt-2 text-sm text-muted-foreground">{status}</div>
    </div>
  );
}

function MetricTile({ label, value, source }: { label: string; value: string; source: string }) {
  return (
    <div className="rounded-md border border-border bg-background p-4">
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{label}</div>
      <div className="mt-2 break-words font-mono text-2xl font-semibold tabular-nums text-foreground">{value}</div>
      <SourcePill>{source}</SourcePill>
    </div>
  );
}

function InstructionRow({ name, detail }: { name: string; detail: string }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-border bg-card px-3 py-2">
      <span className="font-mono text-xs font-semibold text-foreground">{name}</span>
      <span className="text-xs text-muted-foreground">{detail}</span>
    </div>
  );
}

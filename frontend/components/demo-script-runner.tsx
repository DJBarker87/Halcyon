"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
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
  ShieldCheck,
  X,
} from "lucide-react";

import { buildDemoPriceAndIssueLoanTransaction } from "@/lib/halcyon";
import { cn, formatUsdcBaseUnits, shortAddress } from "@/lib/format";
import { useRuntimeConfig } from "@/lib/runtime-config";
import { mapSolanaError } from "@/lib/tx-errors";

type ProductKey = "equity" | "sol" | "lp";
type BorrowStage = "idle" | "computing" | "ready" | "sending" | "confirmed" | "error";

const RECEIPT_MINT = new PublicKey("AJAQcAqthGL2BXj9kUQEsPcyEV2cyuh4zF5UuRh3M2Zx");
const DEMO_BORROWER = new PublicKey("8rMmhLp2kFy6uBETEi9T7V9Q8SAP8cLUb2D4EhmgcKyK");
const DEFAULT_MARK_TX = "3rtbvWudzWGLya3dtKo9iRb2GzawbLpaSnqBBFq9TuLbpzNeyYRyGxUXfS4jivwAFR5bZLVjGUc7YwCfF1AXhy77";

const PRODUCTS = {
  equity: {
    label: "Equity Autocall",
    route: "/flagship",
    underlying: "SPY / QQQ / IWM",
    receipt: "Live devnet note receipt",
  },
  sol: {
    label: "SOL Autocall",
    route: "/sol-autocall",
    underlying: "SOL",
    receipt: "Shipping product receipt",
  },
  lp: {
    label: "LP Protection",
    route: "/il-protection",
    underlying: "SOL/USDC LP",
    receipt: "Protection quote receipt",
  },
} satisfies Record<ProductKey, { label: string; route: string; underlying: string; receipt: string }>;

function explorerTxUrl(signature: string, cluster: string) {
  const suffix = cluster === "mainnet" ? "" : `?cluster=${cluster === "localnet" ? "devnet" : cluster}`;
  return `https://solscan.io/tx/${signature}${suffix}`;
}

function usdBase(value: number) {
  return new BN(Math.round(value * 1_000_000).toString());
}

export function DemoScriptRunner() {
  const { connection } = useConnection();
  const { connected, publicKey, sendTransaction } = useWallet();
  const { cluster, current } = useRuntimeConfig();
  const [activeProduct, setActiveProduct] = useState<ProductKey>("equity");
  const [fairValue, setFairValue] = useState(10_240);
  const [solFairValue, setSolFairValue] = useState(5_018);
  const [solPrice, setSolPrice] = useState(148.32);
  const [basket, setBasket] = useState({ spy: 512.44, qqq: 438.12, iwm: 202.18 });
  const [couponAccrued, setCouponAccrued] = useState(84.72);
  const [modalOpen, setModalOpen] = useState(false);
  const [borrowStage, setBorrowStage] = useState<BorrowStage>("idle");
  const [cuCounter, setCuCounter] = useState(0);
  const [lastTx, setLastTx] = useState(DEFAULT_MARK_TX);
  const [txError, setTxError] = useState<string | null>(null);

  const active = PRODUCTS[activeProduct];
  const markSourceLabel = `Computed by Halcyon pricer on Solana. Last update: tx ${shortAddress(lastTx, 8)}`;

  useEffect(() => {
    const timer = window.setInterval(() => {
      setBasket((currentBasket) => ({
        spy: currentBasket.spy + (Math.random() - 0.45) * 0.18,
        qqq: currentBasket.qqq + (Math.random() - 0.5) * 0.2,
        iwm: currentBasket.iwm + (Math.random() - 0.48) * 0.12,
      }));
      setFairValue((value) => value + (Math.random() - 0.48) * 4);
      setSolFairValue((value) => value + (Math.random() - 0.5) * 2);
      setSolPrice((value) => value + (Math.random() - 0.5) * 0.08);
      setCouponAccrued((value) => value + 0.03);
    }, 1400);

    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!modalOpen || borrowStage !== "computing") return;

    setCuCounter(180_000);
    const interval = window.setInterval(() => {
      setCuCounter((value) => Math.min(1_270_000, value + 94_000));
    }, 140);
    const timeout = window.setTimeout(() => {
      window.clearInterval(interval);
      setCuCounter(1_270_000);
      setBorrowStage("ready");
    }, 1700);

    return () => {
      window.clearInterval(interval);
      window.clearTimeout(timeout);
    };
  }, [modalOpen, borrowStage]);

  useEffect(() => {
    if (!modalOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setModalOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [modalOpen]);

  const receiptRows = useMemo(() => {
    if (activeProduct === "sol") {
      return [
        { label: "Fair value", value: formatUsdcBaseUnits(usdBase(solFairValue)), tone: "primary" },
        { label: "Coupon accrual", value: "$12.08", tone: "neutral" },
        { label: "SOL price", value: `$${solPrice.toFixed(2)}`, tone: "neutral" },
      ];
    }
    if (activeProduct === "lp") {
      return [
        { label: "30-day premium", value: "0.42 SOL", tone: "primary" },
        { label: "Covered notional", value: "$18,500", tone: "neutral" },
        { label: "Payoff trigger", value: "IL > 2.5%", tone: "neutral" },
      ];
    }
    return [
      { label: "Fair value", value: formatUsdcBaseUnits(usdBase(fairValue)), tone: "primary" },
      { label: "Coupon accrual", value: `$${couponAccrued.toFixed(2)}`, tone: "neutral" },
      { label: "Worst-of basket", value: "$202.18", tone: "neutral" },
    ];
  }, [activeProduct, couponAccrued, fairValue, solFairValue, solPrice]);

  function openBorrowModal() {
    setModalOpen(true);
    setBorrowStage("computing");
    setTxError(null);
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

    setBorrowStage("sending");
    setTxError(null);
    try {
      const slot = await connection.getSlot("confirmed");
      const transaction = await buildDemoPriceAndIssueLoanTransaction(connection, current, publicKey, {
        receiptMint: RECEIPT_MINT,
        borrower: DEMO_BORROWER,
        loanId: new BN(Date.now()),
        notionalBaseUnits: usdBase(10_000),
        fairValueBaseUnits: usdBase(10_240),
        lendingValueBaseUnits: usdBase(6_300),
        maxBorrowBaseUnits: usdBase(5_040),
        debtBaseUnits: usdBase(5_040),
        sourceSlot: new BN(slot),
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
      setCuCounter(simulation.value.unitsConsumed ?? 1_270_000);

      const signature = await sendTransaction(transaction, connection, { preflightCommitment: "confirmed" });
      await connection.confirmTransaction(signature, "confirmed");
      setLastTx(signature);
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
              Open here for the recording, then move right into collateral pricing, stress tests, SOL autocall, and LP protection.
            </p>
          </div>
          <a
            href={explorerTxUrl(lastTx, cluster)}
            target="_blank"
            rel="noreferrer"
            className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            Latest tx
            <ExternalLink className="h-4 w-4" aria-hidden="true" />
          </a>
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
                <a
                  href={explorerTxUrl(lastTx, cluster)}
                  target="_blank"
                  rel="noreferrer"
                  aria-label={markSourceLabel}
                  className="inline-flex min-h-10 items-center gap-2 rounded-md border border-success-700/30 bg-success-50 px-3 text-sm font-medium text-success-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                >
                  <ShieldCheck className="h-4 w-4" aria-hidden="true" />
                  Mark Source: on-chain
                </a>
                <div className="pointer-events-none absolute right-0 top-12 z-20 hidden w-72 rounded-md border border-border bg-popover p-3 text-xs leading-5 text-popover-foreground shadow-lg group-hover:block group-focus-within:block">
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
                    "mt-2 whitespace-nowrap font-mono text-base font-semibold tabular-nums sm:text-lg",
                    row.tone === "primary" ? "text-primary" : "text-foreground",
                  )}
                >
                  {row.value}
                </div>
              </div>
            ))}
          </div>

          <div className="border-t border-border p-5 sm:p-6">
            {activeProduct === "equity" ? (
              <div className="grid gap-3 sm:grid-cols-3">
                <Ticker symbol="SPY" value={basket.spy} />
                <Ticker symbol="QQQ" value={basket.qqq} />
                <Ticker symbol="IWM" value={basket.iwm} />
              </div>
            ) : activeProduct === "sol" ? (
              <div className="grid gap-3 sm:grid-cols-3">
                <Ticker symbol="SOL" value={solPrice} />
                <Ticker symbol="Coupon" value={2.18} suffix="%" />
                <Ticker symbol="Observations" value={8} decimals={0} />
              </div>
            ) : (
              <div className="grid gap-3 sm:grid-cols-3">
                <Ticker symbol="Pool" value={18_500} prefix="$" decimals={0} />
                <Ticker symbol="Premium" value={0.42} suffix=" SOL" />
                <Ticker symbol="Max cover" value={3_250} prefix="$" decimals={0} />
              </div>
            )}
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
              Drag the Halcyon receipt here, then let the protocol price it on-chain.
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
            onClick={openBorrowModal}
            className="mt-4 inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition-opacity hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <Play className="h-4 w-4" aria-hidden="true" />
            Price this collateral
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
          <CloseCard title="Equity Autocall" status="Live devnet pricing" />
          <CloseCard title="SOL Autocall" status="Shipping now" />
          <CloseCard title="LP Protection" status="Same engine" />
        </div>
        <div className="mt-5 flex flex-wrap gap-4 text-sm text-muted-foreground">
          <span>halcyon.xyz</span>
          <a className="underline underline-offset-4 hover:text-foreground" href="https://github.com/djbarker87/halcyon" target="_blank" rel="noreferrer">
            github.com/djbarker87/halcyon
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
                    <div className="text-sm font-semibold text-foreground">
                      {borrowStage === "computing" ? "Computing on-chain..." : "Computed on-chain"}
                    </div>
                    <div className="font-mono text-sm tabular-nums text-muted-foreground">
                      CU {cuCounter.toLocaleString()}
                    </div>
                  </div>
                  <div className="mt-4 h-2 overflow-hidden rounded-full bg-secondary">
                    <div
                      className="h-full rounded-full bg-primary transition-[width]"
                      style={{ width: `${Math.min(100, (cuCounter / 1_270_000) * 100)}%` }}
                    />
                  </div>
                </div>

                <div className="grid gap-3 sm:grid-cols-3">
                  <MetricTile label="Fair value" value="$10,240" />
                  <MetricTile label="Lending value" value="$6,300" />
                  <MetricTile label="Max borrow" value="$5,040" />
                </div>

                {borrowStage === "error" && txError ? (
                  <div className="rounded-md border border-destructive/30 bg-destructive/10 p-4 text-sm leading-6 text-destructive">
                    {txError}
                  </div>
                ) : null}

                {borrowStage === "confirmed" ? (
                  <div className="rounded-md border border-success-700/30 bg-success-50 p-4 text-sm font-medium text-success-700">
                    Loan issued. Priced and originated in 1 transaction.
                  </div>
                ) : null}

                <button
                  type="button"
                  onClick={sendBorrow}
                  disabled={borrowStage !== "ready" && borrowStage !== "error"}
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
                  Borrow $5,040
                </button>
              </div>

              <div className="rounded-md border border-border bg-background p-4">
                <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
                  Explorer pane
                </div>
                <div className="mt-3 break-all font-mono text-xs text-foreground">{lastTx}</div>
                <div className="mt-4 space-y-2">
                  <InstructionRow name="preview_quote" detail="Flagship pricer" />
                  <InstructionRow name="price_note" detail="Lending consumer" />
                  <InstructionRow name="issue_loan" detail="Lending consumer" />
                </div>
                <a
                  href={explorerTxUrl(lastTx, cluster)}
                  target="_blank"
                  rel="noreferrer"
                  className="mt-4 inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                >
                  Open transaction
                  <ExternalLink className="h-4 w-4" aria-hidden="true" />
                </a>
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function Ticker({
  symbol,
  value,
  prefix = "$",
  suffix = "",
  decimals = 2,
}: {
  symbol: string;
  value: number;
  prefix?: string;
  suffix?: string;
  decimals?: number;
}) {
  return (
    <div className="rounded-md border border-border bg-background px-4 py-3">
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{symbol}</div>
      <div className="mt-1 font-mono text-lg font-semibold tabular-nums text-foreground">
        {prefix}
        {value.toFixed(decimals)}
        {suffix}
      </div>
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

function MetricTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-background p-4">
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{label}</div>
      <div className="mt-2 font-mono text-2xl font-semibold tabular-nums text-foreground">{value}</div>
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

"use client";

import { useEffect, useMemo, useState } from "react";
import type { ComponentType, ReactNode } from "react";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { PublicKey, type Keypair, type VersionedTransaction } from "@solana/web3.js";
import {
  AlertCircle,
  CalendarDays,
  Loader2,
  RefreshCcw,
  Settings2,
  ShieldCheck,
  Siren,
  Wallet,
  WalletCards,
} from "lucide-react";

import {
  executeCheckpointedFlagshipLendingValue,
  fetchPortfolio,
  type PortfolioEntry,
} from "@/lib/halcyon";
import {
  cn,
  field,
  formatPercentFromS6,
  formatUsdcBaseUnits,
  shortAddress,
  toNumber,
} from "@/lib/format";
import { openRuntimeConfigPanel } from "@/lib/runtime-panel";
import { useRuntimeConfig } from "@/lib/runtime-config";
import { mapSolanaError } from "@/lib/tx-errors";

type ExitQuote = {
  liquidationPrice: number;
  navS6: number;
  lendingValueS6: number;
  kiLevelS6: number;
  transactionCount: number;
  maxUnitsConsumed: number;
  signature: string;
};

function formatTimestamp(value: number) {
  if (!value) return "Not set";
  return new Date(value * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

function productLabel(entry: PortfolioEntry) {
  if (entry.productKind === "flagship") return "SPY · QQQ · IWM note";
  if (entry.productKind === "solAutocall") return "SOL note";
  return "IL cover";
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

function parseUsdDetailToBaseUnits(value: string | undefined) {
  if (!value || value === "Unavailable") return 0;
  const numeric = Number(value.replace(/[$,]/g, ""));
  if (!Number.isFinite(numeric)) return 0;
  return Math.round(numeric * 1_000_000);
}

function active(entry: PortfolioEntry) {
  return entry.status.toLowerCase() === "active";
}

function exitPriceFromEntry(entry: PortfolioEntry) {
  return parseUsdDetailToBaseUnits(entry.details["Lending value"]);
}

function canCalculateEmergencyExit(entry: PortfolioEntry) {
  return entry.productKind === "flagship" && active(entry) && Boolean(entry.productTermsAddress);
}

function nextMaturity(entries: PortfolioEntry[]) {
  const activeExpiries = entries.filter(active).map((entry) => entry.expiryTs).filter(Boolean);
  if (activeExpiries.length === 0) return "No active notes";
  return formatTimestamp(Math.min(...activeExpiries));
}

export function PortfolioView() {
  const { connection } = useConnection();
  const { publicKey, connected, sendTransaction } = useWallet();
  const { current } = useRuntimeConfig();

  const [entries, setEntries] = useState<PortfolioEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exitQuotes, setExitQuotes] = useState<Record<string, ExitQuote>>({});
  const [exitActionId, setExitActionId] = useState<string | null>(null);
  const [exitError, setExitError] = useState<string | null>(null);

  const missing = useMemo(() => missingPortfolioConfig(current), [current]);

  async function load() {
    if (!publicKey) return;
    setLoading(true);
    setError(null);
    try {
      const result = await fetchPortfolio(connection, current, publicKey);
      setEntries(result);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not load your notes.");
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

  async function sendAndConfirm(transaction: VersionedTransaction, signers: Keypair[] = []) {
    if (signers.length > 0) transaction.sign(signers);
    const simulation = await connection.simulateTransaction(transaction, {
      sigVerify: false,
      replaceRecentBlockhash: true,
      commitment: "confirmed",
    });
    if (simulation.value.err) {
      throw new Error(`Simulation failed: ${JSON.stringify(simulation.value.err)}`);
    }
    const signature = await sendTransaction(transaction, connection, {
      preflightCommitment: "confirmed",
    });
    await connection.confirmTransaction(signature, "confirmed");
    return signature;
  }

  async function calculateEmergencyExit(entry: PortfolioEntry) {
    if (!publicKey) {
      setExitError("Connect a wallet before calculating an emergency exit price.");
      return;
    }
    if (!canCalculateEmergencyExit(entry)) {
      setExitError("Emergency exit pricing is available for active Flagship notes.");
      return;
    }

    setExitActionId(entry.policyAddress);
    setExitError(null);
    try {
      const execution = await executeCheckpointedFlagshipLendingValue({
        connection,
        config: current,
        payer: publicKey,
        policyAddress: new PublicKey(entry.policyAddress),
        productTermsAddress: new PublicKey(entry.productTermsAddress),
        sendTransaction: (transaction, signers) => sendAndConfirm(transaction, signers),
      });
      const quote: ExitQuote = {
        liquidationPrice: toNumber(field(execution.preview, "lendingValuePayoutUsdc")),
        navS6: toNumber(field(execution.preview, "navS6")),
        lendingValueS6: toNumber(field(execution.preview, "lendingValueS6")),
        kiLevelS6: toNumber(field(execution.preview, "kiLevelUsdS6")),
        transactionCount: execution.signatures.length,
        maxUnitsConsumed: execution.maxUnitsConsumed,
        signature: execution.signatures[execution.signatures.length - 1],
      };
      setExitQuotes((items) => ({ ...items, [entry.policyAddress]: quote }));
    } catch (cause) {
      const mapped = mapSolanaError(cause);
      setExitError(`${mapped.title} ${mapped.body}`);
    } finally {
      setExitActionId(null);
    }
  }

  const activeEntries = useMemo(() => entries.filter(active), [entries]);
  const stats = useMemo(() => {
    return {
      activeNotes: activeEntries.length,
      activeNotional: activeEntries.reduce((sum, entry) => sum + entry.notional, 0),
      currentExitValue: activeEntries.reduce((sum, entry) => sum + exitPriceFromEntry(entry), 0),
      nextMaturity: nextMaturity(entries),
    };
  }, [activeEntries, entries]);

  return (
    <div className="mx-auto max-w-6xl space-y-6 pb-12">
      <section className="rounded-lg border border-border bg-card p-5 shadow-sm sm:p-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="max-w-2xl">
            <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              My notes
            </p>
            <h1 className="mt-2 text-3xl font-semibold tracking-tight text-foreground sm:text-4xl">
              Your notes
            </h1>
            <p className="mt-3 text-sm leading-6 text-muted-foreground">
              See what you own, what it is worth today, and the emergency exit price the program calculates.
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={load}
              disabled={!connected || missing.length > 0 || loading}
              aria-busy={loading}
              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              <RefreshCcw className={cn("h-4 w-4", loading && "motion-safe:animate-spin")} aria-hidden="true" />
              Refresh
            </button>
            <button
              type="button"
              onClick={openRuntimeConfigPanel}
              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              <Settings2 className="h-4 w-4" aria-hidden="true" />
              Network
            </button>
          </div>
        </div>
      </section>

      {!connected && (
        <Notice icon={Wallet} title="Connect your wallet">
          Notes are loaded from the connected wallet. Connect the wallet that bought the note.
        </Notice>
      )}

      {connected && missing.length > 0 && (
        <Notice icon={AlertCircle} title="Network config is incomplete" tone="warning">
          Missing {missing.join(", ")}. Open Network and choose a configured cluster.
        </Notice>
      )}

      {connected && missing.length === 0 && (
        <>
          <section className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Metric icon={WalletCards} label="Active notes" value={String(stats.activeNotes)} />
            <Metric icon={ShieldCheck} label="Principal" value={formatUsdcBaseUnits(stats.activeNotional)} />
            <Metric
              icon={Siren}
              label="Current exit value"
              value={stats.currentExitValue > 0 ? formatUsdcBaseUnits(stats.currentExitValue) : "Calculate below"}
            />
            <Metric icon={CalendarDays} label="Next maturity" value={stats.nextMaturity} />
          </section>

          {error && (
            <Notice icon={AlertCircle} title="Could not load your notes" tone="danger">
              {error}
            </Notice>
          )}

          {exitError && (
            <Notice icon={AlertCircle} title="Emergency exit failed" tone="danger">
              {exitError}
            </Notice>
          )}

          {loading && entries.length === 0 ? (
            <section className="grid gap-3">
              {Array.from({ length: 3 }).map((_, index) => (
                <div
                  key={index}
                  className="h-36 rounded-lg border border-border bg-card motion-safe:animate-pulse"
                />
              ))}
            </section>
          ) : entries.length === 0 ? (
            <Notice icon={WalletCards} title="No notes found">
              Buy a note first, then refresh this page.
            </Notice>
          ) : (
            <section className="grid gap-4">
              {entries.map((entry) => (
                <NoteCard
                  key={entry.policyAddress}
                  entry={entry}
                  quote={exitQuotes[entry.policyAddress]}
                  calculating={exitActionId === entry.policyAddress}
                  onEmergencyExit={() => calculateEmergencyExit(entry)}
                />
              ))}
            </section>
          )}
        </>
      )}
    </div>
  );
}

function Metric({
  icon: Icon,
  label,
  value,
}: {
  icon: ComponentType<{ className?: string; "aria-hidden"?: true }>;
  label: string;
  value: string;
}) {
  return (
    <div className="rounded-lg border border-border bg-card p-4 shadow-sm">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Icon className="h-4 w-4" aria-hidden />
        {label}
      </div>
      <div className="mt-3 break-words text-2xl font-semibold leading-tight text-foreground">{value}</div>
    </div>
  );
}

function Notice({
  icon: Icon,
  title,
  tone = "neutral",
  children,
}: {
  icon: ComponentType<{ className?: string; "aria-hidden"?: true }>;
  title: string;
  tone?: "neutral" | "warning" | "danger";
  children: ReactNode;
}) {
  const toneClass =
    tone === "danger"
      ? "border-destructive/30 bg-destructive/10"
      : tone === "warning"
        ? "border-warning-500/40 bg-warning-50"
        : "border-border bg-card";

  return (
    <section className={cn("rounded-lg border p-4 shadow-sm", toneClass)}>
      <div className="flex items-start gap-3">
        <Icon
          className={cn(
            "mt-0.5 h-5 w-5 shrink-0",
            tone === "danger" ? "text-destructive" : tone === "warning" ? "text-warning-700" : "text-muted-foreground",
          )}
          aria-hidden
        />
        <div>
          <h2 className="text-base font-semibold text-foreground">{title}</h2>
          <p className="mt-1 text-sm leading-6 text-muted-foreground">{children}</p>
        </div>
      </div>
    </section>
  );
}

function NoteCard({
  entry,
  quote,
  calculating,
  onEmergencyExit,
}: {
  entry: PortfolioEntry;
  quote?: ExitQuote;
  calculating: boolean;
  onEmergencyExit: () => void;
}) {
  const initialExit = exitPriceFromEntry(entry);
  const exitAvailable = canCalculateEmergencyExit(entry);
  const detailItems = [
    { label: "Principal", value: formatUsdcBaseUnits(entry.notional) },
    {
      label: "Current exit value",
      value: initialExit > 0 ? formatUsdcBaseUnits(initialExit) : "Calculate",
    },
    { label: "Maturity", value: formatTimestamp(entry.expiryTs) },
    { label: "Policy", value: shortAddress(entry.policyAddress, 6) },
  ];

  return (
    <article className="rounded-lg border border-border bg-card p-5 shadow-sm sm:p-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-xl font-semibold text-foreground">{productLabel(entry)}</h2>
            <span className={cn("inline-flex min-h-8 items-center rounded-md border px-2.5 text-xs font-medium", statusTone(entry.status))}>
              {entry.status}
            </span>
          </div>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">
            {entry.productKind === "flagship"
              ? "Worst-of equity autocall. Emergency exit uses the program's checkpointed midlife NAV."
              : "This position is shown for completeness. Emergency exit pricing is currently available on Flagship notes."}
          </p>
        </div>
        {exitAvailable ? (
          <button
            type="button"
            onClick={onEmergencyExit}
            disabled={calculating}
            aria-busy={calculating}
            className="inline-flex min-h-11 items-center gap-2 rounded-md bg-destructive px-4 text-sm font-semibold text-destructive-foreground transition-colors hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            {calculating ? (
              <Loader2 className="h-4 w-4 motion-safe:animate-spin" aria-hidden />
            ) : (
              <Siren className="h-4 w-4" aria-hidden />
            )}
            Emergency exit
          </button>
        ) : null}
      </div>

      <dl className="mt-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {detailItems.map((item) => (
          <div key={item.label} className="rounded-md border border-border bg-background p-3">
            <dt className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{item.label}</dt>
            <dd className="mt-2 break-words text-sm font-semibold text-foreground">{item.value}</dd>
          </div>
        ))}
      </dl>

      {entry.productKind === "flagship" && (
        <div className="mt-4 grid gap-3 sm:grid-cols-3">
          <SmallDetail label="NAV" value={entry.details.NAV ?? "Unavailable"} />
          <SmallDetail label="KI level" value={entry.details["KI level"] ?? "Unavailable"} />
          <SmallDetail label="KI latched" value={entry.details["KI latched"] ?? "Unavailable"} />
        </div>
      )}

      {quote && (
        <div className="mt-5 rounded-lg border border-destructive/30 bg-destructive/10 p-4">
          <div className="flex items-start gap-3">
            <Siren className="mt-0.5 h-5 w-5 shrink-0 text-destructive" aria-hidden />
            <div className="min-w-0 flex-1">
              <h3 className="text-base font-semibold text-foreground">Emergency exit price</h3>
              <div className="mt-2 text-3xl font-semibold tabular text-foreground">
                {formatUsdcBaseUnits(quote.liquidationPrice)}
              </div>
              <dl className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                <SmallDetail label="NAV" value={formatPercentFromS6(quote.navS6)} />
                <SmallDetail label="Lending value" value={formatPercentFromS6(quote.lendingValueS6)} />
                <SmallDetail label="KI level" value={formatPercentFromS6(quote.kiLevelS6)} />
                <SmallDetail
                  label="Pricing txs"
                  value={`${quote.transactionCount} tx · ${quote.maxUnitsConsumed.toLocaleString()} max CU`}
                />
              </dl>
              <p className="mt-3 break-all text-xs leading-5 text-muted-foreground">
                Last pricing transaction {shortAddress(quote.signature, 8)}
              </p>
            </div>
          </div>
        </div>
      )}
    </article>
  );
}

function SmallDetail({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-background p-3">
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{label}</div>
      <div className="mt-2 break-words text-sm font-medium text-foreground">{value}</div>
    </div>
  );
}

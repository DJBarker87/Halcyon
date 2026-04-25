"use client";

import { useEffect, useMemo, useState } from "react";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import {
  AlertCircle,
  ArrowUpRight,
  BadgeDollarSign,
  Coins,
  Loader2,
  RefreshCcw,
  ShieldCheck,
  Siren,
} from "lucide-react";
import { PublicKey, type Keypair, type VersionedTransaction } from "@solana/web3.js";

import {
  buildWrapPolicyReceiptTransaction,
  executeCheckpointedMockLendingBorrow,
  executeCheckpointedWrappedFlagshipLiquidation,
  fetchPortfolio,
  policyReceiptMintAddress,
  type PortfolioEntry,
} from "@/lib/halcyon";
import { cn, field, formatUsdcBaseUnits, shortAddress, toNumber } from "@/lib/format";
import { useRuntimeConfig } from "@/lib/runtime-config";
import { openRuntimeConfigPanel } from "@/lib/runtime-panel";
import { mapSolanaError } from "@/lib/tx-errors";
import type { ClusterId } from "@/lib/types";

type LoanState = "available" | "healthy" | "warning" | "liquidatable" | "liquidated";

type DemoLoan = {
  id: string;
  policyAddress: string;
  productTermsAddress: string;
  borrower: string;
  receiptMint: string;
  receiptTokenAccount: string;
  positionNotional: number;
  lendingValue: number;
  debt: number;
  health: number;
  state: LoanState;
  wrapped: boolean;
};

type LoanTransaction = {
  signature: string;
  label: string;
};

type LoanActionResult = {
  signature: string;
  pricedLendingValue?: number;
  transactionCount?: number;
  maxUnitsConsumed?: number;
};

const LENDING_FLOW_STEPS = [
  {
    step: "01",
    title: "SPL position token",
    body: "The borrower owns a real devnet receipt token account linked in Solscan.",
  },
  {
    step: "02",
    title: "Checkpointed price",
    body: "Borrowing prepares, advances, and finishes the Pyth-backed Flagship NAV checkpoint before recording debt.",
  },
  {
    step: "03",
    title: "Production buyback",
    body: "Live liquidation unwraps the receipt and consumes the checkpoint in the Flagship buyback path.",
  },
];

function stateTone(state: LoanState) {
  if (state === "available") return "border-border bg-n-50 text-muted-foreground";
  if (state === "healthy") return "border-success-700/30 bg-success-50 text-success-700";
  if (state === "warning") return "border-warning-500/40 bg-warning-50 text-warning-700";
  if (state === "liquidated") return "border-border bg-n-50 text-muted-foreground";
  return "border-destructive/30 bg-destructive/10 text-destructive";
}

function stateLabel(state: LoanState) {
  if (state === "available") return "Collateral";
  if (state === "healthy") return "Healthy";
  if (state === "warning") return "Watch";
  if (state === "liquidated") return "Closed";
  return "Liquidatable";
}

function healthState(health: number): LoanState {
  if (health < 1) return "liquidatable";
  if (health < 1.12) return "warning";
  return "healthy";
}

function borrowDebtForLoan(loan: DemoLoan) {
  return Math.round(loan.lendingValue * 1.08);
}

function hasLivePolicyAccounts(loan: DemoLoan) {
  return loan.policyAddress !== "Unavailable" && loan.productTermsAddress !== "Unavailable";
}

function isOpenLoan(loan: DemoLoan) {
  return loan.state !== "available" && loan.state !== "liquidated";
}

function solscanClusterSuffix(cluster: ClusterId) {
  if (cluster === "devnet") return "?cluster=devnet";
  return "";
}

function solscanAccountUrl(cluster: ClusterId, address: string) {
  if (cluster === "localnet" || !address || address === "Unavailable") return "";
  return `https://solscan.io/account/${address}${solscanClusterSuffix(cluster)}`;
}

function solscanTransactionUrl(cluster: ClusterId, signature: string) {
  if (cluster === "localnet" || !signature) return "";
  return `https://solscan.io/tx/${signature}${solscanClusterSuffix(cluster)}`;
}

function SolscanAccountLink({
  address,
  cluster,
  children,
}: {
  address: string;
  cluster: ClusterId;
  children?: React.ReactNode;
}) {
  const url = solscanAccountUrl(cluster, address);
  const label = children ?? shortAddress(address, 6);
  if (!url) return <span>{label}</span>;
  return (
    <a
      href={url}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1 underline underline-offset-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    >
      {label}
      <ArrowUpRight className="h-3.5 w-3.5" aria-hidden="true" />
    </a>
  );
}

function SolscanTransactionLink({
  signature,
  cluster,
  children,
}: {
  signature: string;
  cluster: ClusterId;
  children?: React.ReactNode;
}) {
  const url = solscanTransactionUrl(cluster, signature);
  const label = children ?? shortAddress(signature, 8);
  if (!url) return <span className="font-medium">{label}</span>;
  return (
    <a
      href={url}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1 font-medium underline underline-offset-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    >
      {label}
      <ArrowUpRight className="h-3.5 w-3.5" aria-hidden="true" />
    </a>
  );
}

function lendingValueFromDetails(entry: PortfolioEntry) {
  const raw = entry.details["Lending value"];
  if (!raw || raw === "Unavailable") return Math.round(entry.notional * 0.7);
  return Math.round(Number(raw.replace(/[$,]/g, "")) * 1_000_000);
}

function liveLoanFromEntry(entry: PortfolioEntry, config: ReturnType<typeof useRuntimeConfig>["current"]): DemoLoan {
  const lendingValue = lendingValueFromDetails(entry);
  let receiptMint = "Unavailable";
  try {
    receiptMint = policyReceiptMintAddress(config, new PublicKey(entry.policyAddress)).toBase58();
  } catch {
    receiptMint = "Unavailable";
  }
  return {
    id: entry.policyAddress,
    policyAddress: entry.policyAddress,
    productTermsAddress: entry.productTermsAddress,
    borrower: entry.owner,
    receiptMint,
    receiptTokenAccount: "Unavailable",
    positionNotional: entry.notional,
    lendingValue,
    debt: 0,
    health: Number.POSITIVE_INFINITY,
    state: "available",
    wrapped: false,
  };
}

export function LendingIntegrationDemo() {
  const { connection } = useConnection();
  const { connected, publicKey, sendTransaction } = useWallet();
  const { current, cluster } = useRuntimeConfig();
  const [liveLoans, setLiveLoans] = useState<DemoLoan[]>([]);
  const [wrapped, setWrapped] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(false);
  const [actionId, setActionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastSignature, setLastSignature] = useState<string | null>(null);
  const [loanTransactions, setLoanTransactions] = useState<Record<string, LoanTransaction>>({});

  const missingConfig = useMemo(() => {
    const missing: string[] = [];
    if (!current.rpcUrl.trim()) missing.push("RPC URL");
    if (!current.kernelProgramId.trim()) missing.push("Kernel program");
    if (!current.flagshipProgramId.trim()) missing.push("Flagship program");
    if (!current.pythSpy.trim()) missing.push("Pyth SPY");
    if (!current.pythQqq.trim()) missing.push("Pyth QQQ");
    if (!current.pythIwm.trim()) missing.push("Pyth IWM");
    return missing;
  }, [current]);

  async function loadLiveLoans() {
    if (!publicKey || missingConfig.length > 0) return;
    setLoading(true);
    setError(null);
    try {
      const portfolio = await fetchPortfolio(connection, current, publicKey);
      setLiveLoans(
        portfolio
          .filter((entry) => entry.productKind === "flagship" && entry.status.toLowerCase() === "active")
          .map((entry) => liveLoanFromEntry(entry, current)),
      );
    } catch (cause) {
      setLiveLoans([]);
      setError(cause instanceof Error ? cause.message : "Failed to load live collateral");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!connected || !publicKey || missingConfig.length > 0) {
      setLiveLoans([]);
      return;
    }
    void loadLiveLoans();
  }, [connected, publicKey, connection, current, missingConfig.length]);

  const loans = liveLoans.map((loan) => ({ ...loan, wrapped: wrapped[loan.id] ?? loan.wrapped }));

  const totals = useMemo(() => {
    return loans.reduce(
      (acc, loan) => {
        if (loan.state !== "liquidated") {
          acc.collateral += loan.lendingValue;
          acc.debt += loan.debt;
          if (isOpenLoan(loan)) acc.open += 1;
        }
        if (loan.state === "liquidatable") acc.liquidatable += 1;
        return acc;
      },
      { collateral: 0, debt: 0, open: 0, liquidatable: 0 },
    );
  }, [loans]);
  async function sendAndConfirm(tx: VersionedTransaction, signers: Keypair[] = []) {
    if (signers.length > 0) tx.sign(signers);
    const simulation = await connection.simulateTransaction(tx, {
      sigVerify: false,
      replaceRecentBlockhash: true,
      commitment: "confirmed",
    });
    if (simulation.value.err) {
      throw new Error(`Simulation failed: ${JSON.stringify(simulation.value.err)}`);
    }
    const signature = await sendTransaction(tx, connection, { preflightCommitment: "confirmed" });
    await connection.confirmTransaction(signature, "confirmed");
    return signature;
  }

  function rememberLoanTransaction(loan: DemoLoan, signature: string, label: string) {
    setLastSignature(signature);
    setLoanTransactions((items) => ({
      ...items,
      [loan.id]: { signature, label },
    }));
  }

  async function submitLoanAction(
    loan: DemoLoan,
    action: "borrow" | "liquidate",
  ): Promise<LoanActionResult | null> {
    if (!publicKey) {
      setError("Connect a wallet to send the loan transaction.");
      return null;
    }
    if (!hasLivePolicyAccounts(loan)) {
      throw new Error("Live policy and terms accounts are required. No fallback collateral is available.");
    }

    const markerRecipient = new PublicKey(loan.borrower);
    const policyAddress = new PublicKey(loan.policyAddress);
    const productTermsAddress = new PublicKey(loan.productTermsAddress);
    const includeMemo = cluster !== "localnet";

    if (action === "borrow") {
      const estimatedDebt = borrowDebtForLoan(loan);
      const memo = `Halcyon lending borrow ${loan.id}; pricing checkpointed preview_lending_value; policy ${loan.policyAddress}; receipt ${loan.receiptMint}; estimated_debt ${estimatedDebt}`;
      const execution = await executeCheckpointedMockLendingBorrow({
        connection,
        config: current,
        payer: publicKey,
        markerRecipient,
        memo,
        includeMemo,
        policyAddress,
        productTermsAddress,
        sendTransaction: (transaction, signers) => sendAndConfirm(transaction, signers),
      });
      const pricedLendingValue =
        toNumber(field(execution.preview, "lendingValuePayoutUsdc")) || loan.lendingValue;
      return {
        signature: execution.signatures[execution.signatures.length - 1],
        pricedLendingValue,
        transactionCount: execution.signatures.length,
        maxUnitsConsumed: execution.maxUnitsConsumed,
      };
    }

    const execution = await executeCheckpointedWrappedFlagshipLiquidation({
      connection,
      config: current,
      holder: publicKey,
      policyAddress,
      productTermsAddress,
      sendTransaction: (transaction, signers) => sendAndConfirm(transaction, signers),
    });
    return {
      signature: execution.signatures[execution.signatures.length - 1],
      transactionCount: execution.signatures.length,
      maxUnitsConsumed: execution.maxUnitsConsumed,
    };
  }

  async function handleWrap(loan: DemoLoan) {
    if (!publicKey) {
      setError("Connect a wallet to tokenize collateral.");
      return;
    }

    setActionId(`wrap-${loan.id}`);
    setError(null);
    setLastSignature(null);
    try {
      const result = {
        signature: await sendAndConfirm(
          await buildWrapPolicyReceiptTransaction(connection, current, publicKey, new PublicKey(loan.policyAddress)),
        ),
      };
      setWrapped((values) => ({ ...values, [loan.id]: true }));
      rememberLoanTransaction(loan, result.signature, "Tokenization tx");
    } catch (cause) {
      const mapped = mapSolanaError(cause);
      setError(`${mapped.title} ${mapped.body}`);
    } finally {
      setActionId(null);
    }
  }

  async function handleBorrow(loan: DemoLoan) {
    if (!loan.wrapped) {
      setError("Tokenize the position receipt before opening a loan.");
      return;
    }
    if (!publicKey) {
      setError("Connect a wallet to open a loan against live collateral.");
      return;
    }

    setActionId(`borrow-${loan.id}`);
    setError(null);
    setLastSignature(null);
    try {
      const result = await submitLoanAction(loan, "borrow");
      if (!result) return;

      const lendingValue = result.pricedLendingValue ?? loan.lendingValue;
      const debt = borrowDebtForLoan({ ...loan, lendingValue });
      const health = lendingValue / Math.max(1, debt);
      rememberLoanTransaction(
        loan,
        result.signature,
        result.transactionCount
          ? `Checkpointed loan tx (${result.transactionCount} tx)`
          : "Live-priced loan tx",
      );
      setLiveLoans((items) =>
        items.map((item) =>
          item.id === loan.id
            ? {
                ...item,
                lendingValue,
                debt,
                health,
                state: healthState(health),
                wrapped: true,
              }
            : item,
        ),
      );
    } catch (cause) {
      const mapped = mapSolanaError(cause);
      setError(`${mapped.title} ${mapped.body}`);
    } finally {
      setActionId(null);
    }
  }

  async function handleLiquidate(loan: DemoLoan) {
    if (!isOpenLoan(loan)) {
      setError("Open a loan before sending liquidation.");
      return;
    }
    if (!publicKey) {
      setError("Connect a wallet to send the liquidation transaction.");
      return;
    }

    setActionId(`liquidate-${loan.id}`);
    setError(null);
    setLastSignature(null);
    try {
      const result = await submitLoanAction(loan, "liquidate");
      if (!result) return;
      rememberLoanTransaction(
        loan,
        result.signature,
        result.transactionCount
          ? `Checkpointed buyback (${result.transactionCount} tx)`
          : "Checkpointed buyback",
      );
      setLiveLoans((items) =>
        items.map((item) =>
          item.id === loan.id ? { ...item, state: "liquidated", debt: 0, health: Number.POSITIVE_INFINITY } : item,
        ),
      );
    } catch (cause) {
      const mapped = mapSolanaError(cause);
      setError(`${mapped.title} ${mapped.body}`);
    } finally {
      setActionId(null);
    }
  }

  const explorerUrl = lastSignature ? solscanTransactionUrl(cluster, lastSignature) : "";

  return (
    <div className="space-y-6">
      <section className="surface p-5 sm:p-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              Halcyon Credit Demo
            </div>
            <h1 className="mt-2 text-3xl font-semibold tracking-tight text-foreground sm:text-4xl">
              Receipt-token collateral desk
            </h1>
            <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
              Take a live position receipt SPL token, run checkpointed on-chain pricing, open a loan against it, then
              liquidate live collateral through the Flagship buyback path.
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={loadLiveLoans}
              disabled={!connected || missingConfig.length > 0 || loading}
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
              <ShieldCheck className="h-4 w-4" aria-hidden="true" />
              Runtime
            </button>
          </div>
        </div>

        <div className="mt-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <div className="rounded-md border border-border bg-background p-4">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Coins className="h-4 w-4" aria-hidden="true" />
              Collateral value
            </div>
            <div className="mt-3 text-2xl font-semibold tabular text-foreground">
              {formatUsdcBaseUnits(totals.collateral)}
            </div>
          </div>
          <div className="rounded-md border border-border bg-background p-4">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <BadgeDollarSign className="h-4 w-4" aria-hidden="true" />
              Debt
            </div>
            <div className="mt-3 text-2xl font-semibold tabular text-foreground">{formatUsdcBaseUnits(totals.debt)}</div>
          </div>
          <div className="rounded-md border border-border bg-background p-4">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <ShieldCheck className="h-4 w-4" aria-hidden="true" />
              Open loans
            </div>
            <div className="mt-3 text-2xl font-semibold tabular text-foreground">{totals.open}</div>
          </div>
          <div className="rounded-md border border-border bg-background p-4">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Siren className="h-4 w-4" aria-hidden="true" />
              Liquidatable
            </div>
            <div className="mt-3 text-2xl font-semibold tabular text-foreground">{totals.liquidatable}</div>
          </div>
        </div>

        <div className="mt-6 grid gap-3 md:grid-cols-3">
          {LENDING_FLOW_STEPS.map((item) => (
            <div key={item.step} className="rounded-md border border-border bg-background p-4">
              <div className="flex items-baseline justify-between gap-3">
                <div className="text-sm font-semibold text-foreground">{item.title}</div>
                <div className="font-mono text-xs tabular-nums text-muted-foreground">{item.step}</div>
              </div>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">{item.body}</p>
            </div>
          ))}
        </div>
      </section>

      {missingConfig.length > 0 && connected && (
        <section className="rounded-md border border-warning-500/40 bg-warning-50 p-4">
          <div className="flex items-start gap-3">
            <AlertCircle className="mt-0.5 h-5 w-5 text-warning-700" aria-hidden="true" />
            <div>
              <div className="text-sm font-medium text-foreground">Runtime config missing</div>
              <p className="mt-1 text-sm text-muted-foreground">{missingConfig.join(", ")}</p>
            </div>
          </div>
        </section>
      )}

      {!connected && (
        <section className="rounded-md border border-border bg-card p-4 text-sm leading-6 text-muted-foreground">
          Connect a wallet with an active flagship note. This page does not render fallback collateral accounts.
        </section>
      )}

      {connected && cluster === "localnet" && (
        <section className="rounded-md border border-border bg-card p-4 text-sm leading-6 text-muted-foreground">
          Localnet transactions confirm on your validator, but Solscan links are available only on devnet and mainnet.
        </section>
      )}

      {error && (
        <section className="rounded-md border border-destructive/30 bg-destructive/10 p-4">
          <div className="flex items-start gap-3">
            <AlertCircle className="mt-0.5 h-5 w-5 text-destructive" aria-hidden="true" />
            <div>
              <div className="text-sm font-medium text-foreground">Transaction failed</div>
              <p className="mt-1 text-sm text-muted-foreground">{error}</p>
            </div>
          </div>
        </section>
      )}

      {lastSignature && (
        <section className="rounded-md border border-success-700/30 bg-success-50 p-4 text-sm text-success-700">
          {explorerUrl ? (
            <SolscanTransactionLink signature={lastSignature} cluster={cluster}>
              View latest transaction on Solscan
            </SolscanTransactionLink>
          ) : (
            <span className="font-medium">{shortAddress(lastSignature, 8)} confirmed locally</span>
          )}
        </section>
      )}

      <section className="surface overflow-hidden">
        <div className="border-b border-border px-5 py-4 sm:px-6">
          <h2 className="text-xl font-semibold text-foreground">Collateral accounts</h2>
        </div>

        {loading && liveLoans.length === 0 ? (
          <div className="grid gap-3 p-5 sm:p-6">
            {Array.from({ length: 3 }).map((_, index) => (
              <div key={index} className="h-24 rounded-md border border-border bg-background motion-safe:animate-pulse" />
            ))}
          </div>
        ) : loans.length === 0 ? (
          <div className="p-5 sm:p-6">
            <div className="rounded-md border border-border bg-background p-5">
              <div className="text-lg font-semibold text-foreground">No collateral accounts</div>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">Connect a wallet with an active flagship note.</p>
            </div>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-border text-left text-sm">
              <thead className="bg-n-50 text-muted-foreground">
                <tr>
                  <th className="px-4 py-3 font-medium">Receipt SPL token</th>
                  <th className="px-4 py-3 font-medium">Borrower</th>
                  <th className="px-4 py-3 font-medium">Lending value</th>
                  <th className="px-4 py-3 font-medium">Debt</th>
                  <th className="px-4 py-3 font-medium">Loan health</th>
                  <th className="px-4 py-3 font-medium">Status</th>
                  <th className="px-4 py-3 font-medium">Action</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border bg-card">
                {loans.map((loan) => {
                  const wrapping = actionId === `wrap-${loan.id}`;
                  const borrowing = actionId === `borrow-${loan.id}`;
                  const liquidating = actionId === `liquidate-${loan.id}`;
                  const walletUnavailable = !connected;
                  const canBorrow = loan.wrapped && loan.state === "available";
                  const canLiquidate = loan.wrapped && isOpenLoan(loan);
                  const loanTx = loanTransactions[loan.id];
                  return (
                    <tr key={loan.id} className="align-top">
                      <td className="px-4 py-4">
                        <div className="font-mono text-[12px] text-foreground">
                          <SolscanAccountLink address={loan.receiptMint} cluster={cluster} />
                        </div>
                        <div className="mt-1 text-xs text-muted-foreground">Mint</div>
                        <div className="mt-2 text-xs text-muted-foreground">
                          Token acct{" "}
                          <span className="font-mono text-foreground">
                            <SolscanAccountLink address={loan.receiptTokenAccount} cluster={cluster} />
                          </span>
                        </div>
                        <div className="mt-2 text-xs text-muted-foreground">
                          Position notional{" "}
                          <span className="tabular text-foreground">{formatUsdcBaseUnits(loan.positionNotional)}</span>
                        </div>
                        {hasLivePolicyAccounts(loan) ? (
                          <>
                            <div className="mt-2 text-xs text-muted-foreground">
                              Borrow pricing{" "}
                              <span className="font-medium text-foreground">Checkpointed midlife NAV</span>
                            </div>
                            <div className="mt-2 text-xs text-muted-foreground">
                              Policy{" "}
                              <span className="font-mono text-foreground">
                                <SolscanAccountLink address={loan.policyAddress} cluster={cluster} />
                              </span>
                            </div>
                            <div className="mt-1 text-xs text-muted-foreground">
                              Terms{" "}
                              <span className="font-mono text-foreground">
                                <SolscanAccountLink address={loan.productTermsAddress} cluster={cluster} />
                              </span>
                            </div>
                          </>
                        ) : (
                          <div className="mt-2 text-xs text-muted-foreground">
                            Borrow pricing{" "}
                            <span className="font-medium text-foreground">Flagship preview_quote</span>
                          </div>
                        )}
                      </td>
                      <td className="px-4 py-4 font-mono text-[12px] text-foreground">
                        <SolscanAccountLink address={loan.borrower} cluster={cluster} />
                      </td>
                      <td className="px-4 py-4 font-medium tabular text-foreground">
                        {formatUsdcBaseUnits(loan.lendingValue)}
                      </td>
                      <td className="px-4 py-4 tabular text-foreground">{formatUsdcBaseUnits(loan.debt)}</td>
                      <td className="px-4 py-4 tabular text-foreground">
                        {Number.isFinite(loan.health) ? `${loan.health.toFixed(2)}x` : "-"}
                        {loan.state === "available" ? (
                          <div className="mt-1 text-xs text-muted-foreground">No loan open</div>
                        ) : null}
                      </td>
                      <td className="px-4 py-4">
                        <span className={cn("inline-flex min-h-10 items-center rounded-md border px-3 font-medium", stateTone(loan.state))}>
                          {stateLabel(loan.state)}
                        </span>
                      </td>
                      <td className="px-4 py-4">
                        <div className="flex flex-wrap gap-2">
                          {!loan.wrapped && (
                            <button
                              type="button"
                              onClick={() => handleWrap(loan)}
                              disabled={walletUnavailable || Boolean(actionId)}
                              aria-busy={wrapping}
                              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 font-medium text-foreground transition-colors hover:bg-secondary disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                            >
                              {wrapping ? <Loader2 className="h-4 w-4 motion-safe:animate-spin" aria-hidden="true" /> : <Coins className="h-4 w-4" aria-hidden="true" />}
                              Tokenize
                            </button>
                          )}
                          {loan.wrapped && loan.state === "available" ? (
                            <button
                              type="button"
                              onClick={() => handleBorrow(loan)}
                              disabled={walletUnavailable || !canBorrow || Boolean(actionId)}
                              aria-busy={borrowing}
                              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-foreground px-3 font-medium text-background transition-colors hover:bg-foreground/90 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                            >
                              {borrowing ? (
                                <Loader2 className="h-4 w-4 motion-safe:animate-spin" aria-hidden="true" />
                              ) : (
                                <BadgeDollarSign className="h-4 w-4" aria-hidden="true" />
                              )}
                              Take loan
                            </button>
                          ) : null}
                          {loan.state !== "available" ? (
                            <button
                              type="button"
                              onClick={() => handleLiquidate(loan)}
                              disabled={walletUnavailable || !canLiquidate || Boolean(actionId)}
                              aria-busy={liquidating}
                              className="inline-flex min-h-10 items-center gap-2 rounded-md border border-destructive/30 bg-destructive px-3 font-medium text-destructive-foreground transition-colors hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                            >
                              {liquidating ? (
                                <Loader2 className="h-4 w-4 motion-safe:animate-spin" aria-hidden="true" />
                              ) : (
                                <Siren className="h-4 w-4" aria-hidden="true" />
                              )}
                              {loan.state === "liquidated" ? "Liquidated" : "Buy back"}
                            </button>
                          ) : null}
                          {walletUnavailable ? (
                            <div className="basis-full text-xs text-muted-foreground">
                              Wallet required on this cluster.
                            </div>
                          ) : null}
                          {loanTx ? (
                            <div className="basis-full text-xs text-muted-foreground">
                              <SolscanTransactionLink signature={loanTx.signature} cluster={cluster}>
                                {loanTx.label}
                              </SolscanTransactionLink>
                            </div>
                          ) : null}
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}

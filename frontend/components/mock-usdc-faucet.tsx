"use client";

import { FormEvent, ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  ExternalLink,
  LoaderCircle,
  RefreshCw,
  Wallet,
} from "lucide-react";
import { PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";

import { WalletControl } from "@/components/wallet-control";
import { cn, shortAddress } from "@/lib/format";
import { useRuntimeConfig } from "@/lib/runtime-config";

const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const FAUCET_URL = (process.env.NEXT_PUBLIC_MOCK_USDC_FAUCET_URL ?? "").replace(/\/$/, "");

type FaucetHealth = {
  ok: boolean;
  mint: string;
  amount: string;
  maxAmount: string;
  cooldownMs: number;
};

type ClaimResult = {
  ok: boolean;
  signature: string;
  mint: string;
  wallet: string;
  tokenAccount: string;
  amount: string;
  explorerUrl: string;
};

function associatedTokenAddress(owner: PublicKey, mint: PublicKey) {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  )[0];
}

function explorerAddress(address: string) {
  return `https://explorer.solana.com/address/${address}?cluster=devnet`;
}

function formatCooldown(ms: number) {
  const minutes = Math.max(1, Math.ceil(ms / 60_000));
  return `${minutes} min`;
}

function displayBalance(value: string | null) {
  if (!value) return "0";
  const [whole, fraction = ""] = value.split(".");
  const trimmed = fraction.replace(/0+$/, "").slice(0, 4);
  return trimmed ? `${whole}.${trimmed}` : whole;
}

export function MockUsdcFaucet() {
  const { cluster, current } = useRuntimeConfig();
  const { connection } = useConnection();
  const { connected, publicKey } = useWallet();
  const [health, setHealth] = useState<FaucetHealth | null>(null);
  const [healthError, setHealthError] = useState<string | null>(null);
  const [balance, setBalance] = useState<string | null>(null);
  const [balanceBusy, setBalanceBusy] = useState(false);
  const [claimBusy, setClaimBusy] = useState(false);
  const [claimError, setClaimError] = useState<string | null>(null);
  const [claimResult, setClaimResult] = useState<ClaimResult | null>(null);

  const configuredMint = useMemo(() => {
    const value = current.usdcMint || health?.mint || "";
    try {
      return value ? new PublicKey(value) : null;
    } catch {
      return null;
    }
  }, [current.usdcMint, health?.mint]);

  const tokenAccount = useMemo(() => {
    if (!publicKey || !configuredMint) return null;
    return associatedTokenAddress(publicKey, configuredMint);
  }, [publicKey, configuredMint]);

  const loadHealth = useCallback(async () => {
    if (!FAUCET_URL) return;
    setHealthError(null);
    try {
      const response = await fetch(`${FAUCET_URL}/health`, { cache: "no-store" });
      const body = await response.json();
      if (!response.ok || !body?.ok) {
        throw new Error(body?.error ?? `Faucet returned ${response.status}`);
      }
      setHealth(body as FaucetHealth);
    } catch (cause) {
      setHealth(null);
      setHealthError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  const loadBalance = useCallback(async () => {
    if (!tokenAccount) {
      setBalance(null);
      return;
    }
    setBalanceBusy(true);
    try {
      const response = await connection.getTokenAccountBalance(tokenAccount, "confirmed");
      setBalance(response.value.uiAmountString ?? "0");
    } catch {
      setBalance("0");
    } finally {
      setBalanceBusy(false);
    }
  }, [connection, tokenAccount]);

  useEffect(() => {
    void loadHealth();
  }, [loadHealth]);

  useEffect(() => {
    void loadBalance();
  }, [loadBalance]);

  const disabledReason = (() => {
    if (cluster !== "devnet") return "Switch to devnet to request mockUSDC.";
    if (!configuredMint) return "Devnet mockUSDC mint is not configured.";
    if (!FAUCET_URL) return "Faucet URL is not configured.";
    if (!connected || !publicKey) return "Connect a devnet wallet.";
    if (health && health.mint !== configuredMint.toBase58()) {
      return "The faucet mint does not match the app's devnet USDC mint.";
    }
    return null;
  })();

  const handleClaim = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (disabledReason || !publicKey) return;
    setClaimBusy(true);
    setClaimError(null);
    setClaimResult(null);
    try {
      const response = await fetch(`${FAUCET_URL}/airdrop`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ wallet: publicKey.toBase58() }),
      });
      const body = await response.json();
      if (!response.ok || !body?.ok) {
        if (body?.error === "cooldown" && typeof body.retryAfterMs === "number") {
          throw new Error(`This wallet can claim again in ${formatCooldown(body.retryAfterMs)}.`);
        }
        throw new Error(body?.error ?? `Faucet returned ${response.status}`);
      }
      setClaimResult(body as ClaimResult);
      await loadBalance();
    } catch (cause) {
      setClaimError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setClaimBusy(false);
    }
  };

  return (
    <div className="mx-auto max-w-[1120px] space-y-5">
      <section className="surface overflow-hidden">
        <div className="grid gap-0 lg:grid-cols-[1.1fr_0.9fr]">
          <div className="p-5 sm:p-6">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div className="min-w-0">
                <p className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
                  Devnet Faucet
                </p>
                <h1 className="mt-2 text-3xl font-semibold tracking-normal text-foreground">
                  mockUSDC for judge wallets
                </h1>
                <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
                  Claim devnet payment tokens for issuing notes, seeding the vault, and testing
                  the collateral demo. The faucet mints to your associated token account.
                </p>
              </div>
              <div className="flex min-h-10 items-center rounded-md border border-border bg-secondary px-3 text-sm font-medium capitalize text-foreground">
                {cluster}
              </div>
            </div>

            <div className="mt-6 grid gap-3 sm:grid-cols-3">
              <Stat
                label="Mint"
                value={configuredMint ? shortAddress(configuredMint, 5) : "Not set"}
                href={configuredMint ? explorerAddress(configuredMint.toBase58()) : undefined}
              />
              <Stat
                label="Claim size"
                value={`${health?.amount ?? "25000"} mockUSDC`}
              />
              <Stat
                label="Wallet balance"
                value={balanceBusy ? "Loading..." : `${displayBalance(balance)} mockUSDC`}
              />
            </div>
          </div>

          <div className="border-t border-border bg-background p-5 sm:p-6 lg:border-l lg:border-t-0">
            <form onSubmit={handleClaim} className="space-y-4" aria-busy={claimBusy}>
              <fieldset className="space-y-4">
                <legend className="text-sm font-semibold text-foreground">Request tokens</legend>
                <div className="rounded-md border border-border bg-card p-4">
                  <div className="flex items-start gap-3">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md border border-border bg-background">
                      <Wallet className="h-4 w-4" aria-hidden="true" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="text-sm font-medium text-foreground">
                        {publicKey ? shortAddress(publicKey, 5) : "No wallet connected"}
                      </p>
                      <p className="mt-1 break-all text-xs leading-5 text-muted-foreground">
                        {tokenAccount
                          ? `Token account ${shortAddress(tokenAccount, 5)}`
                          : "Connect a wallet to derive the mockUSDC token account."}
                      </p>
                    </div>
                  </div>
                </div>

                {disabledReason ? (
                  <StatusBanner tone="warning" title="Faucet unavailable" message={disabledReason} />
                ) : null}

                {healthError ? (
                  <StatusBanner
                    tone="error"
                    title="Faucet health check failed"
                    message={healthError}
                    action={<RetryButton onClick={loadHealth} />}
                  />
                ) : null}

                {claimError ? (
                  <StatusBanner tone="error" title="Claim failed" message={claimError} />
                ) : null}

                {claimResult ? (
                  <StatusBanner
                    tone="success"
                    title={`${claimResult.amount} mockUSDC minted`}
                    message={`Sent to ${shortAddress(claimResult.tokenAccount, 5)}.`}
                    action={
                      <a
                        href={claimResult.explorerUrl}
                        target="_blank"
                        rel="noreferrer"
                        className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                      >
                        View Tx
                        <ExternalLink className="h-4 w-4" aria-hidden="true" />
                      </a>
                    }
                  />
                ) : null}

                <button
                  type="submit"
                  disabled={Boolean(disabledReason) || claimBusy}
                  className="inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {claimBusy ? (
                    <LoaderCircle className="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
                  ) : null}
                  {claimBusy ? "Minting mockUSDC..." : `Claim ${health?.amount ?? "25000"} mockUSDC`}
                </button>
              </fieldset>
            </form>

            {!connected ? (
              <div className="mt-4">
                <WalletControl />
              </div>
            ) : null}

            <div className="mt-5 border-t border-border pt-4">
              <a
                href="https://faucet.solana.com/"
                target="_blank"
                rel="noreferrer"
                className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
              >
                Need devnet SOL?
                <ExternalLink className="h-4 w-4" aria-hidden="true" />
              </a>
              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                SOL is still needed for rent and transaction fees. mockUSDC is only the demo
                payment token.
              </p>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

function Stat({
  label,
  value,
  href,
}: {
  label: string;
  value: string;
  href?: string;
}) {
  const content = (
    <>
      <dt className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
        {label}
      </dt>
      <dd className="mt-2 break-words font-mono text-sm font-semibold tabular-nums text-foreground">
        {value}
      </dd>
    </>
  );

  if (!href) {
    return <dl className="rounded-md border border-border bg-card p-4">{content}</dl>;
  }

  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="block rounded-md border border-border bg-card p-4 transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    >
      <dl>{content}</dl>
    </a>
  );
}

function RetryButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    >
      <RefreshCw className="h-4 w-4" aria-hidden="true" />
      Retry
    </button>
  );
}

function StatusBanner({
  tone,
  title,
  message,
  action,
}: {
  tone: "success" | "warning" | "error";
  title: string;
  message: string;
  action?: ReactNode;
}) {
  const Icon = tone === "success" ? CheckCircle2 : AlertTriangle;
  return (
    <div
      role={tone === "success" ? "status" : "alert"}
      className={cn(
        "rounded-md border p-3",
        tone === "success" && "border-success-700/20 bg-success-50",
        tone === "warning" && "border-warning-500/30 bg-warning-50",
        tone === "error" && "border-destructive/30 bg-error-50",
      )}
    >
      <div className="flex items-start gap-3">
        <Icon
          className={cn(
            "mt-0.5 h-4 w-4 shrink-0",
            tone === "success" && "text-success-700",
            tone === "warning" && "text-warning-700",
            tone === "error" && "text-error-700",
          )}
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-foreground">{title}</p>
          <p className="mt-1 text-sm leading-5 text-muted-foreground">{message}</p>
          {action ? <div className="mt-3">{action}</div> : null}
        </div>
      </div>
    </div>
  );
}

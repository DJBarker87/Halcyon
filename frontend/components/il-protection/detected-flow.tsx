"use client";

import { useMemo } from "react";
import { ArrowRight, Droplet, RefreshCcw } from "lucide-react";

import type { LpDetectionSuccess } from "@/lib/lp-detection";
import { IssuancePage } from "@/components/issuance-page";

interface DetectedFlowProps {
  data: LpDetectionSuccess;
  onRefresh: () => void;
  onPickSynthetic: () => void;
}

function formatUsd(value: number) {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  }).format(value);
}

function formatToken(value: number, digits = 2) {
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

function formatAge(fetchedAt: number) {
  const ageSec = Math.max(0, Math.round((Date.now() - fetchedAt) / 1000));
  if (ageSec < 60) return `${ageSec}s ago`;
  const ageMin = Math.round(ageSec / 60);
  return `${ageMin}m ago`;
}

/**
 * State A — wallet connected and Raydium SOL/USDC LP detected. Shows the
 * buyer their real position + reserve breakdown, pre-fills the cover
 * notional to match, and renders the standard IssuancePage beneath for the
 * live quote + sign flow.
 */
export function DetectedFlow({ data, onRefresh, onPickSynthetic }: DetectedFlowProps) {
  const notionalDefault = useMemo(
    () => Math.round(data.valueUsdc).toString(),
    [data.valueUsdc],
  );

  return (
    <div className="space-y-8">
      <section className="rounded-md border border-halcyonBlue-300 bg-halcyonBlue-50 p-6 sm:p-8">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="flex items-start gap-4">
            <div className="flex h-11 w-11 items-center justify-center rounded-md bg-halcyonBlue-600 text-paper">
              <Droplet className="h-5 w-5" aria-hidden="true" />
            </div>
            <div>
              <div className="text-xs font-medium uppercase tracking-[0.14em] text-halcyonBlue-700">
                LP position detected
              </div>
              <h1 className="mt-1 font-serif text-3xl leading-tight text-ink sm:text-4xl">
                Your Raydium SOL/USDC LP is worth{" "}
                <span className="tabular">{formatUsd(data.valueUsdc)}</span>.
              </h1>
              <p className="mt-2 max-w-2xl text-sm leading-6 text-foreground/85">
                We'll price cover for your exact exposure. Review the quote
                below, then sign once to buy the 30-day cover in USDC. No LP
                token custody — your position stays where it is.
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onRefresh}
            className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-paper px-3 text-sm font-medium text-foreground transition-colors hover:bg-n-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            <RefreshCcw className="h-4 w-4" aria-hidden="true" />
            Refresh
          </button>
        </div>

        <div className="mt-6 grid gap-3 sm:grid-cols-3">
          <Stat label="SOL" value={`${formatToken(data.underlyingSol, 3)} SOL`} sub={formatUsd(data.underlyingSol * data.solPrice)} />
          <Stat label="USDC" value={`${formatToken(data.underlyingUsdc, 2)} USDC`} sub={formatUsd(data.underlyingUsdc)} />
          <Stat label="LP tokens" value={`${formatToken(data.lpAmount, 4)} LP`} sub={`${formatAge(data.fetchedAt)} · Raydium v4`} />
        </div>
      </section>

      <IssuancePage kind="ilProtection" defaultNotional={notionalDefault} />

      <section className="rounded-md border border-border bg-card p-6 sm:p-8">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <h2 className="font-serif text-xl text-ink">Want to size it yourself instead?</h2>
            <p className="mt-1 text-sm leading-6 text-muted-foreground">
              Detected position not what you want to cover? Switch to a
              synthetic cover and pick any notional in USDC.
            </p>
          </div>
          <button
            type="button"
            onClick={onPickSynthetic}
            className="inline-flex min-h-10 items-center gap-2 rounded-md border border-border bg-paper px-3 text-sm font-medium text-foreground transition-colors hover:bg-n-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          >
            Use synthetic flow
            <ArrowRight className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      </section>
    </div>
  );
}

function Stat({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="rounded-md border border-halcyonBlue-200 bg-paper p-4">
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-halcyonBlue-700">
        {label}
      </div>
      <div className="mt-2 font-serif text-2xl leading-tight text-ink tabular">{value}</div>
      <div className="mt-1 text-xs text-muted-foreground tabular">{sub}</div>
    </div>
  );
}

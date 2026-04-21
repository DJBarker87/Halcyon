"use client";

import { ArrowRight, Droplet, Waves } from "lucide-react";

interface NoLpChoiceProps {
  /** Called when the user picks the synthetic (no-LP-required) path. */
  onPickSynthetic: () => void;
  /** True if the wallet is connected but holds no LP; false if disconnected. */
  walletConnected: boolean;
}

/**
 * State B — "wallet is connected but no Raydium SOL/USDC LP found" (or
 * wallet not connected at all). Offers two paths: the post-hackathon
 * "LP + Insure" bundle and the currently-live synthetic cover.
 */
export function NoLpChoice({ onPickSynthetic, walletConnected }: NoLpChoiceProps) {
  return (
    <div className="mx-auto max-w-4xl space-y-8">
      <div>
        <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
          IL Protection
        </div>
        <h1 className="mt-2 font-serif text-4xl leading-tight text-ink sm:text-5xl">
          Two ways to cover impermanent loss on SOL/USDC.
        </h1>
        <p className="mt-4 max-w-2xl text-base leading-7 text-foreground/85">
          {walletConnected
            ? "We didn't find a Raydium SOL/USDC LP position in your wallet. Pick how you want to buy cover — as a synthetic position you size yourself, or (coming soon) bundled with an LP deposit."
            : "Connect your wallet to automatically detect a Raydium SOL/USDC LP position, or buy a synthetic cover you size yourself — no LP token needed."}
        </p>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <div className="flex flex-col rounded-md border border-border bg-card p-6">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-md bg-halcyonBlue-50 text-halcyonBlue-700">
              <Droplet className="h-5 w-5" aria-hidden="true" />
            </div>
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
              Coming soon
            </div>
          </div>
          <h2 className="mt-4 font-serif text-2xl text-ink">LP + Insure</h2>
          <p className="mt-3 text-sm leading-6 text-muted-foreground">
            Deposit USDC, we route into the Raydium SOL/USDC pool and insure
            the resulting LP position in one transaction.
          </p>
          <p className="mt-2 text-xs text-muted-foreground">
            For: LPs who want AMM fee exposure with IL protection built in.
          </p>
          <div className="mt-6 inline-flex min-h-10 w-fit items-center rounded-md border border-border bg-n-50 px-3 text-sm font-medium text-muted-foreground">
            Post-hackathon
          </div>
        </div>

        <button
          type="button"
          onClick={onPickSynthetic}
          className="group flex flex-col rounded-md border border-halcyonBlue-300 bg-halcyonBlue-50 p-6 text-left transition-colors hover:bg-halcyonBlue-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
        >
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-md bg-halcyonBlue-600 text-paper">
              <Waves className="h-5 w-5" aria-hidden="true" />
            </div>
            <div className="text-xs font-medium uppercase tracking-[0.14em] text-halcyonBlue-700">
              Available now
            </div>
          </div>
          <h2 className="mt-4 font-serif text-2xl text-ink">Synthetic cover</h2>
          <p className="mt-3 text-sm leading-6 text-foreground/85">
            Pick the notional you want to cover and pay a premium in USDC. No
            LP token custody required — the payout at expiry is derived from
            the SOL/USDC price path alone.
          </p>
          <p className="mt-2 text-xs text-muted-foreground">
            For: anyone who wants IL-shaped payoff exposure without running an LP.
          </p>
          <div className="mt-6 inline-flex items-center gap-1.5 text-sm font-semibold text-halcyonBlue-700">
            Pick a notional
            <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-0.5" aria-hidden="true" />
          </div>
        </button>
      </div>
    </div>
  );
}

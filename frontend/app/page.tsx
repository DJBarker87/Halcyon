import Link from "next/link";
import { ArrowRight, BadgeDollarSign, ShieldCheck, Siren, WalletCards } from "lucide-react";

import { KingfisherEditorial } from "@/components/kingfisher";

const BUYER_STEPS = [
  {
    title: "Buy",
    body: "Choose USDC notional and preview the live coupon before signing.",
    href: "/flagship",
    action: "Get quote",
    icon: BadgeDollarSign,
  },
  {
    title: "Monitor",
    body: "See your active notes, current lending value, maturity, and status.",
    href: "/portfolio",
    action: "My notes",
    icon: WalletCards,
  },
  {
    title: "Exit",
    body: "Calculate the on-chain emergency exit price for an active note.",
    href: "/portfolio",
    action: "Emergency exit",
    icon: Siren,
  },
] as const;

export default function LandingPage() {
  return (
    <div className="mx-auto max-w-6xl space-y-8 pb-16">
      <section className="relative overflow-hidden rounded-lg border border-border bg-card p-6 shadow-sm sm:p-8 lg:p-10">
        <KingfisherEditorial
          size={300}
          color="var(--blue-600)"
          className="pointer-events-none absolute -right-6 top-2 hidden opacity-[0.06] lg:block"
        />
        <div className="relative max-w-3xl">
          <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
            Buyer dashboard
          </p>
          <h1 className="mt-3 text-4xl font-semibold leading-tight text-foreground sm:text-5xl">
            Buy the note. Track the value. Exit if you need to.
          </h1>
          <p className="mt-4 max-w-2xl text-base leading-7 text-muted-foreground">
            Halcyon shows only the buyer-critical numbers: your USDC notional, coupon, live note value,
            downside condition, and emergency exit price.
          </p>
          <div className="mt-7 flex flex-wrap gap-3">
            <Link
              href="/flagship"
              className="inline-flex min-h-12 items-center gap-2 rounded-md bg-primary px-5 text-sm font-semibold text-primary-foreground transition-opacity hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              Get live quote
              <ArrowRight className="h-4 w-4" aria-hidden="true" />
            </Link>
            <Link
              href="/portfolio"
              className="inline-flex min-h-12 items-center gap-2 rounded-md border border-border bg-background px-5 text-sm font-semibold text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              View my notes
              <WalletCards className="h-4 w-4" aria-hidden="true" />
            </Link>
          </div>
        </div>
      </section>

      <section className="grid gap-4 md:grid-cols-3">
        {BUYER_STEPS.map((step) => {
          const Icon = step.icon;
          return (
            <Link
              key={step.title}
              href={step.href}
              className="rounded-lg border border-border bg-card p-5 shadow-sm transition-colors hover:bg-secondary/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            >
              <div className="flex h-10 w-10 items-center justify-center rounded-md border border-border bg-background">
                <Icon className="h-5 w-5 text-primary" aria-hidden="true" />
              </div>
              <h2 className="mt-4 text-lg font-semibold text-foreground">{step.title}</h2>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">{step.body}</p>
              <div className="mt-4 inline-flex items-center gap-2 text-sm font-medium text-foreground">
                {step.action}
                <ArrowRight className="h-4 w-4" aria-hidden="true" />
              </div>
            </Link>
          );
        })}
      </section>

      <section className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_360px]">
        <div className="rounded-lg border border-border bg-card p-5 shadow-sm sm:p-6">
          <h2 className="text-xl font-semibold text-foreground">Current buyer terms</h2>
          <dl className="mt-5 grid gap-3 sm:grid-cols-2">
            <Term label="Underlying" value="Worst of SPY · QQQ · IWM" />
            <Term label="Coupon" value="Priced live before signing" />
            <Term label="Downside" value="Principal at risk only after knock-in" />
            <Term label="Emergency exit" value="Calculated from on-chain midlife NAV" />
          </dl>
        </div>
        <div className="rounded-lg border border-border bg-card p-5 shadow-sm sm:p-6">
          <div className="flex items-start gap-3">
            <ShieldCheck className="mt-0.5 h-5 w-5 text-success-700" aria-hidden="true" />
            <div>
              <h2 className="text-lg font-semibold text-foreground">What matters</h2>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">
                The quote and emergency exit price are program-calculated. The UI does not invent a price
                or hide the liquidation value behind operator-only screens.
              </p>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

function Term({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-background p-4">
      <dt className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">
        {label}
      </dt>
      <dd className="mt-2 text-sm font-semibold leading-6 text-foreground">{value}</dd>
    </div>
  );
}

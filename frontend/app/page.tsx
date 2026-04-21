import Link from "next/link";
import { ArrowRight, CheckCircle2, ExternalLink, Terminal } from "lucide-react";

import { Kingfisher, KingfisherEditorial } from "@/components/kingfisher";

export default function LandingPage() {
  return (
    <div className="mx-auto max-w-5xl space-y-16 pb-20">
      {/* Hero */}
      <section className="relative pt-8 sm:pt-12">
        <KingfisherEditorial
          size={280}
          color="var(--halcyonBlue-600, #0A66A0)"
          className="pointer-events-none absolute right-0 top-4 hidden opacity-[0.06] lg:block"
        />
        <div className="relative flex items-center gap-2 text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
          <Kingfisher size={14} color="var(--blue-600)" />
          Halcyon
        </div>
        <h1 className="mt-3 font-serif text-[2.75rem] leading-[1.08] tracking-tight text-foreground sm:text-[3.75rem]">
          Structured products on Solana,<br />
          priced on Solana.
        </h1>
        <p className="mt-6 max-w-2xl text-lg leading-8 text-foreground/90 sm:text-xl">
          Earn a monthly coupon on a basket of <strong>the S&amp;P 500, the Nasdaq-100, and the Russell 2000</strong>{" "}
          (SPY · QQQ · IWM). Every coupon is computed by a Rust program running inside a Solana
          validator — not by an off-chain pricing service. The quote you see is the quote you get,
          and anyone can reproduce it.
        </p>

        <div className="mt-8 flex flex-wrap gap-3">
          <Link
            href="/flagship"
            className="inline-flex min-h-12 items-center gap-2 rounded-md bg-primary px-5 text-sm font-semibold text-primary-foreground transition-opacity hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background sm:text-base"
          >
            See a live quote
            <ArrowRight className="h-4 w-4" aria-hidden="true" />
          </Link>
          <a
            href="https://github.com/DJB8787/colosseumfinal/blob/main/halcyon_whitepaper_v9.md"
            target="_blank"
            rel="noreferrer"
            className="inline-flex min-h-12 items-center gap-2 rounded-md border border-border bg-background px-5 text-sm font-semibold text-foreground transition-colors hover:bg-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background sm:text-base"
          >
            Read the whitepaper
            <ExternalLink className="h-4 w-4" aria-hidden="true" />
          </a>
        </div>
      </section>

      {/* Gap pitch */}
      <section className="grid gap-10 rounded-md border border-border bg-card p-8 sm:p-10 lg:grid-cols-[1.2fr_1fr]">
        <div>
          <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
            Why now
          </div>
          <h2 className="mt-3 font-serif text-3xl leading-tight text-foreground sm:text-4xl">
            A $100B+ TradFi category<br />
            that has never worked on-chain.
          </h2>
          <p className="mt-5 text-base leading-7 text-foreground/85 sm:text-lg">
            Autocallable notes are one of the largest products in traditional structured-finance —
            over $100 billion issued a year, mostly to retail. On-chain, every serious attempt has
            died the same way.
          </p>
          <p className="mt-4 text-base leading-7 text-foreground/85 sm:text-lg">
            Ribbon, Cega, and Friktion all proved the demand (nine figures of TVL at peak). They
            all died from the same bug: their pricing lived off-chain, so buyers had to trust a
            quote oracle or a market-maker intent. The trust evaporated. The protocols shut down.
          </p>
          <p className="mt-4 text-base leading-7 text-foreground/85 sm:text-lg">
            Halcyon is the mechanism they were missing.
          </p>
        </div>
        <div className="space-y-3">
          <GraveyardCard name="Ribbon Finance" peak="$300M TVL" year="Wound down 2024" cause="Off-chain pricing oracle" />
          <GraveyardCard name="Friktion" peak="$150M TVL" year="Wound down 2023" cause="Off-chain quote intent" />
          <GraveyardCard name="Cega" peak="$16.6M TVL" year="Retired 2024" cause="Off-chain vol / barrier pricing" />
        </div>
      </section>

      {/* Problem → forced choice → resolution */}
      <section className="space-y-6">
        <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
          The mechanism
        </div>
        <h2 className="font-serif text-3xl leading-tight text-foreground sm:text-4xl">
          Black-Scholes doesn't fit crypto.<br />
          Everything else didn't fit a program.
        </h2>
        <p className="max-w-3xl text-base leading-7 text-foreground/85 sm:text-lg">
          Traditional structured-product pricing assumes log-normal returns, smooth vol, and
          tractable correlation. Crypto has fat tails, vol clusters, and correlation breaks.
          Previous on-chain teams faced a forced choice: ship off-chain pricing and hope nobody
          exploits the oracle, or don't ship. They shipped off-chain, and they died.
        </p>
        <p className="max-w-3xl text-base leading-7 text-foreground/85 sm:text-lg">
          Halcyon prices the whole family — Normal Inverse Gaussian distributions, Bessel-K₁
          densities, bivariate-normal correlation CDFs, worst-of-three barrier corrections — in
          fixed-point integer math, inside the Solana program itself. Deterministic on every
          validator. Reproducible by anyone.
        </p>

        <div className="mt-2 grid gap-4 sm:grid-cols-3">
          <MechanismCard
            title="On-chain pricer"
            body="A Rust program computes your coupon every block, with no trust assumption beyond Solana itself."
          />
          <MechanismCard
            title="One-sim verification"
            body="Anyone can replay a quote by calling simulateTransaction against the program. The pricer is open source."
          />
          <MechanismCard
            title="Hedged on-chain"
            body="Delta, volatility, and correlation surfaces all live in kernel state. The hedge trades through Jupiter against tokenized equities."
          />
        </div>
      </section>

      {/* Outcomes */}
      <section className="space-y-6">
        <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
          What you can do
        </div>
        <h2 className="font-serif text-3xl leading-tight text-foreground sm:text-4xl">
          Earn equity-like yield without the custody.
        </h2>
        <ul className="grid max-w-3xl gap-3">
          <OutcomeRow>Earn a monthly coupon on SPY / QQQ / IWM without holding the ETFs or routing through a broker.</OutcomeRow>
          <OutcomeRow>Verify every quote in one <code className="rounded bg-secondary px-1.5 py-0.5 font-mono text-[13px]">simulateTransaction</code> before your wallet signs.</OutcomeRow>
          <OutcomeRow>Keep your principal safe unless the worst performer falls past a 20% knock-in at expiry.</OutcomeRow>
          <OutcomeRow>Call out early every quarter when the basket is healthy, and redeploy your capital.</OutcomeRow>
        </ul>
      </section>

      {/* Products */}
      <section className="space-y-6">
        <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
          Three products, one vault
        </div>
        <h2 className="font-serif text-3xl leading-tight text-foreground sm:text-4xl">
          Pick a note.
        </h2>

        <div className="grid gap-4 lg:grid-cols-3">
          <ProductCard
            title="Equity Autocall"
            underlying="SPY · QQQ · IWM"
            blurb="18-month worst-of-3 autocallable. Monthly coupons, quarterly autocall checks, 20% downside cushion."
            href="/flagship"
            featured
          />
          <ProductCard
            title="IL Protection"
            underlying="SOL/USDC on Raydium"
            blurb="30-day cover that pays if your LP position's impermanent loss exceeds a threshold. Pay a premium, keep your pool position."
            href="/il-protection"
          />
          <ProductCard
            title="SOL Autocall"
            underlying="SOL-only · principal-backed"
            blurb="16-day principal-protected note on SOL with 8 observations. Short tenor, full capital protection."
            href="/sol-autocall"
          />
        </div>
      </section>

      {/* Credibility */}
      <section className="rounded-md border border-border bg-card p-8 sm:p-10">
        <div className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
          The math
        </div>
        <h2 className="mt-3 font-serif text-3xl leading-tight text-foreground sm:text-4xl">
          Built on <span className="font-mono text-[0.8em]">solmath-core</span>.
        </h2>
        <p className="mt-5 max-w-3xl text-base leading-7 text-foreground/85">
          The pricing primitives — fixed-point transcendentals, Bessel-K₁, bivariate-normal CDF,
          NIG density, Black-Scholes with all five Greeks — are published to crates.io as a
          standalone library. Validated across 2.5 million test vectors against QuantLib
          (14-decimal agreement), mpmath (50-digit precision), and scipy. The pricing function
          inside the Solana program is the same code, compiled for BPF.
        </p>
        <div className="mt-6 inline-flex items-center gap-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-[13px] text-foreground">
          <Terminal className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
          cargo add solmath
        </div>
      </section>

      {/* CTA */}
      <section className="flex flex-col items-start gap-6 rounded-md border border-primary/20 bg-primary/5 p-8 sm:flex-row sm:items-center sm:justify-between sm:p-10">
        <div>
          <h2 className="font-serif text-2xl leading-tight text-foreground sm:text-3xl">
            See what it quotes right now.
          </h2>
          <p className="mt-2 text-sm leading-6 text-muted-foreground sm:text-base">
            Pick a notional, see the live coupon, and verify it against the open-source pricer.
          </p>
        </div>
        <Link
          href="/flagship"
          className="inline-flex min-h-12 items-center gap-2 rounded-md bg-primary px-5 text-sm font-semibold text-primary-foreground transition-opacity hover:opacity-90 sm:text-base"
        >
          Get a quote
          <ArrowRight className="h-4 w-4" aria-hidden="true" />
        </Link>
      </section>

      {/* Footer */}
      <section className="border-t border-border pt-8 text-sm text-muted-foreground">
        <div className="flex flex-wrap gap-6">
          <a href="https://github.com/DJB8787/colosseumfinal" target="_blank" rel="noreferrer" className="hover:text-foreground">GitHub</a>
          <a href="https://github.com/DJB8787/colosseumfinal/blob/main/halcyon_whitepaper_v9.md" target="_blank" rel="noreferrer" className="hover:text-foreground">Whitepaper</a>
          <a href="https://github.com/DJB8787/colosseumfinal/blob/main/ARCHITECTURE.md" target="_blank" rel="noreferrer" className="hover:text-foreground">Architecture</a>
          <a href="https://github.com/DJB8787/colosseumfinal/blob/main/docs/audit" target="_blank" rel="noreferrer" className="hover:text-foreground">Audit</a>
          <a href="https://crates.io/crates/solmath" target="_blank" rel="noreferrer" className="hover:text-foreground">solmath on crates.io</a>
        </div>
      </section>
    </div>
  );
}

function GraveyardCard({ name, peak, year, cause }: { name: string; peak: string; year: string; cause: string }) {
  return (
    <div className="rounded-md border border-border bg-card p-4">
      <div className="flex items-baseline justify-between gap-3">
        <div className="font-semibold text-foreground">{name}</div>
        <div className="font-mono text-xs text-muted-foreground">{peak}</div>
      </div>
      <div className="mt-1 text-xs text-muted-foreground">{year}</div>
      <div className="mt-3 text-sm text-foreground/85">Cause of death: {cause}</div>
    </div>
  );
}

function MechanismCard({ title, body }: { title: string; body: string }) {
  return (
    <div className="rounded-md border border-border bg-card p-5">
      <div className="text-sm font-semibold text-foreground">{title}</div>
      <p className="mt-2 text-sm leading-6 text-muted-foreground">{body}</p>
    </div>
  );
}

function OutcomeRow({ children }: { children: React.ReactNode }) {
  return (
    <li className="flex items-start gap-3 rounded-md border border-border bg-card px-4 py-3">
      <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-success-700" aria-hidden="true" />
      <span className="text-base leading-7 text-foreground/90">{children}</span>
    </li>
  );
}

function ProductCard({
  title,
  underlying,
  blurb,
  href,
  featured,
}: {
  title: string;
  underlying: string;
  blurb: string;
  href: string;
  featured?: boolean;
}) {
  return (
    <Link
      href={href}
      className={`group flex flex-col rounded-md border p-5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background ${
        featured
          ? "border-primary/30 bg-primary/10 hover:bg-primary/15"
          : "border-border bg-card hover:bg-secondary/70"
      }`}
    >
      <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{underlying}</div>
      <div className="mt-2 font-serif text-2xl text-foreground">{title}</div>
      <p className="mt-3 flex-1 text-sm leading-6 text-foreground/85">{blurb}</p>
      <div className="mt-5 inline-flex items-center gap-1.5 text-sm font-semibold text-foreground">
        Quote this
        <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-0.5" aria-hidden="true" />
      </div>
    </Link>
  );
}

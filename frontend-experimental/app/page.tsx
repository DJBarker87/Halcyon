import Link from "next/link";

export default function Landing() {
  return (
    <main className="relative min-h-screen w-full">
      {/* Ambient swirly backdrop */}
      <div className="pointer-events-none absolute inset-0 -z-10 backdrop-swirl opacity-70" />

      {/* THE ONE FLOATING WRAPPER */}
      <div className="mx-auto max-w-[1240px] px-4 py-6 sm:px-8 sm:py-10">
        <div className="overflow-hidden rounded-[28px] bg-white shadow-[0_40px_120px_-30px_rgba(5,41,64,0.45)] ring-1 ring-black/[0.04] sm:rounded-[36px]">
          {/* Persistent blue header */}
          <header className="flex items-center justify-between bg-halcyon-500 px-6 py-5 text-white sm:px-10">
            <Link href="/" className="font-display text-sm font-extrabold tracking-[0.22em]">
              HALCYON
            </Link>
            <nav className="hidden items-center gap-6 text-[13px] font-medium text-white/85 md:flex">
              <button className="flex items-center gap-1 hover:text-white">Products <span className="text-white/60">▾</span></button>
              <Link href="#thesis"    className="hover:text-white">Thesis</Link>
              <Link href="#mechanism" className="hover:text-white">Mechanism</Link>
              <Link href="#shelf"     className="hover:text-white">Shelf</Link>
              <Link href="#solmath"   className="hover:text-white">SolMath</Link>
              <Link href="#isolation" className="hover:text-white">Isolation</Link>
              <a
                href="https://github.com/DJB8787/colosseumfinal/blob/main/halcyon_whitepaper_v9.md"
                target="_blank"
                rel="noreferrer"
                className="hover:text-white"
              >
                Whitepaper
              </a>
            </nav>
            <div className="hidden items-center gap-2 text-[11px] font-medium uppercase tracking-[0.18em] text-white/80 sm:flex">
              <span className="pulse-dot inline-block h-1.5 w-1.5 rounded-full bg-white" />
              Devnet
            </div>
          </header>

          {/* HERO */}
          <section className="bg-halcyon-500 px-6 pt-24 text-white sm:px-14 sm:pt-36">
            <h1 className="font-display text-hero font-black tracking-tightest">
              Structured Products<br />
              That Stay Collateral
            </h1>
            <p className="mt-5 max-w-xl text-sm text-white/85 sm:text-base">
              Halcyon issues tokenised autocallable notes on Solana that you can post as collateral while they earn.
              On-chain pricing. Mechanical buyback. No oracles. No off-chain desk.
            </p>
            <div className="mt-8 flex flex-wrap gap-3">
              <Link
                href="/flagship"
                className="inline-flex items-center rounded-full bg-white px-6 py-3 text-sm font-semibold text-halcyon-700 transition-transform hover:-translate-y-0.5"
              >
                Get a live quote
              </Link>
              <a
                href="https://github.com/DJB8787/colosseumfinal/blob/main/halcyon_whitepaper_v9.md"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center rounded-full border border-white/40 bg-white/[0.06] px-6 py-3 text-sm font-semibold text-white transition-colors hover:bg-white/10"
              >
                Read the whitepaper
              </a>
            </div>

            {/* Stats */}
            <div className="mt-20 grid grid-cols-2 gap-3 sm:grid-cols-4">
              <StatTile value="$120B" label="US autocallable issuance, 2025" />
              <StatTile value="155K"  label="Solana RWA holders, Mar 2026" />
              <StatTile value="0"     label="Insolvencies in 20-year backtest" />
              <StatTile value="14"    label="Decimals agreement vs QuantLib" />
            </div>

            {/* Flagship bar */}
            <div className="mt-20 pb-16 sm:mt-24 sm:pb-20">
              <div className="text-[10px] font-semibold uppercase tracking-[0.3em] text-white/60">
                Flagship underlyings
              </div>
              <div className="mt-3 flex flex-wrap items-center gap-x-10 gap-y-2 text-sm font-semibold text-white/90">
                <span>SPY</span>
                <span>QQQ</span>
                <span>IWM</span>
                <span className="text-white/55">(tokenised, xStocks-class)</span>
                <span className="font-mono text-white/60">solmath v0.1 · crates.io</span>
              </div>
              <div className="mt-5 text-[11px] text-white/55">
                Solo-built by a mathematics teacher. Three pricing engines, one vault, deployed on devnet.
              </div>
            </div>
          </section>

          {/* PROBLEM → SOLUTION (numbered) */}
          <section id="thesis" className="bg-white px-6 py-24 sm:px-14 sm:py-32">
            <div className="text-center">
              <div className="font-mono text-[11px] font-semibold uppercase tracking-[0.24em] text-halcyon-600">
                The thesis
              </div>
              <h2 className="mt-3 font-display text-section font-black tracking-tightest text-ink">
                On-Chain RWAs Have Two Actions.<br />Halcyon Adds The Third.
              </h2>
              <p className="mx-auto mt-5 max-w-2xl text-base text-ink/70">
                Solana's tokenised RWA base crossed $1.7B in early 2026, with 155K holders. Once you hold that
                exposure, there are two things you can do with it. Hold, or borrow against it. That's the entire
                surface area.
              </p>
            </div>

            <div className="mt-16 grid gap-6 md:grid-cols-2 md:gap-10">
              <StepCard
                n="1"
                tone="light"
                label="The problem"
                title="TradFi Notes Die At The Lender"
                bullets={[
                  "A lender needs a mark they can defend and an exit at that price.",
                  "Signed off-chain pricers can't survive an adversarial liquidation dispute.",
                  "So notes become dead capital for the 12–24 months they're alive.",
                ]}
              />
              <StepCard
                n="2"
                tone="blue"
                label="The unlock"
                title="Compute The Mark. Guarantee The Exit."
                bullets={[
                  "Every quote, every coupon, every buyback runs inside the program.",
                  "A standing, mechanical buyback at min(KI−10%, NAV−10%) cannot be refused.",
                  "The note stays posted as collateral, and it stays earning its coupon.",
                ]}
              />
            </div>
          </section>

          {/* MECHANISM — blue slab */}
          <section id="mechanism" className="bg-halcyon-500 px-6 py-24 text-white sm:px-14 sm:py-32">
            <div className="text-center">
              <div className="font-mono text-[11px] font-semibold uppercase tracking-[0.24em] text-white/75">
                The mechanism
              </div>
              <h2 className="mt-3 font-display text-section font-black tracking-tightest">
                A Liquidation Exit Nobody Can Refuse
              </h2>
              <p className="mx-auto mt-5 max-w-2xl text-base text-white/80">
                Each note's buyback is capitalised from the note's own deposit — never shared capital. The vault
                unwinds the hedge, combines it with the USDC reserve, pays the buyback, and cancels the note. The
                vault literally cannot run out.
              </p>
            </div>

            <div className="mt-12 grid gap-4 md:grid-cols-3">
              <BenefitTile
                value="1,160"
                sub="0 failures"
                label="Mechanism-active buybacks in replay — 452 primary + 708 stress"
              />
              <BenefitTile
                value="1.33×"
                sub="min coverage"
                label="Primary-path coverage ratio with stressed hedge-unwind costs"
              />
              <BenefitTile
                value="48h"
                sub="retail path"
                label="Delayed retail redemption forecloses oracle-latency arb"
              />
            </div>

            <div className="mx-auto mt-12 max-w-3xl rounded-2xl bg-white/[0.08] p-6 ring-1 ring-white/15 sm:p-8">
              <div className="font-mono text-[11px] uppercase tracking-[0.18em] text-white/60">
                Buyback price formula
              </div>
              <div className="mt-2 font-mono text-lg text-white sm:text-xl">
                buyback_price = min(KI_level − 10%, current_NAV − 10%)
              </div>
              <p className="mt-4 text-sm text-white/70">
                Pre-KI at healthy NAV the price is capped at KI−10% — a deterministic ceiling lenders use to set LTV.
                Post-KI the price follows NAV down at a 10% haircut. Surfaced in the UI as <em>Lending Value</em>.
              </p>
            </div>
          </section>

          {/* SHELF — gateway-style with code panels */}
          <section id="shelf" className="bg-white px-6 py-24 sm:px-14 sm:py-32">
            <div className="grid gap-14 md:grid-cols-2 md:items-start">
              <div>
                <div className="font-mono text-[11px] font-semibold uppercase tracking-[0.24em] text-halcyon-600">
                  The shelf
                </div>
                <h2 className="mt-3 font-display text-section font-black tracking-tightest text-ink">
                  Three Products.<br />One Vault. One Pricer.
                </h2>
                <p className="mt-5 max-w-md text-base text-ink/70">
                  Three structurally-uncorrelated failure modes share a single underwriting vault. Every year in
                  the 2020–2024 overlap window is portfolio-positive, including 2022 when SOL fell 94% and
                  equities bear-marketed.
                </p>

                <div className="mt-8 grid grid-cols-3 gap-3">
                  <ProductTile n="01" title="Equity Autocall" sub="SPY · QQQ · IWM"    href="/flagship" />
                  <ProductTile n="02" title="IL Protection"  sub="SOL / USDC · 30d"   href="/il-protection" />
                  <ProductTile n="03" title="SOL Autocall"   sub="Principal-backed"   href="/sol-autocall" />
                </div>

                <ul className="mt-8 space-y-3 text-sm text-ink/75">
                  <ShelfRow
                    title="Flagship: 18mo worst-of-3 autocall on SPY/QQQ/IWM"
                    facts="14.9–15.1% coupon · 22% KI · +5.2–5.6% vault ROC · 0 insolvencies"
                  />
                  <ShelfRow
                    title="IL Protection: 30-day tail insurance for Raydium LPs"
                    facts="1% deductible · 7% cap · +2.2–5.1% vault ROC · NIG Gauss-Legendre"
                  />
                  <ShelfRow
                    title="SOL Autocall: 16-day, 8-obs principal-backed note"
                    facts="Positive every backtested year · POD-DEIM pricer · 946K CU/quote"
                  />
                </ul>

                <div className="mt-10 flex flex-wrap gap-3">
                  <Link
                    href="/flagship"
                    className="inline-flex items-center rounded-full bg-halcyon-500 px-6 py-3 text-sm font-semibold text-white transition-colors hover:bg-halcyon-600"
                  >
                    Quote the flagship
                  </Link>
                  <a
                    href="https://github.com/DJB8787/colosseumfinal/blob/main/halcyon_whitepaper_v9.md"
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 rounded-full px-4 py-3 text-sm font-semibold text-ink/70 hover:text-ink"
                  >
                    unit economics ↗
                  </a>
                </div>
              </div>

              {/* Representative on-chain responses */}
              <div className="space-y-3">
                <CodeSnippet
                  endpoint="flagship_quote — SPY/QQQ/IWM worst-of-3"
                  body='{ "coupon_annualised_bps": 1490, "ki_barrier_pct": 80, "autocall": "quarterly" }'
                />
                <CodeSnippet
                  endpoint="lending_value — mid-life buyback preview"
                  body='{ "buyback_usdc": 720.00, "rule": "min(KI-10%, NAV-10%)" }'
                />
                <CodeSnippet
                  endpoint="il_protection_quote — 30d Raydium LP cover"
                  body='{ "deductible_pct": 1, "cap_pct": 7, "premium_bps": 310 }'
                />
                <CodeSnippet
                  endpoint="sol_autocall_quote — 16d principal-backed"
                  body='{ "coupon_per_obs_bps": 160, "lockout_obs": 2, "ki_pct": 70 }'
                />
                <CodeSnippet
                  endpoint="solmath::greeks — Black-Scholes, all five"
                  body='{ "delta": 0.5418, "gamma": 0.0281, "vega": 0.0832, "theta": -0.0197, "rho": 0.2034 }'
                />
              </div>
            </div>
          </section>

          {/* SOLMATH — navy */}
          <section id="solmath" className="bg-halcyon-950 px-6 py-24 text-white sm:px-14 sm:py-32">
            <div className="grid gap-12 md:grid-cols-[1.2fr_1fr] md:items-end">
              <div>
                <div className="font-mono text-[11px] font-semibold uppercase tracking-[0.24em] text-halcyon-200">
                  The quant library
                </div>
                <h2 className="mt-3 font-display text-section font-black tracking-tightest">
                  Built On <span className="font-mono text-[0.78em] text-halcyon-300">solmath</span>
                </h2>
                <p className="mt-5 max-w-xl text-base text-white/75 sm:text-lg">
                  Fixed-point transcendentals (ln 22× faster than <code className="font-mono text-white/90">rust_decimal</code>),
                  Bessel K₁ from Abramowitz & Stegun, bivariate-normal CDF, NIG density, Black-Scholes with all
                  five Greeks, four barrier types, Fang-Oosterlee COS with a corrected asymmetric recursion.
                  <code className="ml-1 font-mono text-white/90">no_std</code>, zero deps, ~45KB BPF, published on crates.io.
                </p>
                <div className="mt-7 inline-flex items-center gap-2 rounded-full border border-white/20 bg-white/[0.06] px-4 py-2.5 font-mono text-sm text-white">
                  <span className="text-halcyon-300">$</span> cargo add solmath
                </div>
              </div>
              <ul className="grid gap-2 text-sm text-white/85">
                <ValRow k="QuantLib"   v="14-decimal agreement · 2.5M vectors" />
                <ValRow k="mpmath"     v="50-digit precision reference" />
                <ValRow k="scipy"      v="baseline cross-check" />
                <ValRow k="BPF"        v="same binary on-chain & off-chain" />
                <ValRow k="Compute"    v="1.4M CU budget · 946K/quote (SOL AC)" />
                <ValRow k="Cost"       v="$0.01–$0.05 per quote on Solana" />
              </ul>
            </div>
          </section>

          {/* ISOLATION */}
          <section id="isolation" className="bg-white px-6 py-24 sm:px-14 sm:py-32">
            <div className="grid gap-12 md:grid-cols-[1fr_1.1fr] md:items-start">
              <div>
                <div className="font-mono text-[11px] font-semibold uppercase tracking-[0.24em] text-halcyon-600">
                  Post-Drift architecture
                </div>
                <h2 className="mt-3 font-display text-section font-black tracking-tightest text-ink">
                  No Perps.<br />No Bridges.<br />No Protocol Deps.
                </h2>
                <p className="mt-5 max-w-md text-base text-ink/70">
                  On 1 Apr 2026, Drift was drained of $286M through social-engineered admin access. In that
                  environment, every external dependency is an attack vector. Halcyon hedges with spot tokens
                  and DEX swaps only. One named counterparty: the xStocks-class wrapper for the flagship.
                </p>
              </div>

              <div className="rounded-2xl border border-black/[0.06] bg-white/80 p-2">
                <DepRow product="Flagship autocall" hedge="Spot SPY + QQQ wrappers (IWM projected)" dep="xStocks-class issuer" />
                <DepRow product="IL Protection"    hedge="Unhedged · vault takes the risk"          dep="None"                    last />
                <DepRow product="SOL Autocall"     hedge="Spot SOL via Jupiter / Raydium"           dep="DEX execution only"      />
              </div>
            </div>
          </section>

          {/* FOLLOW — blue callout with pill socials */}
          <section className="bg-white px-6 pb-20 pt-10 sm:px-14 sm:pt-16">
            <div className="rounded-[24px] bg-halcyon-500 px-6 py-14 text-center text-white sm:rounded-[30px] sm:py-16">
              <h3 className="font-display text-3xl font-black tracking-tightest sm:text-4xl">
                Follow Halcyon
              </h3>
              <p className="mx-auto mt-3 max-w-md text-sm text-white/80">
                Colosseum submission, April 2026. Mainnet gated behind regulated issuance partnership.
              </p>
              <div className="mt-6 flex flex-wrap justify-center gap-3">
                <Pill icon="📄"  label="Whitepaper" href="https://github.com/DJB8787/colosseumfinal/blob/main/halcyon_whitepaper_v9.md" />
                <Pill icon="</>" label="GitHub"     href="https://github.com/DJB8787/colosseumfinal" />
                <Pill icon="📦"  label="solmath"    href="https://crates.io/crates/solmath" />
                <Pill icon="🏛"   label="Architecture" href="https://github.com/DJB8787/colosseumfinal/blob/main/ARCHITECTURE.md" />
                <Pill icon="✒"    label="Thesis pivot" href="https://github.com/DJB8787/colosseumfinal/blob/main/thesispivot.md" />
              </div>
            </div>
          </section>

          {/* FOOTER */}
          <footer className="bg-white px-6 pb-14 pt-8 sm:px-14">
            <div className="grid gap-10 border-t border-ink/10 pt-12 md:grid-cols-5">
              <div>
                <div className="font-display text-lg font-black tracking-tightest text-ink">Halcyon</div>
                <p className="mt-3 max-w-[14rem] text-sm text-ink/65">
                  The composable structured-product layer for on-chain RWAs on Solana.
                </p>
                <p className="mt-4 text-xs text-ink/45">
                  Solo-built. Solana Colosseum April 2026.
                </p>
              </div>
              <FooterCol title="Protocol" items={["Thesis", "Mechanism", "Shelf", "SolMath", "Isolation"]} />
              <FooterCol
                title="Shelf"
                items={[
                  { label: "Equity Autocall" },
                  { label: "IL Protection" },
                  { label: "SOL Autocall" },
                  { label: "Lending demo", pill: "DEVNET", sub: "Receipt-token collateral" },
                ]}
              />
              <FooterCol
                title="Build"
                items={[
                  { label: "Whitepaper v9" },
                  { label: "Thesis pivot" },
                  { label: "Architecture" },
                  { label: "solmath · crates.io" },
                  { label: "Kamino / MarginFi",  pill: "NEXT", sub: "Lending integration targets" },
                ]}
              />
              <FooterCol title="Open problems" items={["Regulated issuance", "Wrapper liquidity", "Distribution"]} />
            </div>
            <div className="mt-10 flex flex-wrap items-center justify-between gap-4 border-t border-ink/10 pt-6 text-xs text-ink/50">
              <span>© 2026 Halcyon. Not investment advice. Devnet protocol.</span>
              <span className="flex gap-5">
                <a className="hover:text-ink" href="#">Threat model</a>
                <a className="hover:text-ink" href="#">Risk disclosures</a>
              </span>
            </div>
          </footer>
        </div>
      </div>
    </main>
  );
}

function StatTile({ value, label }: { value: string; label: string }) {
  return (
    <div className="rounded-2xl bg-white/[0.08] px-5 py-5 ring-1 ring-white/15 backdrop-blur-[1px]">
      <div className="font-display text-3xl font-black tracking-tightest sm:text-4xl">{value}</div>
      <div className="mt-2 text-[11px] font-medium uppercase tracking-[0.14em] text-white/65">{label}</div>
    </div>
  );
}

function BenefitTile({ value, sub, label }: { value: string; sub: string; label: string }) {
  return (
    <div className="rounded-2xl bg-white/[0.08] px-6 py-7 ring-1 ring-white/15">
      <div className="flex items-baseline gap-2">
        <div className="font-display text-4xl font-black tracking-tightest sm:text-5xl">{value}</div>
        <div className="text-sm text-white/70">{sub}</div>
      </div>
      <div className="mt-3 text-sm text-white/80">{label}</div>
    </div>
  );
}

function StepCard({
  n,
  tone,
  label,
  title,
  bullets,
}: {
  n: string;
  tone: "light" | "blue";
  label: string;
  title: string;
  bullets: string[];
}) {
  const isBlue = tone === "blue";
  return (
    <div className={`relative rounded-2xl px-8 py-8 ring-1 ${
      isBlue
        ? "bg-halcyon-500 text-white ring-halcyon-500/20 shadow-[0_20px_40px_-20px_rgba(10,102,160,0.45)]"
        : "bg-white text-ink ring-black/[0.05]"
    }`}>
      <div className={`absolute -top-4 left-8 inline-flex h-8 w-8 items-center justify-center rounded-full ring-1 ${
        isBlue
          ? "bg-white text-halcyon-700 ring-white/80"
          : "bg-white text-ink/80 ring-black/10"
      } text-xs font-bold`}>
        {n}
      </div>
      <div className={`font-mono text-[11px] font-semibold uppercase tracking-[0.22em] ${
        isBlue ? "text-white/75" : "text-halcyon-600"
      }`}>
        {label}
      </div>
      <div className={`mt-3 font-display text-2xl font-black tracking-tightest sm:text-3xl ${
        isBlue ? "text-white" : "text-ink"
      }`}>
        {title}
      </div>
      <ul className={`mt-5 space-y-2 text-sm ${isBlue ? "text-white/85" : "text-ink/75"}`}>
        {bullets.map((b, i) => (
          <li key={i} className="flex gap-2">
            <span className={isBlue ? "text-white/55" : "text-halcyon-500"}>•</span>
            <span>{b}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function ProductTile({ n, title, sub, href }: { n: string; title: string; sub: string; href: string }) {
  return (
    <Link href={href} className="group rounded-xl border border-black/[0.06] bg-white px-3 py-3 transition-colors hover:border-halcyon-500/30 hover:bg-halcyon-50/60">
      <div className="font-mono text-[10px] text-ink/45">{n}</div>
      <div className="mt-1 text-[13px] font-bold text-ink">{title}</div>
      <div className="text-[11px] text-ink/55">{sub}</div>
    </Link>
  );
}

function ShelfRow({ title, facts }: { title: string; facts: string }) {
  return (
    <li className="border-l-2 border-halcyon-500/40 pl-4">
      <div className="text-[14px] font-semibold text-ink">{title}</div>
      <div className="mt-0.5 font-mono text-[12px] text-ink/55">{facts}</div>
    </li>
  );
}

function CodeSnippet({ endpoint, body }: { endpoint: string; body: string }) {
  return (
    <div className="rounded-xl border border-black/[0.06] bg-white/80 px-4 py-3 shadow-[0_4px_10px_-6px_rgba(0,0,0,0.08)]">
      <div className="font-mono text-[12px] text-ink/55">{endpoint}</div>
      <div className="mt-1.5 font-mono text-[13px] text-ink/85">{body}</div>
    </div>
  );
}

function DepRow({ product, hedge, dep, last }: { product: string; hedge: string; dep: string; last?: boolean }) {
  return (
    <div className={`grid grid-cols-1 gap-1 px-4 py-4 sm:grid-cols-[1.1fr_1.6fr_1fr] sm:items-baseline sm:gap-6 ${last ? "" : "border-b border-ink/5"}`}>
      <div className="text-sm font-semibold text-ink">{product}</div>
      <div className="text-sm text-ink/75">{hedge}</div>
      <div className="font-mono text-[12px] text-halcyon-600">{dep}</div>
    </div>
  );
}

function ValRow({ k, v }: { k: string; v: string }) {
  return (
    <li className="flex items-baseline justify-between border-b border-white/10 pb-2 last:border-b-0">
      <span className="font-display text-base font-bold tracking-tightest">{k}</span>
      <span className="text-sm text-white/70">{v}</span>
    </li>
  );
}

function Pill({ icon, label, href }: { icon: string; label: string; href: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-2 rounded-full border border-white/35 bg-white/[0.06] px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-white/15"
    >
      <span className="text-white/85">{icon}</span>
      {label}
    </a>
  );
}

type FooterItem = string | { label: string; pill?: string; sub?: string };

function FooterCol({ title, items }: { title: string; items: FooterItem[] }) {
  return (
    <div>
      <div className="text-sm font-bold text-ink">{title}</div>
      <ul className="mt-4 space-y-3 text-sm text-ink/70">
        {items.map((it, i) => {
          if (typeof it === "string") {
            return (
              <li key={i}>
                <a href="#" className="hover:text-ink">{it}</a>
              </li>
            );
          }
          return (
            <li key={i}>
              <div className="flex items-center gap-2">
                <a href="#" className="hover:text-ink">{it.label}</a>
                {it.pill && (
                  <span className="rounded-full bg-halcyon-50 px-1.5 py-0.5 font-mono text-[9px] font-semibold uppercase tracking-[0.14em] text-halcyon-700">
                    {it.pill}
                  </span>
                )}
              </div>
              {it.sub && <div className="mt-0.5 text-xs text-ink/45">{it.sub}</div>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

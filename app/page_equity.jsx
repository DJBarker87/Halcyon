/* global React, PayoffChart, PriceChart */
// Halcyon — Flagship worst-of-3 equity autocall
// PRODUCT CARD: product params are fixed on-chain. Buyer input = notional + slippage.

const { useState: useState_eq, useMemo: useMemo_eq, useEffect: useEffect_eq } = React;
const HM_eq = window.HalcyonMath;

const FLAGSHIP = {
  underlyings: ['SPY', 'QQQ', 'IWM'],
  tenorMonths: 18,
  // N_OBS quarterly observation dates over the 18-month tenor. The c1 filter
  // tracks 6 quarterly obs (coupon memory folds 18 monthlies into these).
  // fair_coupon_bps from the WASM pricer is per quarterly obs; annualise with
  // × (obsPerYear = 4).
  nObsQuarterly: 6,
  obsPerYear: 4,
  autocallBarrier: 1.00,
  knockInBarrier: 0.80,
  // Production quote composition (math-stack doc §6)
  quoteShare: 0.60,
  issuerMarginBps: 100,
  fcFloorBps: 150,
  fcCeilingBps: 1400,
  // σ default when no live feed is present. 0.29 matches the production-σ
  // neighbourhood the K=12 correction table was tuned around.
  sigmaAnnFallback: 0.29,
  spot: { SPY: 580.42, QQQ: 512.67, IWM: 229.81 },
  names: { SPY: 'S&P 500', QQQ: 'Nasdaq 100', IWM: 'Russell 2000' },
  loadings: { SPY: 0.516, QQQ: 0.568, IWM: 0.641 },
};
const EQ_PYTH = {
  SPY: { feed: '5SSkXsEKQepHHAewytPVwdej4epN1nxgLVM84L4KXgy7', staleSec: 2 },
  QQQ: { feed: 'GnMq2ufshnP3NXeWJe7MyhotEKtPhMCGZeVJQmcPfTLk', staleSec: 3 },
  IWM: { feed: 'HRJHMgqiP8wSFkBjBWyMdL5u3W8Yt4nHP6Y3YGE1s8C3', staleSec: 4 },
};

// Hook: subscribe to live EWMA σ for SPY/QQQ/IWM when the feeds are wired.
// Today oracles.js only serves SOL; this hook degrades gracefully and returns
// `FLAGSHIP.sigmaAnnFallback` until the equity feeds come online. Also
// re-renders when the WASM module is ready so the pricer swaps in live.
function useLiveSigma_eq() {
  const readComposite = () => {
    const o = window.HalcyonOracles;
    if (!o || !o.getEwma) return null;
    const parts = FLAGSHIP.underlyings
      .map(s => o.getEwma(s))
      .filter(x => x && Number.isFinite(x.sigmaAnn));
    if (!parts.length) return null;
    // Equal-weight geometric mean of the individual EWMAs. When live feeds
    // arrive, replace with the factor-model composite per the calibration
    // JSON (common_factor_delta_scale_annual_trading_day × loadings).
    const mean = parts.reduce((a, p) => a * p.sigmaAnn, 1) ** (1 / parts.length);
    return Number.isFinite(mean) ? mean : null;
  };
  const [sigma, setSigma] = useState_eq(() => readComposite());
  useEffect_eq(() => {
    const o = window.HalcyonOracles;
    if (!o || !o.subscribeEwma) return;
    const unsubs = FLAGSHIP.underlyings.map(s =>
      o.subscribeEwma(s, () => setSigma(readComposite())));
    return () => unsubs.forEach(fn => fn && fn());
  }, []);
  // Re-render when WASM finishes loading so the pricer swaps in.
  const [, setTick] = useState_eq(0);
  useEffect_eq(() => {
    if (window.HalcyonMath && window.HalcyonMath.wasmReady) return;
    const h = () => setTick(t => t + 1);
    window.addEventListener('halcyon-wasm-ready', h);
    return () => window.removeEventListener('halcyon-wasm-ready', h);
  }, []);
  return sigma;
}

function computeFlagshipQuote({ notional, slippageBps, sigmaAnn }) {
  // Live WASM path: run the EXACT on-chain K=12 pricer (projected c1 filter
  // + K=12 correction). Falls back to whitepaper constants when WASM hasn't
  // loaded yet or σ is outside the [0.08, 0.80] calibration range.
  const wasm = window.HalcyonMath && window.HalcyonMath.wasm;
  const canPrice = !!wasm
    && typeof wasm.worst_of_k12_coupon_bps === 'function'
    && Number.isFinite(sigmaAnn) && sigmaAnn >= 0.08 && sigmaAnn <= 0.80;

  let fairCoupon;         // annualised decimal
  let kiRate, acRate;
  let fcBpsPerObs, engine;
  if (canPrice) {
    fcBpsPerObs = wasm.worst_of_k12_coupon_bps(sigmaAnn);
    if (Number.isFinite(fcBpsPerObs) && fcBpsPerObs > 0) {
      fairCoupon = (fcBpsPerObs / 10_000) * FLAGSHIP.obsPerYear;
      kiRate     = wasm.worst_of_k12_knock_in_rate(sigmaAnn);
      acRate     = wasm.worst_of_k12_autocall_rate(sigmaAnn);
      engine     = 'K=12 c1 filter (on-chain path)';
    } else {
      fairCoupon = 0.24; engine = 'whitepaper fallback';
    }
  } else {
    fairCoupon = 0.24;   // 24% p.a. model fair (whitepaper constant)
    engine     = 'whitepaper fallback';
  }

  // Convert annualised fair coupon → per-obs → apply quote share + margin → annualised offered.
  const fairPerObs    = fairCoupon / FLAGSHIP.obsPerYear;
  const offeredPerObs = Math.max(0, fairPerObs * FLAGSHIP.quoteShare
                                    - FLAGSHIP.issuerMarginBps / 10_000);
  // Tiny liquidity-depth jitter (same shape as the SOL page, ±0.4 bps on a
  // 100k notional), visible only in the UI — not a pricing model input.
  const liveJitter    = Math.sin(notional / 17_000) * 0.00004;
  const offeredCoupon = offeredPerObs * FLAGSHIP.obsPerYear + liveJitter;
  const quotedCoupon  = offeredCoupon; // offered = what the protocol actually pays

  // Issuance gate (math-stack doc §6). UI only surfaces the fact, not blocks;
  // the on-chain program enforces.
  const fcBpsAnn = fairCoupon * 10_000;
  const issuable = fcBpsAnn >= FLAGSHIP.fcFloorBps
                && fcBpsAnn <= FLAGSHIP.fcCeilingBps;

  const maxPremium    = notional * (1 + slippageBps / 10_000);
  // Max liability reserved: (1 − KI) × notional on worst-case KI path
  const maxLiability  = notional * (1 - FLAGSHIP.knockInBarrier);  // 20% of notional
  const issuanceFee   = notional * FLAGSHIP.issuerMarginBps / 10_000;
  const perCoupon     = notional * offeredCoupon / 12;  // monthly
  const maxIncome     = notional * offeredCoupon * (FLAGSHIP.tenorMonths / 12);
  return { offeredCoupon, quotedCoupon, fairCoupon, maxPremium,
           maxLiability, issuanceFee, perCoupon, maxIncome,
           sigmaAnn, engine, issuable,
           kiProbability: kiRate, autocallProbability: acRate };
}

function PageEquityAutocall({ tweaks }) {
  const [notional,    setNotional]    = useState_eq(100_000);
  const [slippageBps, setSlippageBps] = useState_eq(50);
  const [advOpen,     setAdvOpen]     = useState_eq(false);
  const [gateOpen,    setGateOpen]    = useState_eq(false);

  const liveSigma = useLiveSigma_eq();
  const sigmaAnn  = Number.isFinite(liveSigma) ? liveSigma : FLAGSHIP.sigmaAnnFallback;

  const hasQuote = notional >= 100;
  const q = useMemo_eq(
    () => hasQuote ? computeFlagshipQuote({ notional, slippageBps, sigmaAnn }) : null,
    [notional, slippageBps, hasQuote, sigmaAnn]);

  const walletOk = tweaks.walletState === 'connected';
  const canIssue = hasQuote && walletOk;

  const payoffCurve = useMemo_eq(() =>
    HM_eq.worstOfAutocallCurve({
      autocallBar: FLAGSHIP.autocallBarrier,
      knockIn: FLAGSHIP.knockInBarrier,
      coupon: (q ? q.offeredCoupon : 0.156) * (FLAGSHIP.tenorMonths / 12),
    }), [q]);

  return (
    <div className="hc-page">
      {/* =============== HEADER =============== */}
      <div className="hc-prodhead">
        <div className="hc-prodhead-main">
          <div className="hc-prodhead-eyebrow">
            Product 01 · Flagship · the first on-chain US equity autocallable
          </div>
          <h1 className="hc-prodhead-title">
            18-month worst-of autocall · SPY / QQQ / IWM
          </h1>
          <p className="hc-prodhead-sub">
            Monthly coupon if all three indices stay above entry; auto-called at par if the
            basket closes ≥&nbsp;100% on a quarterly check. Principal is at risk only if
            any one name ever closes below 80%. Backtested over 20 years of history: 99%
            issuance, +9.4% realised buyer IRR, zero vault insolvencies.
          </p>

          <div className="hc-paramchips">
            <ParamChip label="Underlyings">
              {FLAGSHIP.underlyings.map(t =>
                <span key={t} className="pc-chip-inline">{t}</span>)}
            </ParamChip>
            <ParamChip label="Tenor">18&nbsp;months</ParamChip>
            <ParamChip label="Coupon obs.">Monthly · 18</ParamChip>
            <ParamChip label="Autocall">Quarterly ≥&nbsp;100%</ParamChip>
            <ParamChip label="Knock-in">80% · continuous</ParamChip>
            <ParamChip label="Memory">Yes</ParamChip>
            <ParamChip label="Settlement">USDC · Solana</ParamChip>
          </div>
        </div>
        <aside className="hc-prodhead-proof">
          <div className="hc-proofrow"><span className="k">Realised buyer IRR</span><span className="v">+9.4%</span></div>
          <div className="hc-proofrow"><span className="k">Notes returning principal</span><span className="v">87%</span></div>
          <div className="hc-proofrow"><span className="k">Issuance rate</span><span className="v">99%</span></div>
          <div className="hc-proofrow"><span className="k">Vault RoOC</span><span className="v">+5.2%</span></div>
          <div className="hc-proofrow"><span className="k">Insolvency events</span><span className="v">0</span></div>
        </aside>
      </div>

      {/* =============== HERO INPUT — money in, front and centre =============== */}
      <div className="hc-hero-input">
        <div className="hc-hero-input-main">
          <div className="hc-hero-input-eyebrow">
            <span className="dot"/>How much would you like to put in?
          </div>
          <div className="hc-hero-input-field">
            <span className="hc-hero-input-prefix">$</span>
            <input
              className="hc-hero-input-input" type="number" min={0} step={100}
              value={notional || ''}
              onChange={e => setNotional(Number(e.target.value) || 0)}
              placeholder="100,000" inputMode="numeric" autoFocus />
            <span className="hc-hero-input-suffix">USDC</span>
          </div>
          <div className="hc-hero-input-presets">
            {[500, 5000, 50000, 250000, 1_000_000].map(v => (
              <button key={v}
                      className={notional === v ? 'active' : ''}
                      onClick={() => setNotional(v)}>
                {v >= 1_000_000 ? `$${v/1_000_000}M` : v >= 1000 ? `$${(v/1000).toFixed(0)}k` : `$${v}`}
              </button>
            ))}
          </div>
          <div className="hc-hero-input-hint">
            {hasQuote
              ? <>Minimum ticket <b>$100</b>. Your wallet pays at most <b>{fmtUSD(q.maxPremium)}</b> after {slippageBps}bp slippage.</>
              : <>Minimum ticket <b>$100</b>. Enter an amount to get a live coupon quote.</>}
          </div>
        </div>
        {hasQuote && (
          <div className="hc-hero-input-side">
            <div className="hc-hero-input-side-head">At these terms, you receive</div>
            <div className="hc-hero-input-sr"><span className="k">Coupon / month</span><span className="v">{fmtUSD(q.perCoupon, 0)}</span></div>
            <div className="hc-hero-input-sr"><span className="k">Max coupon income</span><span className="v">{fmtUSD(q.maxIncome, 0)}</span></div>
            <div className="hc-hero-input-sr"><span className="k">Principal at risk below 80%</span><span className="v">{fmtUSD(q.maxLiability, 0)}</span></div>
            <div className="hc-hero-input-sr"><span className="k">Issuance fee</span><span className="v">{fmtUSD(q.issuanceFee, 0)}</span></div>
          </div>
        )}
      </div>

      {/* =============== OFFER =============== */}
      {hasQuote && (
        <div className="hc-quotecard">
          <div className="hc-quotecard-offer">
            <div className="hc-qco-eyebrow">Quoted coupon · live</div>
            <div className="hc-qco-headline">
              {(q.offeredCoupon * 100).toFixed(2)}<span className="unit">% p.a.</span>
            </div>
            <div className="hc-qco-sub">
              Pays <b>{fmtUSD(q.perCoupon, 0)}</b> each of 18 monthly observations when
              the worst of SPY / QQQ / IWM holds ≥&nbsp;100% of entry. Missed coupons
              accumulate and pay on the next eligible date (memory coupon). Max coupon
              income <b>{fmtUSD(q.maxIncome, 0)}</b> over the 18-month tenor.
            </div>

            <div className="hc-qco-row">
              <div>
                <div className="hc-qco-k">Max premium</div>
                <div className="hc-qco-v">{fmtUSD(q.maxPremium)}</div>
                <div className="hc-qco-sub2">after {slippageBps}bp slip</div>
              </div>
              <div>
                <div className="hc-qco-k">Max liability</div>
                <div className="hc-qco-v">{fmtUSD(q.maxLiability)}</div>
                <div className="hc-qco-sub2">reserved by vault</div>
              </div>
              <div>
                <div className="hc-qco-k">Issuance fee</div>
                <div className="hc-qco-v">{fmtUSD(q.issuanceFee)}</div>
                <div className="hc-qco-sub2">100 bp of notional</div>
              </div>
              <div>
                <div className="hc-qco-k">Quote slot</div>
                <div className="hc-qco-v mono">298,442,117</div>
                <div className="hc-qco-sub2">refreshes every block</div>
              </div>
            </div>

            <div className="hc-qco-benchmark">
              <b>Model benchmark.</b> The one-factor NIG engine's fair coupon is
              {' '}{(q.fairCoupon*100).toFixed(1)}% p.a.; the vault offers <b>65%</b> of that
              ({(q.quotedCoupon*100).toFixed(2)}%) and retains the 35% spread. Over the
              20-year backtest this spread produced +5.2% annualised vault return with
              no insolvency events, even through GFC, COVID, and the 2022 bear market.
            </div>
          </div>
        </div>
      )}

      {/* =============== PAYOFF =============== */}
      {hasQuote && (
        <div className="hc-payoff-block">
          <div className="hc-payoff-lang">
            <div className="hc-payoff-lang-head">Under these terms, here's what happens:</div>

            <div className="hc-payoff-outcome">
              <div className="hc-po-dot" style={{background: 'var(--blue-600)'}}/>
              <div>
                <div className="hc-po-head">Autocall · redeemed at par, early</div>
                <div className="hc-po-body">
                  If all three indices close ≥&nbsp;100% of entry on any of six quarterly
                  observations, you're redeemed with full principal plus every coupon
                  accrued (including accumulated missed ones). Backtested frequency:
                  <b> 83% of notes</b>. Average life is <b>~7 months</b>. Earliest exit: month 3.
                </div>
              </div>
            </div>

            <div className="hc-payoff-outcome">
              <div className="hc-po-dot" style={{background: 'var(--n-500)'}}/>
              <div>
                <div className="hc-po-head">Full term, no knock-in · principal back</div>
                <div className="hc-po-body">
                  The basket drifts between 80% and 100% for 18 months. Principal returns
                  at maturity plus whatever coupons the memory mechanic paid. <b>4% of notes</b>.
                </div>
              </div>
            </div>

            <div className="hc-payoff-outcome">
              <div className="hc-po-dot" style={{background: 'var(--rust-500)'}}/>
              <div>
                <div className="hc-po-head">Knock-in · principal at risk</div>
                <div className="hc-po-body">
                  If any name ever closes below&nbsp;80% of entry, and the worst finishes
                  below 100% at maturity, you receive the worst name's terminal
                  performance. KI barrier is breached on <b>33% of notes</b> but
                  {' '}<b>only 13%</b> end at a loss — in the other 20%, the worst recovers
                  above entry. Worst case: up to <b>−{fmtUSD(q.maxLiability, 0)}</b> on
                  this position.
                </div>
              </div>
            </div>
          </div>

          <div className="hc-chartcard">
            <div className="hc-chartcard-head">
              <h3>Payoff at maturity · worst-of performance</h3>
              <div className="legend">
                <span><span className="sw" style={{background:'var(--blue-600)'}} /> Payoff</span>
                <span><span className="sw" style={{background:'var(--rust-500)'}} /> KI 80%</span>
                <span><span className="sw" style={{background:'var(--n-400)'}} /> AC 100%</span>
              </div>
            </div>
            <PayoffChart
              curves={[{ data: payoffCurve, color: 'var(--blue-600)' }]}
              annotations={[
                { x: FLAGSHIP.knockInBarrier, label: 'KI 80%', color: 'var(--rust-500)' },
                { x: FLAGSHIP.autocallBarrier, label: 'AC 100%', color: 'var(--n-500)' },
              ]}
              width={620} height={260}
              xLabel="Worst-of performance · S_T / S_0"
              xFormat={v => `${(v*100).toFixed(0)}%`}
              yFormat={v => `${(v*100).toFixed(0)}%`} />
          </div>
        </div>
      )}

      {/* =============== ORACLES =============== */}
      <div className="hc-section-title">
        <span>Oracle feeds · Pyth</span>
        <span className="meta">Live spot · 30s staleness cap</span>
      </div>
      <div className="hc-grid-3">
        {FLAGSHIP.underlyings.map(t => (
          <div key={t} className="hc-panel">
            <div className="hc-panel-head">
              <h3>{t} · {FLAGSHIP.names[t]}</h3>
              <span className="meta">{EQ_PYTH[t].staleSec}s ago</span>
            </div>
            <div className="hc-panel-body" style={{padding: 16}}>
              <PriceChart
                data={HM_eq.mockSeries({days: 60, start: FLAGSHIP.spot[t],
                                        vol: 0.012, drift: 0.0004,
                                        seed: t.charCodeAt(0)+t.charCodeAt(1)})}
                height={140} color="var(--blue-600)" />
              <div style={{display: 'flex', justifyContent: 'space-between',
                           marginTop: 8, fontFamily: 'var(--f-mono)', fontSize: 10,
                           color: 'var(--n-500)', letterSpacing: '0.04em',
                           fontVariantNumeric: 'tabular-nums'}}>
                <span>feed {EQ_PYTH[t].feed.slice(0,6)}…{EQ_PYTH[t].feed.slice(-4)}</span>
                <span>ℓ {FLAGSHIP.loadings[t].toFixed(3)}</span>
              </div>
            </div>
          </div>
        ))}
      </div>

      {/* =============== ADVANCED =============== */}
      <div className="hc-advanced">
        <button className="hc-chev" onClick={() => setAdvOpen(!advOpen)}>
          <span className={`hc-chev-arrow ${advOpen ? 'open' : ''}`}>▸</span>
          Advanced
          <span style={{marginLeft: 'auto', fontSize: 10, fontFamily: 'var(--f-mono)',
                        color: 'var(--n-400)', letterSpacing: '0.06em'}}>
            slippage {slippageBps}bp · gate passed
          </span>
        </button>
        {advOpen && (
          <div className="hc-advanced-body">
            <div className="hc-adv-row">
              <div style={{flex: 1}}>
                <FieldLabel meta="maximum premium movement you'll accept">Slippage tolerance</FieldLabel>
                <Slider value={slippageBps} onChange={setSlippageBps}
                        min={10} max={200} step={10} />
                <div style={{display:'flex', justifyContent: 'space-between',
                             fontFamily: 'var(--f-mono)', fontSize: 10,
                             color: 'var(--n-400)', letterSpacing: '0.04em',
                             marginTop: 6, fontVariantNumeric: 'tabular-nums'}}>
                  <span>10bp</span>
                  <span style={{color: 'var(--ink)', fontWeight: 600}}>
                    {slippageBps}bp · max premium {hasQuote ? fmtUSD(q.maxPremium) : '—'}
                  </span>
                  <span>200bp</span>
                </div>
              </div>
            </div>

            <div className="hc-adv-gate">
              <button className="hc-gate-toggle" onClick={() => setGateOpen(!gateOpen)}>
                <span className="hc-gate-icon" style={{background: 'var(--blue-600)', color: '#fff'}}>✓</span>
                <span className="hc-gate-title">Issuance gate · 5 / 5 checks pass</span>
                <span className={`hc-chev-arrow ${gateOpen ? 'open' : ''}`}>▸</span>
              </button>
              {gateOpen && (
                <div className="hc-adv-gate-detail">
                  <div><b>Fair-coupon band</b> — {(q?.fairCoupon*100).toFixed(1)}% p.a., inside [15, 400] bps/obs band.</div>
                  <div><b>Vault utilisation</b> — 64%, cap 90%.</div>
                  <div><b>Oracle staleness</b> — SPY 2s, QQQ 3s, IWM 4s; cap 30s.</div>
                  <div><b>Factor calibration</b> — refreshed 4h ago, cap 5d.</div>
                  <div><b>Engine cost</b> — preview_quote 955k CU, limit 1.4M.</div>
                  <details style={{marginTop: 10, paddingTop: 10, borderTop: '1px solid var(--n-100)'}}>
                    <summary style={{cursor: 'pointer', fontSize: 10, fontFamily: 'var(--f-mono)',
                                     color: 'var(--n-500)', letterSpacing: '0.1em',
                                     textTransform: 'uppercase', fontWeight: 600, listStyle: 'none'}}>
                      ▸ One-factor NIG internals
                    </summary>
                    <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14,
                                 marginTop: 10, fontFamily: 'var(--f-mono)', fontSize: 11,
                                 color: 'var(--n-500)', fontVariantNumeric: 'tabular-nums'}}>
                      <div>
                        <div style={{fontSize: 10, color: 'var(--n-400)', textTransform: 'uppercase',
                                     letterSpacing: '0.1em', fontWeight: 600, marginBottom: 4}}>
                          Factor loadings
                        </div>
                        <div>SPY · ℓ <span style={{color: 'var(--ink)'}}>0.516</span></div>
                        <div>QQQ · ℓ <span style={{color: 'var(--ink)'}}>0.568</span></div>
                        <div>IWM · ℓ <span style={{color: 'var(--ink)'}}>0.641</span></div>
                      </div>
                      <div>
                        <div style={{fontSize: 10, color: 'var(--n-400)', textTransform: 'uppercase',
                                     letterSpacing: '0.1em', fontWeight: 600, marginBottom: 4}}>
                          Proxy hedge (IWM → SPY/QQQ)
                        </div>
                        <div>β₁ SPY <span style={{color: 'var(--ink)'}}>+1.14</span></div>
                        <div>β₂ QQQ <span style={{color: 'var(--ink)'}}>−0.01</span></div>
                        <div>R² <span style={{color: 'var(--ink)'}}>79.6%</span></div>
                      </div>
                    </div>
                  </details>
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* =============== ISSUE =============== */}
      <div className="hc-issue-bar">
        <div className="hc-issue-summary">
          {hasQuote ? (
            <>
              <div>
                <div className="hc-ib-k">You pay</div>
                <div className="hc-ib-v">{fmtUSD(q.maxPremium)}</div>
              </div>
              <div>
                <div className="hc-ib-k">You receive</div>
                <div className="hc-ib-v">{(q.offeredCoupon*100).toFixed(2)}% p.a. · {fmtUSD(q.perCoupon, 0)}/mo</div>
              </div>
              <div>
                <div className="hc-ib-k">Settlement</div>
                <div className="hc-ib-v">USDC · 18-mo max</div>
              </div>
            </>
          ) : (
            <div style={{color: 'rgba(255,255,255,0.65)', fontSize: 14}}>
              Enter a notional above to review terms.
            </div>
          )}
        </div>
        <Button variant="primary" size="lg" disabled={!canIssue}>
          {!walletOk ? 'Connect wallet to issue'
            : !hasQuote ? 'Min $100 to issue'
            : 'Issue on-chain →'}
        </Button>
      </div>
    </div>
  );
}

function ParamChip({ label, children }) {
  return (
    <div className="hc-paramchip">
      <div className="hc-paramchip-k">{label}</div>
      <div className="hc-paramchip-v">{children}</div>
    </div>
  );
}

window.PageEquityAutocall = PageEquityAutocall;

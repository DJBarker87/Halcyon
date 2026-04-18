/* global React, PayoffChart */
// Halcyon — SOL Autocall issuance page
// PRODUCT CARD: fixed 16-day, 2-day cadence, lockout on obs 1, 70% KI, 102.5% AC.
// Buyer input = notional + slippage.

const { useState: useState_sol, useMemo: useMemo_sol, useEffect: useEffect_sol } = React;
const HM_sol = window.HalcyonMath;

// Hook: subscribe to the live EWMA45 σ and re-render on updates.
function useLiveSigma_sol(symbol) {
  const [sig, setSig] = useState_sol(() => {
    const s = window.HalcyonOracles && window.HalcyonOracles.getEwma(symbol);
    return s ? s.sigmaAnn : null;
  });
  const [meta, setMeta] = useState_sol(() => window.HalcyonOracles && window.HalcyonOracles.getEwma(symbol) || null);
  useEffect_sol(() => {
    if (!window.HalcyonOracles) return;
    return window.HalcyonOracles.subscribeEwma(symbol, payload => {
      setSig(payload.sigmaAnn);
      setMeta(payload);
    });
  }, [symbol]);
  // Also re-render when WASM finishes loading so the pricer swaps in
  const [, setTick] = useState_sol(0);
  useEffect_sol(() => {
    if (window.HalcyonMath && window.HalcyonMath.wasmReady) return;
    const h = () => setTick(t => t + 1);
    window.addEventListener('halcyon-wasm-ready', h);
    return () => window.removeEventListener('halcyon-wasm-ready', h);
  }, []);
  return { sigma: sig, meta };
}

const SOL = {
  tenorDays: 16,
  obsCadenceDays: 2,
  nObs: 8,
  autocallBarrier: 1.025,
  couponBarrier: 1.00,
  knockInBarrier: 0.70,
  lockoutObs: 1,
  floorBps: 50,                 // fair-coupon floor per obs
  quoteShare: 0.75,
  issuerMarginBps: 50,
  // NIG calibration
  alpha: 13.04,
  beta: 1.52,
  spot: 182.47,
};

function computeSolQuote({ notional, slippageBps, sigmaAnn }) {
  // Real POD-DEIM autocall pricer (halcyon-quote) when WASM is up.
  // Falls back to a sensible seed (175 bps, matches math stack at σ≈0.82)
  // if the WASM module or EWMA vol hasn't loaded yet.
  const wasm = window.HalcyonMath && window.HalcyonMath.wasm;
  const canPrice = !!wasm && !!wasm.sol_fair_coupon && Number.isFinite(sigmaAnn);
  const fairCouponPerObs = canPrice ? wasm.sol_fair_coupon(sigmaAnn) : 0.0175;
  const pricingEngine    = canPrice ? wasm.sol_pricing_engine(sigmaAnn) : 0;
  const pricingEngineName = ({ 2: 'POD-DEIM E11', 1: 'Richardson CTMC', 0: 'cached seed' })[pricingEngine];

  const quotedCouponPerObs  = fairCouponPerObs * SOL.quoteShare;
  const liveJitter          = Math.sin(notional / 900) * 0.00015;
  const offeredPerObs       = quotedCouponPerObs + liveJitter;

  const maxPremium   = notional * (1 + slippageBps / 10_000);
  const maxLiability = notional * (1 - SOL.knockInBarrier); // 30% of notional
  const issuanceFee  = notional * SOL.issuerMarginBps / 10_000;

  const perCoupon    = notional * offeredPerObs;
  const maxIncome    = perCoupon * SOL.nObs;
  const annualisedHeadline = offeredPerObs * (365 / SOL.obsCadenceDays);

  // Modelled knock-in probability from solmath-core's first-passage formula
  // (GBM r=0, i128 fixed-point), uses the same live σ feeding the pricer.
  const kiProbability = HM_sol.barrierHitProb({
    spot: 1.0,
    barrier: SOL.knockInBarrier,
    sigma: sigmaAnn || 0.82,
    tenorYears: SOL.tenorDays / 365,
    isUpper: false,
  });

  return {
    fairCouponPerObs, quotedCouponPerObs, offeredPerObs,
    annualisedHeadline, perCoupon, maxIncome,
    maxPremium, maxLiability, issuanceFee,
    kiProbability,
    sigmaAnn, canPrice, pricingEngineName,
  };
}

function PageSolAutocall({ tweaks }) {
  const [notional,    setNotional]    = useState_sol(5_000);
  const [slippageBps, setSlippageBps] = useState_sol(75);
  const [advOpen,     setAdvOpen]     = useState_sol(false);
  const [gateOpen,    setGateOpen]    = useState_sol(false);

  const { sigma: liveSigma, meta: sigmaMeta } = useLiveSigma_sol('SOL');
  const sigmaAnn = liveSigma != null ? liveSigma : 0.82; // seed until EWMA loads

  const hasQuote = notional >= 100;
  const q = useMemo_sol(
    () => hasQuote ? computeSolQuote({ notional, slippageBps, sigmaAnn }) : null,
    [notional, slippageBps, hasQuote, sigmaAnn]);

  const walletOk = tweaks.walletState === 'connected';
  const canIssue = hasQuote && walletOk;

  const payoffCurve = useMemo_sol(() =>
    HM_sol.solAutocallCurve({
      autocallBar: SOL.autocallBarrier,
      knockIn: SOL.knockInBarrier,
      coupon: (q ? q.offeredPerObs : 0.013) * SOL.nObs,
    }), [q]);

  const liveObsIdx = 3;

  return (
    <div className="hc-page">
      {/* =============== HEADER =============== */}
      <div className="hc-prodhead">
        <div className="hc-prodhead-main">
          <div className="hc-prodhead-eyebrow">
            Product 02 · SOL Autocall · native high-frequency structured note
          </div>
          <h1 className="hc-prodhead-title">
            16-day autocall on SOL · 8 observations, 2-day cadence
          </h1>
          <p className="hc-prodhead-sub">
            Short-dated structured note on native SOL. Autocall is suppressed on observation
            1 — every note runs at least 4 days. Hedged with spot SOL on-chain; no perps, no
            CEX, no bridge. Backtested over 5.6 years of SOL history: 94% of notes profitable
            for the buyer, vault positive in every calendar year.
          </p>

          <div className="hc-paramchips">
            <ParamChipS label="Underlying"><span className="pc-chip-inline">SOL</span></ParamChipS>
            <ParamChipS label="Tenor">16&nbsp;days</ParamChipS>
            <ParamChipS label="Observations">8 · every 2d</ParamChipS>
            <ParamChipS label="Autocall">≥&nbsp;102.5% · obs 2+</ParamChipS>
            <ParamChipS label="Knock-in">70% · discrete</ParamChipS>
            <ParamChipS label="Lockout">obs 1 · 4-day min</ParamChipS>
            <ParamChipS label="Hedge">Spot SOL · δ_obs_050</ParamChipS>
          </div>
        </div>
        <aside className="hc-prodhead-proof">
          <div className="hc-proofrow"><span className="k">Buyer mean return / note</span><span className="v">+1.65%</span></div>
          <div className="hc-proofrow"><span className="k">Profitable notes</span><span className="v">94%</span></div>
          <div className="hc-proofrow"><span className="k">Autocall rate</span><span className="v">68%</span></div>
          <div className="hc-proofrow"><span className="k">Avg note life</span><span className="v">9.3 d</span></div>
          <div className="hc-proofrow"><span className="k">Vault positive years</span><span className="v">7 / 7</span></div>
        </aside>
      </div>

      {/* =============== QUOTE =============== */}
      {/* =============== HERO INPUT =============== */}
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
              placeholder="5,000" inputMode="numeric" autoFocus />
            <span className="hc-hero-input-suffix">USDC</span>
          </div>
          <div className="hc-hero-input-presets">
            {[1000, 5000, 10000, 50000, 250000].map(v => (
              <button key={v}
                      className={notional === v ? 'active' : ''}
                      onClick={() => setNotional(v)}>
                ${v >= 1000 ? `${(v/1000).toFixed(0)}k` : v}
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
            <div className="hc-hero-input-sr"><span className="k">Coupon / obs</span><span className="v">{fmtUSD(q.perCoupon, 2)}</span></div>
            <div className="hc-hero-input-sr"><span className="k">Max coupon income</span><span className="v">{fmtUSD(q.maxIncome, 0)}</span></div>
            <div className="hc-hero-input-sr"><span className="k">Principal at risk below 70%</span><span className="v">{fmtUSD(q.maxLiability, 0)}</span></div>
            <div className="hc-hero-input-sr"><span className="k">Issuance fee</span><span className="v">{fmtUSD(q.issuanceFee, 2)}</span></div>
            <div className="hc-hero-input-sr"
                 title={`Fair coupon priced live by halcyon-quote ${q.pricingEngineName || 'cached seed'}. EWMA45(SOL) = ${(q.sigmaAnn*100).toFixed(1)}% annualized.`}>
              <span className="k">Fair coupon / obs<span style={{color: q.canPrice ? 'var(--blue-600)' : 'var(--n-400)', fontWeight: 700, marginLeft: 6}}>·{q.canPrice ? (q.pricingEngineName || 'live') : 'seed'}·</span></span>
              <span className="v">{(q.fairCouponPerObs*100).toFixed(3)}%</span>
            </div>
            <div className="hc-hero-input-sr"
                 title="Knock-in probability from solmath-core barrier_hit_probability (i128 fixed-point, first-passage GBM formula). Uses the same live σ feeding the pricer.">
              <span className="k">KI probability (σ={(q.sigmaAnn*100).toFixed(1)}%)<span style={{color: 'var(--blue-600)', fontWeight: 700, marginLeft: 6}}>·solmath·</span></span>
              <span className="v">{Number.isFinite(q.kiProbability) ? (q.kiProbability * 100).toFixed(2) + '%' : '—'}</span>
            </div>
          </div>
        )}
      </div>

      {/* =============== OFFER =============== */}
      {hasQuote && (
        <div className="hc-quotecard">
          <div className="hc-quotecard-offer">
            <div className="hc-qco-eyebrow">Offered coupon · per 2-day obs.</div>
            <div className="hc-qco-headline">
              {(q.offeredPerObs * 100).toFixed(2)}<span className="unit">% / obs</span>
            </div>
            <div className="hc-qco-sub">
              Pays <b>{fmtUSD(q.perCoupon, 2)}</b> on each of {SOL.nObs} observations when SOL closes
              ≥&nbsp;100% of entry. If SOL closes ≥&nbsp;102.5% from obs 2 onward, the note autocalls
              at par. Max coupon income <b>{fmtUSD(q.maxIncome, 0)}</b> · headline annualised
              {' '}<b>{(q.annualisedHeadline * 100).toFixed(0)}%</b> if the note never autocalled.
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
                <div className="hc-qco-sub2">to −30% at KI</div>
              </div>
              <div>
                <div className="hc-qco-k">Issuance fee</div>
                <div className="hc-qco-v">{fmtUSD(q.issuanceFee)}</div>
                <div className="hc-qco-sub2">50 bp · fixed</div>
              </div>
              <div>
                <div className="hc-qco-k">Quote slot</div>
                <div className="hc-qco-v mono">298,442,117</div>
                <div className="hc-qco-sub2">refreshes every block</div>
              </div>
            </div>

            <div className="hc-qco-benchmark">
              <b>Model benchmark.</b> The NIG-driven fair coupon on this slot is
              {' '}{(q.fairCouponPerObs*100).toFixed(2)}%/obs; the vault offers <b>75%</b>
              ({(q.quotedCouponPerObs*100).toFixed(2)}%/obs) and retains the 25% spread
              plus the 50bp issuance margin. Across 1,638 backtested notes the vault
              earned <b>+$4.79 per $1,000 note</b> — positive in every one of seven
              calendar years and in the 2022 SOL crash.
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
                <div className="hc-po-head">Autocall · redeemed early at par</div>
                <div className="hc-po-body">
                  If SOL closes ≥&nbsp;102.5% of entry on any observation from day 4 onward,
                  the note redeems with full principal plus every coupon earned. Backtested
                  frequency: <b>68% of notes</b>. <b>41%</b> exit on day 4 — the earliest
                  allowed; <b>34%</b> run the full 16 days.
                </div>
              </div>
            </div>

            <div className="hc-payoff-outcome">
              <div className="hc-po-dot" style={{background: 'var(--n-500)'}}/>
              <div>
                <div className="hc-po-head">Dead zone · principal back, partial coupons</div>
                <div className="hc-po-body">
                  SOL dips below 100% but stays above 70% for the whole 16 days. You get
                  principal back at maturity plus whatever coupons were paid before the dip.
                  <b> 26% of notes</b>.
                </div>
              </div>
            </div>

            <div className="hc-payoff-outcome">
              <div className="hc-po-dot" style={{background: 'var(--rust-500)'}}/>
              <div>
                <div className="hc-po-head">Knock-in · principal at risk</div>
                <div className="hc-po-body">
                  SOL closes below 70% of entry on an observation day and finishes below
                  entry at maturity. You take the terminal SOL performance 1-for-1, up to
                  <b> −{fmtUSD(q.maxLiability, 0)}</b>. Backtested frequency: <b>6% of
                  notes</b>. Worst single-note outcome in the backtest: −67.9%.
                </div>
              </div>
            </div>
          </div>

          <div className="hc-chartcard">
            <div className="hc-chartcard-head">
              <h3>Payoff at maturity · SOL terminal performance</h3>
              <div className="legend">
                <span><span className="sw" style={{background:'var(--blue-600)'}} /> Payoff</span>
                <span><span className="sw" style={{background:'var(--rust-500)'}} /> KI 70%</span>
                <span><span className="sw" style={{background:'var(--n-400)'}} /> AC 102.5%</span>
              </div>
            </div>
            <PayoffChart
              curves={[{ data: payoffCurve, color: 'var(--blue-600)' }]}
              annotations={[
                { x: SOL.knockInBarrier,   label: 'KI 70%',    color: 'var(--rust-500)' },
                { x: SOL.autocallBarrier,  label: 'AC 102.5%', color: 'var(--n-500)' },
              ]}
              width={620} height={260}
              xLabel="SOL terminal · S_T / S_0"
              xFormat={v => `${(v*100).toFixed(0)}%`}
              yFormat={v => `${(v*100).toFixed(0)}%`} />
          </div>
        </div>
      )}

      {/* =============== SCHEDULE =============== */}
      <div className="hc-section-title">
        <span>Observation schedule</span>
        <span className="meta">2-day cadence · lockout on obs 1 · discrete KI</span>
      </div>
      <div className="hc-schedule">
        <div className="hc-sched-axis">
          <div className="hc-sched-line" />
          {Array.from({length: SOL.nObs}, (_, i) => {
            const pct = (i / (SOL.nObs - 1)) * 100;
            const locked = i === 0;
            const live   = i === liveObsIdx;
            return (
              <div key={i}
                   className={`hc-sched-obs ${locked ? 'locked' : ''} ${live ? 'live' : ''}`}
                   style={{left: `${pct}%`}}>
                <span className="hc-sched-obs-day">T+{(i+1)*2}d</span>
                <span className="hc-sched-obs-label">
                  {locked ? 'lockout' : live ? 'live' : `obs ${i+1}`}
                </span>
              </div>
            );
          })}
        </div>
        <div className="hc-sched-legend">
          <span><span className="dot locked" /> lockout (no AC)</span>
          <span><span className="dot" /> future obs</span>
          <span><span className="dot live" /> current</span>
        </div>
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
                <FieldLabel meta="max premium movement you'll accept">Slippage tolerance</FieldLabel>
                <Slider value={slippageBps} onChange={setSlippageBps}
                        min={25} max={250} step={25} />
                <div style={{display:'flex', justifyContent: 'space-between',
                             fontFamily: 'var(--f-mono)', fontSize: 10,
                             color: 'var(--n-400)', letterSpacing: '0.04em',
                             marginTop: 6, fontVariantNumeric: 'tabular-nums'}}>
                  <span>25bp</span>
                  <span style={{color: 'var(--ink)', fontWeight: 600}}>
                    {slippageBps}bp · max premium {hasQuote ? fmtUSD(q.maxPremium) : '—'}
                  </span>
                  <span>250bp</span>
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
                  <div><b>Fair-coupon floor</b> — {(q?.fairCouponPerObs*100).toFixed(2)}%/obs, floor 50 bps/obs.</div>
                  <div><b>Vault utilisation</b> — 58%, cap 90%.</div>
                  <div><b>Coupon-alive ratio</b> — 42%, cap 50% of active notes.</div>
                  <div><b>Pyth SOL/USD</b> — 1s stale, cap 30s.</div>
                  <div><b>Hedge sleeve</b> — 3,420 SOL earmarked on delta_obs_050.</div>
                  <details style={{marginTop: 10, paddingTop: 10, borderTop: '1px solid var(--n-100)'}}>
                    <summary style={{cursor: 'pointer', fontSize: 10, fontFamily: 'var(--f-mono)',
                                     color: 'var(--n-500)', letterSpacing: '0.1em',
                                     textTransform: 'uppercase', fontWeight: 600, listStyle: 'none'}}>
                      ▸ NIG engine internals
                    </summary>
                    <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14,
                                 marginTop: 10, fontFamily: 'var(--f-mono)', fontSize: 11,
                                 color: 'var(--n-500)', fontVariantNumeric: 'tabular-nums'}}>
                      <div>
                        <div style={{fontSize: 10, color: 'var(--n-400)', textTransform: 'uppercase',
                                     letterSpacing: '0.1em', fontWeight: 600, marginBottom: 4}}>
                          NIG calibration
                        </div>
                        <div>α <span style={{color: 'var(--ink)'}}>{SOL.alpha.toFixed(2)}</span></div>
                        <div>β <span style={{color: 'var(--ink)'}}>+{SOL.beta.toFixed(2)}</span></div>
                        <div>σ source <span style={{color: 'var(--ink)'}}>EWMA-45</span></div>
                      </div>
                      <div>
                        <div style={{fontSize: 10, color: 'var(--n-400)', textTransform: 'uppercase',
                                     letterSpacing: '0.1em', fontWeight: 600, marginBottom: 4}}>
                          Lockout repricing
                        </div>
                        <div>engine <span style={{color: 'var(--ink)'}}>gated Richardson</span></div>
                        <div>N₁ / N₂ <span style={{color: 'var(--ink)'}}>10 / 15</span></div>
                        <div>Δ fair <span style={{color: 'var(--ink)'}}>−6.8%</span></div>
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
                <div className="hc-ib-k">You receive / obs</div>
                <div className="hc-ib-v">{(q.offeredPerObs*100).toFixed(2)}% · {fmtUSD(q.perCoupon, 2)}</div>
              </div>
              <div>
                <div className="hc-ib-k">Settlement</div>
                <div className="hc-ib-v">USDC · 16-d max</div>
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

function ParamChipS({ label, children }) {
  return (
    <div className="hc-paramchip">
      <div className="hc-paramchip-k">{label}</div>
      <div className="hc-paramchip-v">{children}</div>
    </div>
  );
}

window.PageSolAutocall = PageSolAutocall;

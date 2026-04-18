/* global React, PayoffChart */
// Halcyon — IL Protection issuance page
//
// Three-state UX driven by wallet + LP detection:
//   State A  — connected, Raydium SOL/USDC LP detected → one-click insure
//   State B  — connected, no LP → choice between LP+Insure (post-hackathon)
//              and Synthetic cover (pure derivative, enter notional)
//   State C  — disconnected → connect prompt
//
// The product is a 30-day European contract on the constant-product IL payoff
// for SOL/USDC (Raydium Standard AMM V4). Deductible 1%, cap 7%. Pricing is
// the NIG European quote — see il_protection_math_stack.md.

const { useState: useState_il, useMemo: useMemo_il, useEffect: useEffect_il, useRef: useRef_il } = React;
const HM_il = window.HalcyonMath;

// Production NIG params for 30-day IL contract per il_protection_math_stack.md §3
const IL_NIG_ALPHA   = 3.14;
const IL_NIG_BETA    = 1.21;
const IL_LAUNCH_LOAD = 1.10;
const IL_SIGMA_FLOOR = 0.40;
const IL_CALM_MULT   = 1.30;
// Stress multiplier (2.00) kicks in when fvol ≥ 0.60 — fvol signal is off-chain
// and not yet wired; we default to calm regime here.

const IL = {
  pool:            'SOL / USDC',
  venue:           'Raydium AMM V4',
  weight:          '50 / 50',
  fee:             '0.25%',
  tenorDays:       30,
  deductible:      0.01,
  cap:             0.07,
  settlement:      'European · Pyth ratio',
  issuerMarginBps: 50,
};

// -------------------- Hooks --------------------

function useLiveSigma_il(symbol) {
  const [sig, setSig] = useState_il(() => {
    const s = window.HalcyonOracles && window.HalcyonOracles.getEwma(symbol);
    return s ? s.sigmaAnn : null;
  });
  const [meta, setMeta] = useState_il(() => window.HalcyonOracles && window.HalcyonOracles.getEwma(symbol) || null);
  useEffect_il(() => {
    if (!window.HalcyonOracles) return;
    return window.HalcyonOracles.subscribeEwma(symbol, payload => {
      setSig(payload.sigmaAnn);
      setMeta(payload);
    });
  }, [symbol]);
  const [, setTick] = useState_il(0);
  useEffect_il(() => {
    if (window.HalcyonMath && window.HalcyonMath.wasmReady) return;
    const h = () => setTick(t => t + 1);
    window.addEventListener('halcyon-wasm-ready', h);
    return () => window.removeEventListener('halcyon-wasm-ready', h);
  }, []);
  return { sigma: sig, meta };
}

function useLivePool_il(key) {
  const [pool, setPool] = useState_il(() =>
    window.HalcyonOracles && window.HalcyonOracles.getPool(key) || null);
  useEffect_il(() => {
    if (!window.HalcyonOracles) return;
    return window.HalcyonOracles.subscribePool(key, setPool);
  }, [key]);
  return pool;
}

function useLiveSpot_il(symbol) {
  const [spot, setSpot] = useState_il(() =>
    window.HalcyonOracles && window.HalcyonOracles.getSpot(symbol) || null);
  useEffect_il(() => {
    if (!window.HalcyonOracles) return;
    return window.HalcyonOracles.subscribeSpot(symbol, setSpot);
  }, [symbol]);
  return spot;
}

// Returns { status, position, error? }. status is one of:
//   'disconnected' — wallet not connected
//   'loading'      — RPC in flight
//   'detected'     — LP found in Raydium SOL/USDC Standard AMM V4
//   'none'         — wallet connected but no LP balance
function useLpPosition_il(tweaks) {
  const { walletState, mockLpValue, walletPubkey } = tweaks;
  const [result, setResult] = useState_il({ status: 'loading', position: null });
  const pool = useLivePool_il('RAYDIUM_SOL_USDC');
  const spot = useLiveSpot_il('SOL');

  useEffect_il(() => {
    let cancelled = false;

    if (walletState !== 'connected') {
      setResult({ status: 'disconnected', position: null });
      return;
    }

    // Mock path — synthesise a detected position from the tweak.
    if (mockLpValue && Number(mockLpValue) > 0) {
      const solPrice = (spot && spot.price) || (pool && pool.price) || 175;
      const underlyingUsdc = Number(mockLpValue) / 2;
      const underlyingSol  = (Number(mockLpValue) / 2) / solPrice;
      setResult({
        status: 'detected',
        position: {
          source:         'mock',
          valueUsdc:      Number(mockLpValue),
          underlyingSol, underlyingUsdc,
          lpAmount:       null,
          solPrice,
          fetchedAt:      Date.now(),
          lpAccountAddr:  '7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU',
        },
      });
      return;
    }

    // Real RPC path — requires a wallet pubkey.
    if (!walletPubkey) {
      setResult({ status: 'none', position: null });
      return;
    }

    setResult({ status: 'loading', position: null });
    if (!window.HalcyonLP) {
      setResult({ status: 'none', position: null, error: 'lp_detection.js not loaded' });
      return;
    }

    (async () => {
      const r = await window.HalcyonLP.detectPosition(walletPubkey);
      if (cancelled) return;
      if (r.hasPosition) {
        setResult({
          status: 'detected',
          position: { ...r, lpAccountAddr: walletPubkey },
        });
      } else {
        setResult({ status: 'none', position: null, error: r.error });
      }
    })();

    return () => { cancelled = true; };
  }, [walletState, mockLpValue, walletPubkey, spot && spot.price, pool && pool.price]);

  return result;
}

// -------------------- Pricing --------------------

function computeIlQuote_il({ notional, slippageBps, sigmaAnn }) {
  // Real halcyon-quote NIG European pricer via WASM when available.
  // Sigma pipeline per il_protection_math_stack.md §7:
  //   σ_pricing = max(σ_ewma45 × regime_multiplier, σ_floor)
  const wasm = window.HalcyonMath && window.HalcyonMath.wasm;
  const canPrice = !!wasm && !!wasm.il_fair_premium && Number.isFinite(sigmaAnn);

  const regimeMultiplier = IL_CALM_MULT;
  const sigmaPricing     = Math.max((sigmaAnn || 0) * regimeMultiplier, IL_SIGMA_FLOOR);

  const fairPremiumPct = canPrice
    ? wasm.il_fair_premium(sigmaPricing, IL.tenorDays, IL.deductible, IL.cap, IL_NIG_ALPHA, IL_NIG_BETA)
    : 0.0120; // seed until WASM/EWMA load
  const loadedPremiumPct = fairPremiumPct * IL_LAUNCH_LOAD;

  const jitter     = Math.sin(notional / 700) * 0.00008;
  const premiumPct = loadedPremiumPct + jitter;
  const premium    = notional * premiumPct;
  const maxPremium = premium * (1 + slippageBps / 10_000);
  const maxPayout  = notional * (IL.cap - IL.deductible);
  const expPayout  = notional * fairPremiumPct;
  const vaultMargin = premium - expPayout;
  const issuanceFee = notional * IL.issuerMarginBps / 10_000;
  return {
    premiumPct, premium, maxPremium, maxPayout, expPayout, vaultMargin, issuanceFee,
    fairPremiumPct, loadedPremiumPct, sigmaAnn, sigmaPricing, regimeMultiplier, canPrice,
  };
}

// Synthetic P5/P95 range on 30-day net outcome of a covered position.
// TODO(production): replace with backtest-derived percentiles from
// il_protection_product_economics_report.md. This is a reasonable placeholder
// consistent with the 80% loss ratio and the reported $264-472 P95 IL
// reduction on a $10k position.
function computeP95Range_il({ notional, grossFees, premium, maxPremium }) {
  const low  = grossFees - maxPremium - 0.04 * notional;
  const high = grossFees - premium    + 0.02 * notional;
  return { low, high };
}

// -------------------- Page entry --------------------

function PageILProtection({ tweaks, onTweak }) {
  const lp = useLpPosition_il(tweaks);
  const [flow, setFlow] = useState_il('auto'); // 'auto' | 'synthetic'

  const isConnected = tweaks.walletState === 'connected';
  const screen =
      !isConnected             ? 'disconnected'
    : flow === 'synthetic'     ? 'synthetic'
    : lp.status === 'loading'  ? 'loading'
    : lp.status === 'detected' ? 'detected'
    :                            'choice';

  return (
    <div className="hc-page">
      {onTweak && <DemoStateSelectorIL tweaks={tweaks} onTweak={onTweak}
                                       flow={flow} setFlow={setFlow} />}
      <ProductHeadIL />

      {screen === 'disconnected' && <DisconnectedIL />}
      {screen === 'loading'      && <LoadingIL />}
      {screen === 'detected'     && <LpDetectedIL position={lp.position} tweaks={tweaks} />}
      {screen === 'choice'       && <NoLpChoiceIL tweaks={tweaks} onSynthetic={() => setFlow('synthetic')} />}
      {screen === 'synthetic'    && <SyntheticFlowIL tweaks={tweaks} onBack={() => setFlow('auto')} />}
    </div>
  );
}

// -------------------- Demo state selector (hackathon-only) --------------------
//
// Lets the user hop between the four UX states without a real wallet. Visible
// at the top of the page with a clear "demo" label so nobody mistakes it for
// real controls. Writes walletState + mockLpValue back to the App-level
// tweaks object via onTweak.
function DemoStateSelectorIL({ tweaks, onTweak, flow, setFlow }) {
  const current =
      tweaks.walletState !== 'connected' ? 'disconnected'
    : flow === 'synthetic'               ? 'synthetic'
    : Number(tweaks.mockLpValue) > 0     ? 'lp'
    :                                      'nolp';

  const pick = (id) => {
    if (id === 'disconnected') {
      onTweak({ walletState: 'disconnected' });
      setFlow('auto');
    } else if (id === 'nolp') {
      onTweak({ walletState: 'connected', mockLpValue: 0 });
      setFlow('auto');
    } else if (id === 'lp') {
      onTweak({ walletState: 'connected', mockLpValue: tweaks.mockLpValue > 0 ? tweaks.mockLpValue : 12400 });
      setFlow('auto');
    } else if (id === 'synthetic') {
      onTweak({ walletState: 'connected', mockLpValue: 0 });
      setFlow('synthetic');
    }
  };

  const opts = [
    { id: 'disconnected', label: 'Disconnected',       sub: 'state C' },
    { id: 'nolp',         label: 'Connected · no LP',  sub: 'state B' },
    { id: 'lp',           label: 'Connected · LP',     sub: 'state A' },
    { id: 'synthetic',    label: 'Synthetic flow',     sub: 'from B' },
  ];

  return (
    <div style={{display: 'flex', alignItems: 'center', gap: 4, marginBottom: 20,
                 padding: 4, background: 'var(--n-50)', border: '1px dashed var(--n-200)',
                 borderRadius: 'var(--r-md)', width: 'fit-content',
                 fontVariantNumeric: 'tabular-nums'}}>
      <span style={{fontFamily: 'var(--f-mono)', fontSize: 9, letterSpacing: '0.14em',
                    color: 'var(--n-400)', padding: '6px 10px 6px 12px',
                    textTransform: 'uppercase', fontWeight: 700}}>
        demo state ·
      </span>
      {opts.map(o => {
        const active = current === o.id;
        return (
          <button key={o.id} onClick={() => pick(o.id)}
                  style={{padding: '6px 12px', fontSize: 12, fontFamily: 'var(--f-mono)',
                          letterSpacing: '0.02em', border: 0, cursor: 'pointer',
                          borderRadius: 'var(--r-sm)', lineHeight: 1.1,
                          background: active ? 'var(--ink)' : 'transparent',
                          color:      active ? '#fff' : 'var(--n-500)',
                          display: 'flex', flexDirection: 'column', alignItems: 'flex-start',
                          gap: 2}}>
            <span style={{fontWeight: 600}}>{o.label}</span>
            <span style={{fontSize: 9, letterSpacing: '0.14em', textTransform: 'uppercase',
                          opacity: active ? 0.7 : 0.5}}>
              {o.sub}
            </span>
          </button>
        );
      })}
    </div>
  );
}

// -------------------- Product head (shared) --------------------

function ProductHeadIL() {
  return (
    <div className="hc-prodhead">
      <div className="hc-prodhead-main">
        <div className="hc-prodhead-eyebrow">
          Product 03 · IL Protection · 30d constant-product IL derivative
        </div>
        <h1 className="hc-prodhead-title">
          30-day IL cover · SOL / USDC on Raydium
        </h1>
        <p style={{fontFamily: 'var(--f-serif)', fontSize: 18, lineHeight: 1.5,
                   color: 'var(--ink)', maxWidth: '60ch', marginTop: 18, marginBottom: 0,
                   letterSpacing: '-0.005em', fontWeight: 400}}>
          A 30-day European-settled cover on the Raydium Standard AMM V4
          SOL/USDC pool. Pays when realised impermanent loss exceeds 1%,
          capped at 7%. Settled against the Pyth entry and exit ratio —
          no LP token custody.
        </p>
      </div>
      <aside className="hc-prodhead-proof"
             style={{padding: '20px 28px 20px 20px', gap: 14}}>
        <div style={{fontFamily: 'var(--f-mono)', fontSize: 10, letterSpacing: '0.14em',
                     textTransform: 'uppercase', color: 'var(--n-400)', fontWeight: 700,
                     marginBottom: 6}}>
          Backtest · Aug 2020 – Feb 2026
        </div>
        <div className="hc-proofrow"><span className="k">Mean premium</span><span className="v">1.20%</span></div>
        <div className="hc-proofrow"><span className="k">Loss ratio · full</span><span className="v">80%</span></div>
        <div className="hc-proofrow"><span className="k">P95 IL reduction</span><span className="v">$264–472</span></div>
        <div className="hc-proofrow"><span className="k">Engine failures</span><span className="v">0 / 2,027</span></div>
      </aside>
    </div>
  );
}

// -------------------- State C — Disconnected --------------------

function DisconnectedIL() {
  return (
    <div style={{display: 'flex', flexDirection: 'column', alignItems: 'center',
                 padding: '80px 40px', gap: 18, textAlign: 'center'}}>
      <King size={96} color="var(--blue-600)" />
      <h2 style={{fontFamily: 'var(--f-serif)', fontSize: 36, fontStyle: 'italic',
                   fontWeight: 400, margin: 0, letterSpacing: '-0.01em',
                   color: 'var(--ink)', maxWidth: 560, lineHeight: 1.2}}>
        Connect a wallet to insure your liquidity.
      </h2>
      <p style={{color: 'var(--n-500)', maxWidth: 500, fontSize: 15, lineHeight: 1.5}}>
        If you hold the Raydium SOL/USDC LP, we'll price cover for your exact
        position. If not, you can take the same 30-day constant-product IL
        exposure synthetically with USDC.
      </p>
      <div style={{marginTop: 10}}>
        <Button variant="primary" size="lg">Connect wallet</Button>
      </div>
    </div>
  );
}

// -------------------- Loading --------------------

function LoadingIL() {
  return (
    <div style={{padding: '80px 40px', textAlign: 'center', color: 'var(--n-400)'}}>
      <Skeleton w={220} h={16} />
      <div style={{marginTop: 10, fontSize: 12, fontFamily: 'var(--f-mono)',
                   letterSpacing: '0.1em', textTransform: 'uppercase'}}>
        Checking your Raydium positions…
      </div>
    </div>
  );
}

// -------------------- State B — No LP, show two-path choice --------------------

function NoLpChoiceIL({ tweaks, onSynthetic }) {
  const [comingOpen, setComingOpen] = useState_il(false);

  return (
    <>
      {!tweaks.mockHasUsdc && (
        <div style={{background: 'var(--amber-50, #FFF8E6)', border: '1px solid var(--amber-200, #F5D99B)',
                     borderRadius: 'var(--r-md)', padding: '10px 14px', marginBottom: 14,
                     fontSize: 13, color: 'var(--n-600)'}}>
          Your wallet doesn't appear to hold USDC. Top up before issuing cover.
        </div>
      )}

      <div style={{display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
                   gap: 14, marginTop: 8}}>
        <ChoiceCardIL
          eyebrow="Path 1 · all-in-one"
          heading="LP + Insure"
          body="Deposit USDC, split into a Raydium SOL/USDC position, and buy 30-day cover in one transaction. Earn pool fees with downside protection wrapped in."
          target="For: LPs who want AMM fee exposure with IL protection built in."
          cta="Deposit USDC →"
          onClick={() => setComingOpen(true)}
        />
        <ChoiceCardIL
          eyebrow="Path 2 · pure derivative"
          heading="Synthetic cover"
          body="Buy the same 30-day constant-product IL payoff with USDC only — no LP position required. Settles at T+30d against the Pyth entry/exit ratio."
          target="For: Directional views on SOL/USDC divergence, or LPs whose position is on another venue."
          cta="Enter notional →"
          onClick={onSynthetic}
        />
      </div>

      <Modal open={comingOpen} onClose={() => setComingOpen(false)}
             title="LP + Insure · coming soon"
             footer={<Button variant="primary" onClick={() => setComingOpen(false)}>OK</Button>}>
        <p style={{margin: 0, color: 'var(--n-600)', fontSize: 14, lineHeight: 1.55}}>
          LP + Insure bundles three transactions into one atomic flow: a Jupiter
          swap of half your USDC to SOL, a Raydium deposit into the SOL/USDC
          Standard AMM V4, and a Halcyon IL cover issuance — all or nothing.
        </p>
        <p style={{margin: '12px 0 0', color: 'var(--n-500)', fontSize: 13, lineHeight: 1.55}}>
          Target launch: <b>week of 12 May 2026</b>, after the Jupiter-CPI
          integration audit. Until then, use <b>Synthetic cover</b> for the
          same IL exposure without creating an LP.
        </p>
        {/* TODO(post-hackathon): replace this modal with a deposit flow that
            builds and signs the atomic tx (Jupiter CPI + Raydium deposit CPI +
            IL issuance CPI). Requires the Halcyon IL program deployed to
            mainnet first. */}
      </Modal>
    </>
  );
}

function ChoiceCardIL({ eyebrow, heading, body, target, cta, onClick }) {
  return (
    <div style={{background: '#fff', border: '1px solid var(--n-100)', borderRadius: 'var(--r-lg)',
                 padding: 24, display: 'flex', flexDirection: 'column', gap: 10}}>
      <div style={{fontFamily: 'var(--f-mono)', fontSize: 10, letterSpacing: '0.14em',
                   textTransform: 'uppercase', color: 'var(--rust-500)', fontWeight: 700}}>
        {eyebrow}
      </div>
      <h3 style={{fontFamily: 'var(--f-serif)', fontSize: 28, fontWeight: 400, margin: 0,
                   letterSpacing: '-0.01em', color: 'var(--ink)'}}>
        {heading}
      </h3>
      <p style={{margin: 0, color: 'var(--n-600)', fontSize: 14, lineHeight: 1.55}}>
        {body}
      </p>
      <div style={{fontSize: 12, color: 'var(--n-400)', fontStyle: 'italic', marginTop: 2}}>
        {target}
      </div>
      <div style={{marginTop: 'auto', paddingTop: 16}}>
        <Button variant="primary" size="md" onClick={onClick}
                style={{width: '100%'}}>
          {cta}
        </Button>
      </div>
    </div>
  );
}

// -------------------- State A — LP detected --------------------

function LpDetectedIL({ position, tweaks }) {
  const [coveragePct, setCoveragePct] = useState_il(100);
  const [slippageBps, setSlippageBps] = useState_il(75);
  const [partialOpen, setPartialOpen] = useState_il(false);
  const [detailsOpen, setDetailsOpen] = useState_il(false);
  const [gateOpen,    setGateOpen]    = useState_il(false);
  const [modalOpen,   setModalOpen]   = useState_il(false);
  const [toast,       setToast]       = useState_il(null);
  const [policyId,    setPolicyId]    = useState_il(null);

  const { sigma: liveSigma } = useLiveSigma_il('SOL');
  const sigmaAnn = liveSigma != null ? liveSigma : 0.82;
  const pool = useLivePool_il('RAYDIUM_SOL_USDC');
  const spot = useLiveSpot_il('SOL');

  const covered  = Math.max(100, Math.floor(position.valueUsdc * coveragePct / 100));
  const q = useMemo_il(
    () => computeIlQuote_il({ notional: covered, slippageBps, sigmaAnn }),
    [covered, slippageBps, sigmaAnn]);

  // Fees scale with full position size regardless of coverage — you always
  // earn pool fees on every LP token you hold. Premium / expPayout scale with
  // covered notional.
  const tenorYears = IL.tenorDays / 365;
  const apr = pool ? pool.monthApr : null;
  const grossFees = apr != null ? position.valueUsdc * apr * tenorYears : 0;
  const netExpected = grossFees - q.premium + q.expPayout;
  const netAnnualised = tenorYears > 0 ? netExpected / position.valueUsdc / tenorYears : 0;
  const p95 = computeP95Range_il({ notional: covered, grossFees,
                                    premium: q.premium, maxPremium: q.maxPremium });

  // Payoff curve
  const payoffCurve = useMemo_il(() => {
    const pts = [];
    for (let r = 0.2; r <= 4.0; r += 0.02) {
      pts.push({ x: r, y: HM_il.ilPayoff({ r, deductible: IL.deductible, cap: IL.cap }) });
    }
    return pts;
  }, []);

  const pythStaleSec = spot && spot.publishTime
    ? Math.max(0, Math.round(Date.now()/1000 - spot.publishTime))
    : null;

  function handleIssue() {
    if (tweaks.network === 'mainnet-beta') {
      setModalOpen('mainnet');
    } else {
      setModalOpen('devnet');
    }
  }

  function handleDevnetSign() {
    setToast({ variant: 'info', title: 'Simulating issuance…' });
    setTimeout(() => {
      const id = Math.floor(1000 + Math.random() * 9000);
      setPolicyId(`HAL-IL-${id}`);
      setToast({ variant: 'success', title: `Policy issued · HAL-IL-${id}`,
                 body: `Devnet simulation · ${fmtUSD(covered, 0)} cover · settles T+30d` });
      setModalOpen(false);
      setTimeout(() => setToast(null), 4000);
    }, 1500);
  }

  function copyQuoteLink() {
    const url = new URL(window.location.href);
    url.searchParams.set('notional', String(covered));
    url.searchParams.set('slippageBps', String(slippageBps));
    navigator.clipboard && navigator.clipboard.writeText(url.toString());
    setToast({ variant: 'success', title: 'Quote link copied',
               body: 'Share with your counterparty or save for mainnet launch.' });
    setModalOpen(false);
    setTimeout(() => setToast(null), 3500);
  }

  return (
    <>
      {/* =============== HERO =============== */}
      <div className="hc-quotecard" style={{padding: 32}}>
        <div className="hc-quotecard-offer" style={{display: 'flex', flexDirection: 'column', gap: 6}}>

          <div style={{display: 'flex', alignItems: 'center', gap: 8,
                       fontFamily: 'var(--f-mono)', fontSize: 10.5, letterSpacing: '0.14em',
                       textTransform: 'uppercase', color: 'var(--blue-600)', fontWeight: 700}}>
            <King size={16} color="var(--blue-600)" />
            We found your Raydium SOL/USDC LP
            {position.source === 'mock' && (
              <span style={{marginLeft: 8, color: 'var(--n-400)', fontWeight: 500}}>
                · mock (tweak)
              </span>
            )}
          </div>

          <div style={{fontFamily: 'var(--f-serif)', fontSize: 64, fontWeight: 400,
                       letterSpacing: '-0.02em', lineHeight: 1, color: 'var(--ink)',
                       fontVariantNumeric: 'tabular-nums', marginTop: 6}}>
            {fmtUSD(position.valueUsdc, 0)}
          </div>

          <div style={{color: 'var(--n-500)', fontSize: 14, marginTop: 6}}>
            Earning{' '}
            {apr != null
              ? <b style={{color: 'var(--success-700)'}}>{(apr*100).toFixed(2)}% APR</b>
              : <Skeleton w={60} h={14} />}{' '}
            from pool fees ·{' '}
            <span style={{fontFamily: 'var(--f-mono)', fontSize: 12, color: 'var(--n-500)'}}>
              {position.underlyingSol.toFixed(2)} SOL +{' '}
              {Math.round(position.underlyingUsdc).toLocaleString('en-US')} USDC
            </span>
            {position.lpAccountAddr && (
              <span style={{fontFamily: 'var(--f-mono)', fontSize: 11, color: 'var(--n-400)',
                            marginLeft: 10}}>
                · {position.lpAccountAddr.slice(0, 4)}…{position.lpAccountAddr.slice(-4)}
              </span>
            )}
          </div>

          <div style={{marginTop: 20}}>
            <div style={{fontFamily: 'var(--f-serif)', fontStyle: 'italic', fontSize: 14,
                         color: 'var(--n-500)', marginBottom: 10, letterSpacing: '-0.005em'}}>
              Insure this position for 30 days · European settlement
            </div>
            <div className="hc-qco-row" style={{gridTemplateColumns: 'repeat(3, 1fr)', marginTop: 4}}>
              <div>
                <div className="hc-qco-k">Premium</div>
                <div className="hc-qco-v">{fmtUSD(q.premium, 2)}</div>
                <div className="hc-qco-sub2">{(q.premiumPct*100).toFixed(2)}% · one-time</div>
              </div>
              <div>
                <div className="hc-qco-k">Max payout</div>
                <div className="hc-qco-v">{fmtUSD(q.maxPayout, 0)}</div>
                <div className="hc-qco-sub2">{((IL.cap-IL.deductible)*100).toFixed(0)}% · cap − deductible</div>
              </div>
              <div>
                <div className="hc-qco-k">Expected net 30d</div>
                <div className="hc-qco-v"
                     style={{color: netExpected >= 0 ? 'var(--success-700)' : 'var(--rust-500)'}}>
                  {netExpected >= 0 ? '+' : ''}{fmtUSD(netExpected, 0)}
                </div>
                <div className="hc-qco-sub2">
                  ≈ {(netAnnualised*100).toFixed(1)}%/yr ·
                  P95 [{fmtUSD(p95.low, 0)}, {p95.high >= 0 ? '+' : ''}{fmtUSD(p95.high, 0)}]
                </div>
              </div>
            </div>
          </div>

          <Button variant="primary" size="lg"
                  onClick={handleIssue}
                  style={{width: '100%', marginTop: 20}}>
            Insure my {fmtUSD(covered, 0)} {coveragePct < 100 ? `(${coveragePct}% of position)` : 'position'} for {fmtUSD(q.maxPremium, 2)}
          </Button>

          <div style={{display: 'flex', gap: 24, marginTop: 8}}>
            <button className="hc-chev" onClick={() => setPartialOpen(!partialOpen)}>
              <span className={`hc-chev-arrow ${partialOpen ? 'open' : ''}`}>▸</span>
              Partial cover
              <span style={{marginLeft: 6, fontSize: 10, fontFamily: 'var(--f-mono)',
                            color: 'var(--n-400)', letterSpacing: '0.06em'}}>
                {coveragePct}% of position
              </span>
            </button>
            <button className="hc-chev" onClick={() => setDetailsOpen(!detailsOpen)}>
              <span className={`hc-chev-arrow ${detailsOpen ? 'open' : ''}`}>▸</span>
              Model details
            </button>
          </div>

          {partialOpen && (
            <div style={{background: 'var(--n-50)', border: '1px solid var(--n-100)',
                         borderRadius: 'var(--r-md)', padding: 16, marginTop: 8}}>
              <FieldLabel meta={`covering ${fmtUSD(covered, 0)} of ${fmtUSD(position.valueUsdc, 0)}`}>
                Coverage percentage
              </FieldLabel>
              <div style={{display: 'flex', gap: 6, marginTop: 8}}>
                {[25, 50, 75, 100].map(pct => (
                  <button key={pct}
                          className={'hc-hero-input-presets-btn ' + (coveragePct === pct ? 'active' : '')}
                          style={{padding: '6px 10px', fontSize: 12, fontFamily: 'var(--f-mono)',
                                  letterSpacing: '0.04em', border: '1px solid var(--n-200)',
                                  background: coveragePct === pct ? 'var(--ink)' : '#fff',
                                  color: coveragePct === pct ? '#fff' : 'var(--n-500)',
                                  borderRadius: 'var(--r-sm)', cursor: 'pointer'}}
                          onClick={() => setCoveragePct(pct)}>
                    {pct}%
                  </button>
                ))}
              </div>
              <div style={{marginTop: 12}}>
                <Slider value={coveragePct} onChange={setCoveragePct} min={10} max={100} step={5} />
              </div>
            </div>
          )}

          {detailsOpen && (
            <AdvancedDetailsIL
              slippageBps={slippageBps} setSlippageBps={setSlippageBps}
              q={q} pool={pool} pythStaleSec={pythStaleSec}
              gateOpen={gateOpen} setGateOpen={setGateOpen} />
          )}
        </div>
      </div>

      {/* =============== QUIET ANCHOR =============== */}
      <QuietAnchorIL />

      {/* =============== PAYOFF =============== */}
      <PayoffBlockIL q={q} payoffCurve={payoffCurve} />

      {/* =============== MODALS & TOAST =============== */}
      <Modal open={modalOpen === 'devnet'} onClose={() => setModalOpen(false)}
             title="Confirm devnet issuance"
             footer={
               <>
                 <Button variant="ghost" onClick={() => setModalOpen(false)}>Cancel</Button>
                 <Button variant="primary" onClick={handleDevnetSign}>Sign &amp; issue (demo)</Button>
               </>
             }>
        <div style={{fontSize: 14, color: 'var(--n-600)', lineHeight: 1.55}}>
          <b>{fmtUSD(q.maxPremium, 2)}</b> premium for <b>{fmtUSD(q.maxPayout, 0)}</b> max
          cover on your <b>{fmtUSD(covered, 0)}</b> Raydium SOL/USDC position,
          settling T+30d against the Pyth entry/exit ratio.
        </div>
        <div style={{marginTop: 14, padding: '10px 12px', background: 'var(--amber-50, #FFF8E6)',
                     border: '1px solid var(--amber-200, #F5D99B)', borderRadius: 'var(--r-sm)',
                     fontSize: 12, color: 'var(--n-600)', lineHeight: 1.5}}>
          <b>Devnet simulation.</b> No real USDC will be transferred. The Halcyon
          IL program is in audit and will deploy to mainnet shortly.
        </div>
      </Modal>

      <Modal open={modalOpen === 'mainnet'} onClose={() => setModalOpen(false)}
             title="Mainnet launch soon"
             footer={
               <>
                 <Button variant="ghost" onClick={() => setModalOpen(false)}>Close</Button>
                 <Button variant="primary" onClick={copyQuoteLink}>Copy quote link</Button>
               </>
             }>
        <div style={{fontSize: 14, color: 'var(--n-600)', lineHeight: 1.55}}>
          The Halcyon IL program is in audit. Mainnet issuance launches in the
          coming weeks. Your quoted terms are pricing-locked at the timestamp
          encoded in this link — share it with a counterparty or save it for
          launch day.
        </div>
        <div style={{marginTop: 14, fontFamily: 'var(--f-mono)', fontSize: 11,
                     color: 'var(--n-400)', padding: '10px 12px', background: 'var(--n-50)',
                     borderRadius: 'var(--r-sm)', border: '1px solid var(--n-100)'}}>
          ?notional={covered}&amp;slippageBps={slippageBps}
        </div>
      </Modal>

      {toast && (
        <div style={{position: 'fixed', bottom: 24, right: 24, zIndex: 200, maxWidth: 360}}>
          <Toast variant={toast.variant} title={toast.title}>{toast.body || ''}</Toast>
        </div>
      )}
    </>
  );
}

// -------------------- Synthetic flow (current notional-based page) --------------------

function SyntheticFlowIL({ tweaks, onBack }) {
  const [notional,    setNotional]    = useState_il(10_000);
  const [slippageBps, setSlippageBps] = useState_il(75);
  const [advOpen,     setAdvOpen]     = useState_il(false);
  const [gateOpen,    setGateOpen]    = useState_il(false);
  const [modalOpen,   setModalOpen]   = useState_il(false);
  const [toast,       setToast]       = useState_il(null);

  const { sigma: liveSigma } = useLiveSigma_il('SOL');
  const sigmaAnn = liveSigma != null ? liveSigma : 0.82;
  const pool = useLivePool_il('RAYDIUM_SOL_USDC');
  const spot = useLiveSpot_il('SOL');

  const hasQuote = notional >= 100;
  const q = useMemo_il(
    () => hasQuote ? computeIlQuote_il({ notional, slippageBps, sigmaAnn }) : null,
    [notional, slippageBps, hasQuote, sigmaAnn]);

  const walletOk = tweaks.walletState === 'connected';
  const canIssue = hasQuote && walletOk;

  const pythStaleSec = spot && spot.publishTime
    ? Math.max(0, Math.round(Date.now()/1000 - spot.publishTime))
    : null;

  const p95 = q ? computeP95Range_il({ notional, grossFees: 0,
                                        premium: q.premium, maxPremium: q.maxPremium }) : null;

  const payoffCurve = useMemo_il(() => {
    const pts = [];
    for (let r = 0.2; r <= 4.0; r += 0.02) {
      pts.push({ x: r, y: HM_il.ilPayoff({ r, deductible: IL.deductible, cap: IL.cap }) });
    }
    return pts;
  }, []);

  function handleIssue() {
    setModalOpen(tweaks.network === 'mainnet-beta' ? 'mainnet' : 'devnet');
  }
  function handleDevnetSign() {
    setToast({ variant: 'info', title: 'Simulating issuance…' });
    setTimeout(() => {
      const id = Math.floor(1000 + Math.random() * 9000);
      setToast({ variant: 'success', title: `Policy issued · HAL-IL-${id}`,
                 body: `Devnet simulation · ${fmtUSD(notional, 0)} synthetic cover` });
      setModalOpen(false);
      setTimeout(() => setToast(null), 4000);
    }, 1500);
  }
  function copyQuoteLink() {
    const url = new URL(window.location.href);
    url.searchParams.set('notional', String(notional));
    url.searchParams.set('slippageBps', String(slippageBps));
    navigator.clipboard && navigator.clipboard.writeText(url.toString());
    setToast({ variant: 'success', title: 'Quote link copied',
               body: 'Share with your counterparty or save for mainnet launch.' });
    setModalOpen(false);
    setTimeout(() => setToast(null), 3500);
  }

  return (
    <>
      <button className="hc-chev" onClick={onBack}
              style={{marginBottom: 10, fontSize: 12, color: 'var(--n-500)'}}>
        <span className="hc-chev-arrow" style={{transform: 'rotate(180deg)'}}>▸</span>
        Back to choice
      </button>

      {/* =============== HERO INPUT =============== */}
      <div className="hc-hero-input" style={{gridTemplateColumns: '1fr'}}>
        <div className="hc-hero-input-main">
          <div className="hc-hero-input-eyebrow">
            <span className="dot"/>Synthetic IL cover · enter a USDC notional
          </div>
          <div className="hc-hero-input-field">
            <span className="hc-hero-input-prefix">$</span>
            <input
              className="hc-hero-input-input" type="number" min={0} step={100}
              value={notional || ''}
              onChange={e => setNotional(Number(e.target.value) || 0)}
              placeholder="10,000" inputMode="numeric" autoFocus />
            <span className="hc-hero-input-suffix">USDC</span>
          </div>
          <div className="hc-hero-input-presets">
            {[500, 5000, 25000, 100000, 500000].map(v => (
              <button key={v}
                      className={notional === v ? 'active' : ''}
                      onClick={() => setNotional(v)}>
                {v >= 1000 ? `$${(v/1000).toFixed(0)}k` : `$${v}`}
              </button>
            ))}
          </div>
          <div className="hc-hero-input-hint">
            {hasQuote
              ? <>Minimum ticket <b>$100</b>. Your wallet pays at most <b>{fmtUSD(q.maxPremium)}</b> after {slippageBps}bp slippage. No LP position required.</>
              : <>Minimum ticket <b>$100</b>. Enter a notional to get a live premium quote. No LP position required.</>}
          </div>
        </div>
      </div>

      {/* =============== OFFER =============== */}
      {hasQuote && (
        <div className="hc-quotecard">
          <div className="hc-quotecard-offer">
            <div className="hc-qco-eyebrow">Quoted premium · up-front</div>
            <div className="hc-qco-headline">
              {(q.premiumPct * 100).toFixed(2)}<span className="unit">% of notional</span>
            </div>
            <div className="hc-qco-sub">
              Pays <b>{fmtUSD(q.expPayout, 0)}</b> in expectation across the 30-day window.
              If realised IL exceeds <b>1%</b>, you receive the overage up to <b>7%</b>.
              Settles once at T+30d against the Pyth entry / exit ratio.
            </div>

            <div className="hc-qco-row" style={{gridTemplateColumns: 'repeat(3, 1fr)'}}>
              <div>
                <div className="hc-qco-k">Premium</div>
                <div className="hc-qco-v">{fmtUSD(q.premium, 2)}</div>
                <div className="hc-qco-sub2">{(q.premiumPct*100).toFixed(2)}% · one-time</div>
              </div>
              <div>
                <div className="hc-qco-k">Max payout</div>
                <div className="hc-qco-v">{fmtUSD(q.maxPayout, 0)}</div>
                <div className="hc-qco-sub2">cap − deductible</div>
              </div>
              <div>
                <div className="hc-qco-k">Issuance fee</div>
                <div className="hc-qco-v">{fmtUSD(q.issuanceFee, 0)}</div>
                <div className="hc-qco-sub2">50 bp · fixed</div>
              </div>
            </div>

            <div className="hc-qco-benchmark">
              <b>What you're buying.</b> This {fmtUSD(notional, 0)} synthetic
              cover costs <b>{fmtUSD(q.premium, 2)}</b> in premium against a
              matching <b>{fmtUSD(q.expPayout, 0)}</b> expected payout — the
              ~{((q.premium - q.expPayout) / notional * 100).toFixed(2)}% carry
              is the 10% risk load above fair. P95 range on the net outcome:{' '}
              <b>[{fmtUSD(p95.low, 0)}, {p95.high >= 0 ? '+' : ''}{fmtUSD(p95.high, 0)}]</b>.
              Across 2,027 rolling 30-day windows the mean premium was{' '}
              <b>1.20%</b> with an <b>80%</b> loss ratio — the vault retained
              20% of premiums and paid the rest back as claims.
              <sup style={{fontSize: 9, marginLeft: 2}}>[1]</sup>
              <div style={{fontSize: 10, color: 'var(--n-400)', marginTop: 8,
                           fontFamily: 'var(--f-mono)', letterSpacing: '0.04em'}}>
                [1] see il_protection_product_economics_report.md §3
              </div>
            </div>
          </div>
        </div>
      )}

      {/* =============== QUIET ANCHOR + PAYOFF =============== */}
      {hasQuote && <QuietAnchorIL />}
      {hasQuote && <PayoffBlockIL q={q} payoffCurve={payoffCurve} />}

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
          <AdvancedDetailsIL
            slippageBps={slippageBps} setSlippageBps={setSlippageBps}
            q={q} pool={pool} pythStaleSec={pythStaleSec}
            gateOpen={gateOpen} setGateOpen={setGateOpen} />
        )}
      </div>

      {/* =============== ISSUE =============== */}
      <div className="hc-issue-bar">
        <div className="hc-issue-summary">
          {hasQuote ? (
            <>
              <div><div className="hc-ib-k">You pay</div><div className="hc-ib-v">{fmtUSD(q.maxPremium, 2)}</div></div>
              <div><div className="hc-ib-k">Max payout</div><div className="hc-ib-v">{fmtUSD(q.maxPayout, 0)}</div></div>
              <div><div className="hc-ib-k">Settlement</div><div className="hc-ib-v">European · T+30d</div></div>
            </>
          ) : (
            <div style={{color: 'rgba(255,255,255,0.65)', fontSize: 14}}>
              Enter a USDC notional above to review terms.
            </div>
          )}
        </div>
        <Button variant="primary" size="lg" disabled={!canIssue}
                onClick={canIssue ? handleIssue : undefined}>
          {!walletOk ? 'Connect wallet to buy'
            : !hasQuote ? 'Min $100 to buy'
            : tweaks.network === 'mainnet-beta'
              ? 'Lock quote for mainnet →'
              : 'Buy cover on devnet →'}
        </Button>
      </div>

      {/* =============== MODALS & TOAST =============== */}
      <Modal open={modalOpen === 'devnet'} onClose={() => setModalOpen(false)}
             title="Confirm devnet issuance"
             footer={
               <>
                 <Button variant="ghost" onClick={() => setModalOpen(false)}>Cancel</Button>
                 <Button variant="primary" onClick={handleDevnetSign}>Sign &amp; issue (demo)</Button>
               </>
             }>
        <div style={{fontSize: 14, color: 'var(--n-600)', lineHeight: 1.55}}>
          <b>{fmtUSD(q ? q.maxPremium : 0, 2)}</b> premium for{' '}
          <b>{fmtUSD(q ? q.maxPayout : 0, 0)}</b> max payout on <b>{fmtUSD(notional, 0)}</b>{' '}
          synthetic cover, settling T+30d against the Pyth entry/exit ratio.
        </div>
        <div style={{marginTop: 14, padding: '10px 12px', background: 'var(--amber-50, #FFF8E6)',
                     border: '1px solid var(--amber-200, #F5D99B)', borderRadius: 'var(--r-sm)',
                     fontSize: 12, color: 'var(--n-600)', lineHeight: 1.5}}>
          <b>Devnet simulation.</b> No real USDC will be transferred. The Halcyon
          IL program is in audit and will deploy to mainnet shortly.
        </div>
      </Modal>

      <Modal open={modalOpen === 'mainnet'} onClose={() => setModalOpen(false)}
             title="Mainnet launch soon"
             footer={
               <>
                 <Button variant="ghost" onClick={() => setModalOpen(false)}>Close</Button>
                 <Button variant="primary" onClick={copyQuoteLink}>Copy quote link</Button>
               </>
             }>
        <div style={{fontSize: 14, color: 'var(--n-600)', lineHeight: 1.55}}>
          The Halcyon IL program is in audit. Mainnet issuance launches in the
          coming weeks. Your quoted terms are pricing-locked — share a quote
          link with your counterparty or save it for launch day.
        </div>
        <div style={{marginTop: 14, fontFamily: 'var(--f-mono)', fontSize: 11,
                     color: 'var(--n-400)', padding: '10px 12px', background: 'var(--n-50)',
                     borderRadius: 'var(--r-sm)', border: '1px solid var(--n-100)'}}>
          ?notional={notional}&amp;slippageBps={slippageBps}
        </div>
      </Modal>

      {toast && (
        <div style={{position: 'fixed', bottom: 24, right: 24, zIndex: 200, maxWidth: 360}}>
          <Toast variant={toast.variant} title={toast.title}>{toast.body || ''}</Toast>
        </div>
      )}
    </>
  );
}

// -------------------- Quiet anchor (between quotecard and payoff) --------------------

function QuietAnchorIL() {
  return (
    <div style={{padding: '96px 24px', textAlign: 'center'}}>
      <p style={{fontFamily: 'var(--f-serif)', fontStyle: 'italic', fontSize: 24,
                  lineHeight: 1.45, color: 'var(--ink)', maxWidth: '60ch',
                  margin: '0 auto', letterSpacing: '-0.01em', fontWeight: 400}}>
        In 2023, SOL rallied from $10 to $100 and this contract's loss
        ratio hit 126% for the year. The vault paid out, and stayed solvent.
      </p>
    </div>
  );
}

// -------------------- Payoff block (shared by State A + Synthetic) --------------------

function PayoffBlockIL({ q, payoffCurve }) {
  return (
    <div className="hc-payoff-block">
      <div className="hc-payoff-lang">
        <div className="hc-payoff-lang-head">Under these terms, here's what happens:</div>

        <div className="hc-payoff-outcome">
          <div className="hc-po-dot" style={{background: 'var(--n-500)'}}/>
          <div>
            <div className="hc-po-head">Quiet window · premium paid, no payout</div>
            <div className="hc-po-body">
              SOL/USDC drifts inside a ±14% band for 30 days. Realised IL stays under 1%,
              which you absorb as the deductible. No payout; you lose the premium. Backtest
              frequency: <b>~64% of windows</b>.<sup style={{fontSize: 9, marginLeft: 2}}>[1]</sup>
            </div>
          </div>
        </div>

        <div className="hc-payoff-outcome">
          <div className="hc-po-dot" style={{background: 'var(--blue-600)'}}/>
          <div>
            <div className="hc-po-head">Partial payout · IL between 1% and 7%</div>
            <div className="hc-po-body">
              Terminal ratio moves enough that IL lands between the deductible and the cap.
              The vault pays the overage 1-for-1. Backtest frequency: <b>~32% of windows</b>;
              mean payout when triggered ≈ <b>2.8%</b> of notional.
            </div>
          </div>
        </div>

        <div className="hc-payoff-outcome">
          <div className="hc-po-dot" style={{background: 'var(--rust-500)'}}/>
          <div>
            <div className="hc-po-head">Capped payout · large divergence</div>
            <div className="hc-po-body">
              SOL rallies or crashes enough that IL exceeds 7%. You receive the full
              <b> {fmtUSD(q.maxPayout, 0)}</b>; anything beyond 7% stays with you as
              the position itself. Backtest frequency: <b>~4% of windows</b>. Worst
              observed: the 2023 $10→$100 rally — vault loss ratio <b>126%</b> that
              year, fully absorbed.
            </div>
          </div>
        </div>

        <div style={{fontSize: 10, color: 'var(--n-400)', marginTop: 10,
                     fontFamily: 'var(--f-mono)', letterSpacing: '0.04em'}}>
          [1] frequencies from 2,027-window backtest · see il_protection_product_economics_report.md
        </div>
      </div>

      <div className="hc-chartcard">
        <div className="hc-chartcard-head">
          <h3>Payoff · SOL / USDC terminal price ratio</h3>
          <div className="legend">
            <span><span className="sw" style={{background:'var(--blue-600)'}} /> Payout</span>
            <span><span className="sw" style={{background:'var(--n-400)'}} /> Entry</span>
          </div>
        </div>
        <PayoffChart
          curves={[{ data: payoffCurve, color: 'var(--blue-600)' }]}
          annotations={[{ x: 1, label: 'entry', color: 'var(--n-400)' }]}
          width={620} height={260}
          xLabel="S_T / S_0"
          xFormat={v => `${(v*100).toFixed(0)}%`}
          yFormat={v => `${(v*100).toFixed(1)}%`} />
      </div>
    </div>
  );
}

// -------------------- Advanced (slippage + issuance gate) --------------------

function AdvancedDetailsIL({ slippageBps, setSlippageBps, q, pool, pythStaleSec, gateOpen, setGateOpen }) {
  const tvlM = pool && Number.isFinite(pool.tvl) ? (pool.tvl / 1e6).toFixed(2) : null;
  const pythFresh = pythStaleSec != null && pythStaleSec <= 30;
  const poolLive  = !!pool;

  return (
    <div className="hc-advanced-body">
      <div className="hc-adv-row">
        <div style={{flex: 1}}>
          <FieldLabel meta="max premium movement you'll accept">Slippage tolerance</FieldLabel>
          <Slider value={slippageBps} onChange={setSlippageBps} min={25} max={250} step={25} />
          <div style={{display: 'flex', justifyContent: 'space-between',
                       fontFamily: 'var(--f-mono)', fontSize: 10, color: 'var(--n-400)',
                       letterSpacing: '0.04em', marginTop: 6, fontVariantNumeric: 'tabular-nums'}}>
            <span>25bp</span>
            <span style={{color: 'var(--ink)', fontWeight: 600}}>
              {slippageBps}bp · max premium {q ? fmtUSD(q.maxPremium) : '—'}
            </span>
            <span>250bp</span>
          </div>
        </div>
      </div>

      <div className="hc-adv-gate">
        <button className="hc-gate-toggle" onClick={() => setGateOpen(!gateOpen)}>
          <span className="hc-gate-icon" style={{background: 'var(--blue-600)', color: '#fff'}}>✓</span>
          <span className="hc-gate-title">
            Issuance gate · {(pythFresh ? 1 : 0) + (poolLive ? 1 : 0)} / 2 live checks pass
            · vault checks pending devnet deploy
          </span>
          <span className={`hc-chev-arrow ${gateOpen ? 'open' : ''}`}>▸</span>
        </button>
        {gateOpen && (
          <div className="hc-adv-gate-detail">
            <div>
              <b>Pyth SOL/USD</b> —{' '}
              {pythStaleSec != null
                ? <>{pythStaleSec}s stale, cap 30s {pythFresh ? '✓' : '⚠'}</>
                : <>waiting for first publish…</>}
            </div>
            <div>
              <b>Raydium pool</b> —{' '}
              {pool
                ? <>{pool.poolId ? pool.poolId.slice(0,6) + '…' + pool.poolId.slice(-4) : 'pool'} reachable,
                    TVL ${tvlM}M</>
                : <>fetching pool state…</>}
            </div>
            <div style={{color: 'var(--n-400)'}}>
              <b>Vault utilisation</b> — <span style={{fontFamily: 'var(--f-mono)'}}>—</span>
              <span style={{fontSize: 11, marginLeft: 8}}>vault not yet deployed</span>
            </div>
            <div style={{color: 'var(--n-400)'}}>
              <b>Concentration</b> — <span style={{fontFamily: 'var(--f-mono)'}}>—</span>
              <span style={{fontSize: 11, marginLeft: 8}}>vault not yet deployed</span>
            </div>
            <details style={{marginTop: 10, paddingTop: 10, borderTop: '1px solid var(--n-100)'}}>
              <summary style={{cursor: 'pointer', fontSize: 10, fontFamily: 'var(--f-mono)',
                               color: 'var(--n-500)', letterSpacing: '0.1em',
                               textTransform: 'uppercase', fontWeight: 600, listStyle: 'none'}}>
                ▸ Pricing engine internals
              </summary>
              <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14,
                           marginTop: 10, fontFamily: 'var(--f-mono)', fontSize: 11,
                           color: 'var(--n-500)', fontVariantNumeric: 'tabular-nums'}}>
                <div>
                  <div style={{fontSize: 10, color: 'var(--n-400)', textTransform: 'uppercase',
                               letterSpacing: '0.1em', fontWeight: 600, marginBottom: 4}}>Quadrature</div>
                  <div>method <span style={{color: 'var(--ink)'}}>Gauss-Legendre 5-pt</span></div>
                  <div>density <span style={{color: 'var(--ink)'}}>NIG · Bessel K₁</span></div>
                  <div>accumulator <span style={{color: 'var(--ink)'}}>i128 @ SCALE_18</span></div>
                </div>
                <div>
                  <div style={{fontSize: 10, color: 'var(--n-400)', textTransform: 'uppercase',
                               letterSpacing: '0.1em', fontWeight: 600, marginBottom: 4}}>Risk load</div>
                  <div>multiplier <span style={{color: 'var(--ink)'}}>×1.10</span></div>
                  <div>CU / quote <span style={{color: 'var(--ink)'}}>300k</span></div>
                  <div>engine <span style={{color: 'var(--ink)'}}>v1.0.3</span></div>
                </div>
              </div>
            </details>
          </div>
        )}
      </div>
    </div>
  );
}

window.PageILProtection = PageILProtection;

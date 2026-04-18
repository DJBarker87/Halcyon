/* global React, PriceChart */
// Halcyon — Shared Vault page

const { useState: useState_v, useMemo: useMemo_v } = React;
const HM_v = window.HalcyonMath;

function PageVault({ tweaks }) {
  const [tranche, setTranche] = useState_v('senior');
  const [amount, setAmount] = useState_v(25_000);

  const seniorTVL = 8_420_000;
  const juniorTVL = 1_580_000;
  const totalTVL = seniorTVL + juniorTVL;
  const reserved = 6_400_000;
  const utilisation = reserved / totalTVL;

  const nav = useMemo_v(() => HM_v.mockSeries({ days: 180, start: 1.000, vol: 0.002, drift: 0.00022, seed: 7 })
                              .map(p => ({ ...p, y: Math.max(0.99, p.y) })), []);

  const canDeposit = amount >= 100 && tweaks.walletState === 'connected';

  return (
    <div className="hc-page">
      <PageHead
        eyebrow="Capital · Shared underwriting vault"
        title="Vault"
        sub={<>One pool backs all three products. Senior tranche earns coupon flow with a 7-day cooldown; junior tranche absorbs first loss and cannot withdraw while any policy is active. Founder-seeded junior at v1.</>}
        meta={
          <>
            <div className="hc-kv-inline"><span>Total TVL</span><b>{fmtUSD(totalTVL, 0)}</b></div>
            <div className="hc-kv-inline"><span>Utilisation</span><b>{(utilisation*100).toFixed(1)}%</b></div>
            <div className="hc-kv-inline"><span>Insolvencies</span><b>0</b></div>
          </>
        }
      />

      {/* Tranche capital stack */}
      <div className="hc-section-title">
        <span>Capital stack</span>
        <span className="meta">Senior first-paid · junior first-loss</span>
      </div>

      <div className="hc-vault-stack">
        <div className="hc-tranche hc-tranche--senior">
          <div className="name">Senior tranche</div>
          <div className="amount">
            {fmtUSD(seniorTVL, 0).replace('$', '$')}
            <span className="unit">USDC</span>
          </div>
          <div className="share">{((seniorTVL/totalTVL)*100).toFixed(1)}% of TVL · 128 depositors · 7-day cooldown</div>
          <div style={{display:'flex', gap: 16, marginTop: 12, paddingTop: 12,
                       borderTop: '1px solid var(--n-100)',
                       fontFamily: 'var(--f-mono)', fontSize: 11, color: 'var(--n-500)',
                       letterSpacing: '0.04em'}}>
            <span>TTM yield · <b style={{color: 'var(--success-700)'}}>+8.4%</b></span>
            <span>30d yield · <b style={{color: 'var(--success-700)'}}>+9.1%</b></span>
          </div>
        </div>

        <div className="hc-tranche hc-tranche--junior">
          <div className="name">Junior tranche</div>
          <div className="amount">
            {fmtUSD(juniorTVL, 0)}
            <span className="unit">USDC</span>
          </div>
          <div className="share">{((juniorTVL/totalTVL)*100).toFixed(1)}% of TVL · founder-seeded · non-withdrawable while active</div>
          <div style={{display:'flex', gap: 16, marginTop: 12, paddingTop: 12,
                       borderTop: '1px solid var(--n-100)',
                       fontFamily: 'var(--f-mono)', fontSize: 11, color: 'var(--n-500)',
                       letterSpacing: '0.04em'}}>
            <span>TTM yield · <b style={{color: 'var(--success-700)'}}>+16.2%</b></span>
            <span>30d yield · <b style={{color: 'var(--success-700)'}}>+18.7%</b></span>
          </div>
        </div>
      </div>

      {/* Utilisation */}
      <div className="hc-util">
        <div className="hc-util-head">
          <span className="label">Vault utilisation · reserved / deposits</span>
          <span className="value">{(utilisation*100).toFixed(1)}%</span>
        </div>
        <div className="hc-util-bar">
          <div className="hc-util-bar-fill" style={{width: `${utilisation*100}%`}} />
          <div className="hc-util-bar-kink" style={{left: '90%'}} title="90% hard cap" />
        </div>
        <div className="hc-util-legend">
          <span>0%</span>
          <span>Reserved {fmtUSD(reserved)}</span>
          <span>Cap 90%</span>
        </div>
      </div>

      <div className="hc-workbench" style={{marginTop: 32}}>
        {/* LEFT — deposit */}
        <div className="hc-panel">
          <div className="hc-panel-head">
            <h3>Deposit</h3>
            <span className="meta">kernel · reserve_and_issue</span>
          </div>
          <div className="hc-panel-body hc-form-group">

            <div className="field">
              <FieldLabel>Tranche</FieldLabel>
              <div style={{display:'flex', flexDirection:'column', gap: 8}}>
                {[
                  {id: 'senior', name: 'Senior', sub: 'Coupon flow · 7-day cooldown', y: '+8.4% TTM'},
                  {id: 'junior', name: 'Junior', sub: 'First-loss · locked while active', y: '+16.2% TTM'}
                ].map(t => (
                  <button key={t.id}
                          className={`hc-ul-chip ${tranche === t.id ? 'on' : ''}`}
                          onClick={() => setTranche(t.id)}
                          style={{flexDirection: 'row', justifyContent:'space-between', alignItems:'center'}}>
                    <div>
                      <div className="ticker" style={{fontFamily:'var(--f-serif)',fontSize:18,letterSpacing:0}}>{t.name}</div>
                      <div className="name" style={{textTransform:'none',letterSpacing:0,fontSize:11,color:'var(--n-500)'}}>{t.sub}</div>
                    </div>
                    <span style={{fontFamily:'var(--f-mono)',fontSize:13,color:'var(--success-700)',fontWeight:600}}>{t.y}</span>
                  </button>
                ))}
              </div>
            </div>

            <div className="field">
              <FieldLabel meta="USDC">Amount</FieldLabel>
              <div className="input-wrap has-prefix">
                <span className="input-prefix">$</span>
                <input className="input" type="number" value={amount} step={500}
                       onChange={e => setAmount(Number(e.target.value))} />
                <span className="input-suffix">USDC</span>
              </div>
              <div style={{display: 'flex', gap: 6, marginTop: 8}}>
                {[1000, 5000, 25000, 100000].map(v => (
                  <button key={v} className="btn btn-ghost btn-sm" onClick={() => setAmount(v)}>
                    {v >= 1000 ? (v/1000)+'k' : v}
                  </button>
                ))}
              </div>
            </div>

            <div className="hc-form-sep" />

            <Button variant="primary" size="lg" disabled={!canDeposit}>
              {canDeposit ? `Deposit ${fmtUSD(amount)} → ${tranche}` :
                (tweaks.walletState !== 'connected' ? 'Connect wallet' : 'Amount too small')}
            </Button>

            <div style={{fontFamily:'var(--f-mono)', fontSize: 10, color: 'var(--n-400)',
                         letterSpacing:'0.06em', textTransform: 'uppercase', marginTop: 4, textAlign:'center'}}>
              7-day cooldown · kernel PDA · upgradeable → frozen 3-6mo post launch
            </div>
          </div>
        </div>

        {/* RIGHT — NAV + breakdown */}
        <div className="hc-row-6">
          <div className="hc-chartcard">
            <div className="hc-chartcard-head">
              <h3>Senior NAV · 180 days</h3>
              <div className="legend">
                <span><span className="sw" style={{background:'var(--blue-600)'}} /> NAV per share</span>
              </div>
            </div>
            <PriceChart data={nav} height={180} color="var(--blue-600)"
                        yFormat={v => v.toFixed(4)} />
          </div>

          <div className="hc-grid-2">
            <div className="hc-panel">
              <div className="hc-panel-head">
                <h3>Liability by product</h3>
                <span className="meta">reserved / notional</span>
              </div>
              <div className="hc-panel-body" style={{padding: 16}}>
                {[
                  {name: 'Equity Autocall', res: 4_100_000, notes: 64, color: 'var(--blue-600)'},
                  {name: 'IL Protection', res: 820_000, notes: 142, color: 'var(--blue-400)'},
                  {name: 'SOL Autocall', res: 1_480_000, notes: 38, color: 'var(--rust-500)'}
                ].map(p => {
                  const pct = p.res / reserved * 100;
                  return (
                    <div key={p.name} style={{marginBottom: 12}}>
                      <div style={{display:'flex',justifyContent:'space-between',marginBottom:4,
                                   fontSize: 12}}>
                        <span style={{color:'var(--ink)'}}>{p.name}</span>
                        <span style={{fontFamily:'var(--f-mono)',color:'var(--n-500)',fontSize:11}}>
                          {fmtUSD(p.res, 0)} · {p.notes} notes
                        </span>
                      </div>
                      <div style={{height: 6, background: 'var(--n-100)', borderRadius: 3, overflow: 'hidden'}}>
                        <div style={{height: '100%', width: `${pct}%`, background: p.color}} />
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>

            <div className="hc-panel">
              <div className="hc-panel-head">
                <h3>Recent events</h3>
                <span className="meta">last 24h</span>
              </div>
              <div className="hc-panel-body" style={{padding: '8px 0'}}>
                {[
                  {time: '2m',  type: 'coupon', what: 'Equity #0042 · monthly coupon', amt: '+$128.42'},
                  {time: '14m', type: 'obs',    what: 'SOL #0112 · obs 3 · barrier clear', amt: null},
                  {time: '1h',  type: 'issue',  what: 'IL Protection · $10k · SOL/USDC', amt: '−$47.20'},
                  {time: '3h',  type: 'settle', what: 'SOL #0098 · autocalled at obs 2', amt: '+$5,125'},
                  {time: '6h',  type: 'hedge',  what: 'Flagship · rebalance SPYx +0.42', amt: null},
                ].map((e, i) => (
                  <div key={i} style={{padding: '8px 16px', borderBottom: '1px solid var(--n-50)',
                                       display: 'flex', alignItems: 'center', gap: 10, fontSize: 12}}>
                    <Badge variant={e.type === 'settle' || e.type === 'coupon' ? 'success' :
                                     e.type === 'issue' ? 'info' : 'neutral'}>
                      {e.type}
                    </Badge>
                    <span style={{flex: 1, color: 'var(--n-700)'}}>{e.what}</span>
                    {e.amt && <span style={{fontFamily: 'var(--f-mono)', fontSize: 11,
                                            color: e.amt.startsWith('+') ? 'var(--success-700)' : 'var(--n-600)',
                                            fontVariantNumeric: 'tabular-nums'}}>
                      {e.amt}
                    </span>}
                    <span style={{fontFamily:'var(--f-mono)',fontSize:10,color:'var(--n-400)'}}>{e.time}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Backtest */}
      <div className="hc-section-title">
        <span>Backtest · combined vault</span>
        <span className="meta">20y walk-forward · all three products active</span>
      </div>

      <div className="hc-grid-3">
        <StatBlock boxed label="Senior return" value="+6.8" unit="% p.a." />
        <StatBlock boxed label="Junior return" value="+14.2" unit="% p.a." />
        <StatBlock boxed label="Sharpe" value="2.4" unit="vault-weighted" />
        <StatBlock boxed label="Worst month" value="−1.4" unit="% senior" />
        <StatBlock boxed label="Worst month · junior" value="−8.7" unit="%" />
        <StatBlock boxed label="Coverage ratio" value="1.84" unit="× reserved" />
      </div>
    </div>
  );
}

window.PageVault = PageVault;

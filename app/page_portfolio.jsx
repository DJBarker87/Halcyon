/* global React */
// Halcyon — Portfolio page

const { useState: useState_p } = React;

const POSITIONS = [
  { id: 'HAL-EQ-0042', product: 'equity', name: 'Worst-of SPY·QQQ·IWM', notional: 50_000,
    opened: '2026-03-14', expires: '2027-09-14', coupon: '17.80%', status: 'active',
    pnl: +2_430, nextObs: 'T+14d · monthly coupon' },
  { id: 'HAL-SOL-0112', product: 'sol', name: 'SOL 16d Autocall', notional: 5_000,
    opened: '2026-04-09', expires: '2026-04-25', coupon: '2.60%/obs', status: 'active',
    pnl: +130, nextObs: 'T+2d · obs 3' },
  { id: 'HAL-IL-0087', product: 'il', name: 'IL cover · SOL/USDC', notional: 10_000,
    opened: '2026-04-01', expires: '2026-05-01', coupon: '0.58%', status: 'active',
    pnl: -47, nextObs: 'T+14d · settlement' },
  { id: 'HAL-EQ-0038', product: 'equity', name: 'Worst-of SPY·QQQ', notional: 100_000,
    opened: '2025-01-04', expires: '2026-07-04', coupon: '14.40%', status: 'settled',
    pnl: +18_720, nextObs: null },
  { id: 'HAL-SOL-0098', product: 'sol', name: 'SOL 16d Autocall', notional: 3_000,
    opened: '2026-04-05', expires: '2026-04-21', coupon: '2.40%/obs', status: 'autocalled',
    pnl: +125, nextObs: null },
];

function PagePortfolio({ tweaks }) {
  const [filter, setFilter] = useState_p('all');

  const filtered = POSITIONS.filter(p =>
    filter === 'all' ? true :
    filter === 'active' ? p.status === 'active' :
    p.product === filter
  );

  const aggNotional = POSITIONS.filter(p => p.status === 'active').reduce((s,p) => s + p.notional, 0);
  const aggPnL = POSITIONS.filter(p => p.status === 'active').reduce((s,p) => s + p.pnl, 0);
  const aggClosed = POSITIONS.filter(p => p.status !== 'active').reduce((s,p) => s + p.pnl, 0);

  return (
    <div className="hc-page">
      <PageHead
        eyebrow="Your positions"
        title="Portfolio"
        sub="All Halcyon notes held by the connected wallet, across products."
        meta={
          <>
            <div className="hc-kv-inline"><span>Active notional</span><b>{fmtUSD(aggNotional, 0)}</b></div>
            <div className="hc-kv-inline"><span>Unrealised P&L</span><b style={{color:'var(--success-700)'}}>+{fmtUSD(aggPnL, 0)}</b></div>
            <div className="hc-kv-inline"><span>Realised (lifetime)</span><b style={{color:'var(--success-700)'}}>+{fmtUSD(aggClosed, 0)}</b></div>
          </>
        }
      />

      <div style={{display:'flex', alignItems:'center', gap: 8, marginBottom: 16}}>
        <SegControl
          options={[
            {label:'All',value:'all'},
            {label:'Active',value:'active'},
            {label:'Equity',value:'equity'},
            {label:'IL',value:'il'},
            {label:'SOL',value:'sol'}
          ]}
          value={filter} onChange={setFilter}
        />
      </div>

      <div className="hc-table-wrap">
        <div className="hc-table-head">
          <h3>Positions · {filtered.length}</h3>
          <span className="meta">sorted by opened desc</span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>ID</th>
              <th>Product</th>
              <th className="num">Notional</th>
              <th>Coupon</th>
              <th>Opened</th>
              <th>Expires</th>
              <th>Status</th>
              <th className="num">P&L</th>
              <th>Next event</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {filtered.map(p => (
              <tr key={p.id}>
                <td className="mono" style={{fontSize:11, color:'var(--n-500)'}}>{p.id}</td>
                <td>{p.name}</td>
                <td className="num mono">{fmtUSD(p.notional, 0)}</td>
                <td className="mono" style={{fontSize:12}}>{p.coupon}</td>
                <td className="mono" style={{fontSize:11, color:'var(--n-500)'}}>{p.opened}</td>
                <td className="mono" style={{fontSize:11, color:'var(--n-500)'}}>{p.expires}</td>
                <td>
                  <Badge variant={
                    p.status === 'active' ? 'info' :
                    p.status === 'settled' ? 'neutral' :
                    'success'
                  }>{p.status}</Badge>
                </td>
                <td className="num mono" style={{
                  color: p.pnl >= 0 ? 'var(--success-700)' : 'var(--error-700)',
                  fontWeight: 600
                }}>
                  {p.pnl >= 0 ? '+' : ''}{fmtUSD(Math.abs(p.pnl), 0)}
                </td>
                <td style={{fontSize:12, color: p.nextObs ? 'var(--n-600)' : 'var(--n-300)'}}>
                  {p.nextObs || '—'}
                </td>
                <td><Button variant="ghost" size="sm">View</Button></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="hc-section-title" style={{marginTop: 32}}>
        <span>Upcoming events</span>
        <span className="meta">observation keeper schedule</span>
      </div>

      <div className="hc-panel">
        <div className="hc-panel-body" style={{padding: 0}}>
          {[
            {when: 'in 1d 18h', event: 'SOL #0112 · observation 3', detail: 'Autocall if SOL ≥ $183.30'},
            {when: 'in 14d',   event: 'IL #0087 · settlement',      detail: 'Against Pyth SOL/USDC ratio'},
            {when: 'in 14d',   event: 'Equity #0042 · monthly coupon', detail: 'Pays if worst ≥ 100%'},
            {when: 'in 76d',   event: 'Equity #0042 · quarterly autocall', detail: 'Earliest autocall window'},
          ].map((e,i) => (
            <div key={i} style={{display:'flex', padding: '12px 24px',
                                 borderBottom: i < 3 ? '1px solid var(--n-50)' : 'none',
                                 gap: 16, alignItems: 'center'}}>
              <div style={{fontFamily: 'var(--f-mono)', fontSize: 11, color: 'var(--n-400)',
                           letterSpacing: '0.06em', textTransform: 'uppercase',
                           minWidth: 90}}>{e.when}</div>
              <div style={{flex: 1}}>
                <div style={{fontSize: 13, color: 'var(--ink)'}}>{e.event}</div>
                <div style={{fontSize: 12, color: 'var(--n-500)', marginTop: 2}}>{e.detail}</div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

window.PagePortfolio = PagePortfolio;

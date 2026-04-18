/* global React */
// Halcyon — App shell: sidebar, topbar, page chrome

const { useState: useState_sh, useEffect: useEffect_sh } = React;

// ---- WASM status badge ----
// Tracks `window.HalcyonMath.wasmReady`; flips to "live" when wasm_loader.js
// fires the `halcyon-wasm-ready` event.
function WasmStatus() {
  const [ready, setReady] = useState_sh(() => !!(window.HalcyonMath && window.HalcyonMath.wasmReady));
  useEffect_sh(() => {
    if (ready) return;
    const h = () => setReady(true);
    window.addEventListener('halcyon-wasm-ready', h);
    return () => window.removeEventListener('halcyon-wasm-ready', h);
  }, [ready]);
  return ready ? (
    <>
      <span title="solmath-core i128 fixed-point math running in the browser">solmath-core 0.1.2</span>
      <span style={{color: 'var(--success-700)'}}>● wasm</span>
    </>
  ) : (
    <>
      <span>solmath-core 0.1.2</span>
      <span style={{color: 'var(--n-400)'}}>○ loading…</span>
    </>
  );
}

// ---- Sidebar ----
function Sidebar({ route, onRoute, network }) {
  const navItems = [
    { group: 'Issue' },
    { id: 'equity', name: 'Equity Autocall', tag: 'SPY · QQQ · IWM', count: 'v1.2.0' },
    { id: 'il',     name: 'IL Protection',   tag: 'SOL/USDC · Raydium', count: 'v1.0.3' },
    { id: 'sol',    name: 'SOL Autocall',    tag: '16-day · 8 obs', count: 'v1.1.1' },
    { group: 'Capital' },
    { id: 'vault',  name: 'Shared Vault',    tag: 'Senior · Junior', count: '' },
    { id: 'portfolio', name: 'Portfolio',    tag: 'Active policies', count: '3' },
  ];
  return (
    <nav className="hc-side">
      <div className="hc-brand">
        <King size={32} color="var(--blue-600)" />
        <div>
          <div className="hc-brand-word">Halcyon</div>
          <div className="hc-brand-sub">Issuance · {network === 'mainnet-beta' ? 'Mainnet' : 'Devnet'}</div>
        </div>
      </div>

      {navItems.map((item, i) => {
        if (item.group) {
          return <div key={'g'+i} className="hc-nav-group-label" style={{marginTop: i > 0 ? 8 : 0}}>{item.group}</div>;
        }
        return (
          <button key={item.id}
                  className={`hc-nav-item ${route === item.id ? 'active' : ''}`}
                  onClick={() => onRoute(item.id)}>
            <NavIcon id={item.id} />
            <div style={{minWidth: 0, flex: 1}}>
              <div style={{whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis'}}>{item.name}</div>
              <div style={{fontSize: 10, color: 'var(--n-400)', fontFamily: 'var(--f-mono)',
                           letterSpacing: '0.04em', marginTop: 1}}>{item.tag}</div>
            </div>
            {item.count && <span className="hc-nav-counter">{item.count}</span>}
          </button>
        );
      })}

      <div className="hc-side-footer">
        <WasmStatus />
      </div>
    </nav>
  );
}

function NavIcon({ id }) {
  const stroke = 'currentColor';
  const common = { width: 16, height: 16, viewBox: '0 0 16 16', fill: 'none', stroke, strokeWidth: 1.5, strokeLinecap: 'round', strokeLinejoin: 'round' };
  switch (id) {
    case 'equity':
      return <svg {...common} className="icon"><path d="M2 12l3-4 3 2 5-7"/><path d="M10 3h3v3"/></svg>;
    case 'il':
      return <svg {...common} className="icon"><path d="M8 2c3 4 4 6 4 8a4 4 0 01-8 0c0-2 1-4 4-8z"/></svg>;
    case 'sol':
      return <svg {...common} className="icon"><rect x="2" y="4" width="12" height="8" rx="1"/><path d="M4 8h8M4 6h6M4 10h6"/></svg>;
    case 'vault':
      return <svg {...common} className="icon"><rect x="2" y="3" width="12" height="10" rx="1"/><circle cx="8" cy="8" r="2"/><path d="M8 5v1M8 10v1M5 8h1M10 8h1"/></svg>;
    case 'portfolio':
      return <svg {...common} className="icon"><rect x="2" y="4" width="12" height="9" rx="1"/><path d="M6 4V2h4v2"/><path d="M2 8h12"/></svg>;
    default:
      return <svg {...common} className="icon"><circle cx="8" cy="8" r="5"/></svg>;
  }
}

// ---- Top bar ----
function TopBar({ route, network, walletState }) {
  const routeLabels = {
    equity: ['Issue', 'Equity Autocall · SPY/QQQ/IWM'],
    il: ['Issue', 'IL Protection · SOL/USDC'],
    sol: ['Issue', 'SOL Autocall'],
    vault: ['Capital', 'Shared Underwriting Vault'],
    portfolio: ['Capital', 'Portfolio'],
  };
  const [g, n] = routeLabels[route] || ['', ''];

  // Live SOL from Pyth Hermes; SPY/QQQ/IWM are placeholders until their
  // Hermes feeds are wired (requires mainnet equity feed IDs).
  const [solSpot, setSolSpot] = useState_sh(() => {
    const s = window.HalcyonOracles && window.HalcyonOracles.getSpot('SOL');
    return s ? s.price : null;
  });
  useEffect_sh(() => {
    if (!window.HalcyonOracles) return;
    return window.HalcyonOracles.subscribeSpot('SOL', ({ price }) => setSolSpot(price));
  }, []);

  // Only show tickers that are contextually live on the current page. SPY /
  // QQQ / IWM placeholders are hidden on non-equity routes so they don't
  // silently go stale on SOL-based products.
  const solQuote = {
    k: 'SOL',
    v: solSpot != null ? solSpot.toFixed(2) : '—',
    d: null,
    live: solSpot != null,
  };
  const equityQuotes = [
    { k: 'SPY', v: '580.42', d: +0.18, live: false },
    { k: 'QQQ', v: '512.67', d: -0.22, live: false },
    { k: 'IWM', v: '229.81', d: +0.44, live: false },
  ];
  const quotes =
      route === 'il'     ? [solQuote]
    : route === 'sol'    ? [solQuote]
    : route === 'equity' ? [...equityQuotes, solQuote]
    :                      [...equityQuotes, solQuote];

  return (
    <div className="hc-topbar">
      <span className="crumb">
        <span>Halcyon</span>
        <span className="sep">/</span>
        <span>{g}</span>
        <span className="sep">/</span>
        <b>{n}</b>
      </span>
      <span className="spacer" />
      <div className="tb-meta">
        {quotes.map(q => (
          <span key={q.k} className="tb-quote" title={q.live ? 'Pyth Hermes' : 'placeholder'}>
            <span className="k">{q.k}{q.live && <span style={{color: 'var(--success-700)', marginLeft: 3}}>●</span>}</span>
            <span className="v">{q.v}</span>
            {q.d != null && (
              <span className={q.d >= 0 ? 'pos' : 'neg'}>
                {q.d >= 0 ? '+' : ''}{q.d.toFixed(2)}%
              </span>
            )}
          </span>
        ))}
      </div>
      <NetworkIndicator network={network || 'devnet'} />
      <WalletConnectButton state={walletState || 'connected'} />
    </div>
  );
}

// ---- Page head ----
function PageHead({ eyebrow, title, sub, meta }) {
  return (
    <div className="hc-page-head">
      <div>
        {eyebrow && <div style={{fontFamily: 'var(--f-mono)', fontSize: 11, letterSpacing: '0.14em',
                                  textTransform: 'uppercase', color: 'var(--rust-500)',
                                  fontWeight: 700, marginBottom: 10}}>{eyebrow}</div>}
        <h1>{title}</h1>
        {sub && <div className="sub" style={{marginTop: 10}}>{sub}</div>}
      </div>
      {meta && <div className="hp-meta">{meta}</div>}
    </div>
  );
}

// ---- Issuance gate banner ----
function IssuanceGate({ status = 'pass', title, children }) {
  return (
    <div className={`hc-gate hc-gate--${status}`}>
      <span className="hc-gate-icon">{status === 'pass' ? '✓' : status === 'warn' ? '!' : '×'}</span>
      <div className="hc-gate-body">
        <b>{title}</b>
        {children}
      </div>
    </div>
  );
}

// ---- Form field helpers ----
function FieldLabel({ children, meta }) {
  return (
    <div className="field-head">
      <span className="field-label">{children}</span>
      {meta && <span className="meta">{meta}</span>}
    </div>
  );
}

function SegControl({ options, value, onChange }) {
  return (
    <div className="hc-seg">
      {options.map(o => (
        <button key={o.value} className={`hc-seg-btn ${o.value === value ? 'on' : ''}`}
                onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

// ---- Money formatting ----
function fmtUSD(n, digits = 0) {
  return '$' + n.toLocaleString('en-US', { minimumFractionDigits: digits, maximumFractionDigits: digits });
}
function fmtPct(n, digits = 2) {
  return (n >= 0 ? '+' : '') + n.toFixed(digits) + '%';
}
function fmtPctAbs(n, digits = 2) {
  return n.toFixed(digits) + '%';
}
function fmtBps(n) {
  return n.toFixed(0) + ' bps';
}

Object.assign(window, {
  Sidebar, TopBar, PageHead, IssuanceGate, FieldLabel, SegControl,
  fmtUSD, fmtPct, fmtPctAbs, fmtBps
});

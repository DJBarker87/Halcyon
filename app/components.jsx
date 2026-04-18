/* global React, Kingfisher */
// Halcyon — shared components (React + Babel)
// Exposes all primitives to window for use by sections/*.jsx

const { useState, useEffect, useRef, useMemo } = React;

// -------------------- Kingfisher SVG (inline) --------------------
function King({ size = 24, color = 'currentColor', eye = true, className }) {
  const d = Kingfisher.MARK_PATH.replace(/\s+/g, ' ').trim();
  const E = Kingfisher.EYE;
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} className={className}
         style={{ display: 'inline-block', verticalAlign: 'middle', flex: 'none' }}
         role="img" aria-label="Halcyon">
      <g fill={color}>
        <path d={d} />
        {eye && size >= 16 && (
          <circle cx={E.cx} cy={E.cy} r={E.r} fill="var(--paper, #FAFAF7)" />
        )}
      </g>
    </svg>
  );
}

// -------------------- Buttons --------------------
function Button({ variant = 'primary', size = 'md', children, ...rest }) {
  return (
    <button className={`btn btn--${variant} btn--${size}`} {...rest}>{children}</button>
  );
}

// -------------------- Inputs --------------------
function Input({ label, suffix, prefix, ...rest }) {
  return (
    <label className="field">
      {label && <span className="field-label">{label}</span>}
      <span className={`input-wrap ${prefix ? 'has-prefix' : ''}`}>
        {prefix && <span className="input-prefix">{prefix}</span>}
        <input className="input" {...rest} />
        {suffix && <span className="input-suffix">{suffix}</span>}
      </span>
    </label>
  );
}

function NumberStepper({ value, onChange, step = 1, min, max, suffix }) {
  return (
    <div className="input-wrap" style={{ maxWidth: 200 }}>
      <input className="input" type="number" value={value}
             onChange={e => onChange(Number(e.target.value))}
             step={step} min={min} max={max}
             style={{ paddingRight: suffix ? 70 : 56, fontFeatureSettings: '"tnum" 1' }} />
      {suffix && <span className="input-suffix" style={{ right: 38 }}>{suffix}</span>}
      <div style={{
        position: 'absolute', right: 4, top: 4, bottom: 4,
        display: 'flex', flexDirection: 'column', gap: 1
      }}>
        <button onClick={() => onChange((value || 0) + step)}
                style={stepperBtnStyle}>▲</button>
        <button onClick={() => onChange((value || 0) - step)}
                style={stepperBtnStyle}>▼</button>
      </div>
    </div>
  );
}
const stepperBtnStyle = {
  background: 'var(--n-50)', border: 0, cursor: 'pointer',
  flex: 1, padding: '0 6px', fontSize: 8, color: 'var(--n-500)',
  borderRadius: 2, lineHeight: 1
};

function Select({ value, onChange, children, ...rest }) {
  return (
    <select className="select-native" value={value} onChange={e => onChange(e.target.value)} {...rest}>
      {children}
    </select>
  );
}

function Slider({ value, onChange, min = 0, max = 100, step = 1 }) {
  return (
    <input className="hc-slider" type="range" min={min} max={max} step={step}
           value={value} onChange={e => onChange(Number(e.target.value))} />
  );
}

function Toggle({ checked, onChange }) {
  return (
    <label className="hc-toggle">
      <input type="checkbox" checked={checked} onChange={e => onChange(e.target.checked)} />
      <span className="hc-toggle-track"></span>
    </label>
  );
}

function Checkbox({ checked, onChange, label }) {
  return (
    <label className="hc-check">
      <input type="checkbox" checked={checked} onChange={e => onChange(e.target.checked)} />
      <span>{label}</span>
    </label>
  );
}

function Tabs({ options, value, onChange, variant = 'underline' }) {
  return (
    <div className={`hc-tabs ${variant === 'pill' ? 'hc-tabs--pill' : ''}`}>
      {options.map(o => (
        <button key={o.value}
                className={`hc-tab ${o.value === value ? 'active' : ''}`}
                onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

// -------------------- Badges --------------------
function Badge({ variant = 'default', children, dot = true }) {
  return (
    <span className={`hc-badge ${variant !== 'default' ? 'hc-badge--' + variant : ''}`}>
      {dot && <span className="dot" />}
      {children}
    </span>
  );
}

// -------------------- StatBlock --------------------
function StatBlock({ label, value, unit, delta, boxed }) {
  return (
    <div className={`statblock ${boxed ? 'statblock--boxed' : ''}`}>
      <span className="sb-label">{label}</span>
      <span className="sb-value num">
        {value}{unit && <span className="sb-unit">{unit}</span>}
      </span>
      {delta !== undefined && (
        <span className={`sb-delta sb-delta--${delta >= 0 ? 'up' : 'down'}`}>
          {delta >= 0 ? '↑' : '↓'} {Math.abs(delta)}%
        </span>
      )}
    </div>
  );
}

// -------------------- Address --------------------
function AddressDisplay({ addr = '7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU', len = 4 }) {
  const [copied, setCopied] = useState(false);
  const truncated = `${addr.slice(0, len)}…${addr.slice(-len)}`;
  return (
    <span className="hc-addr" onClick={() => {
      navigator.clipboard?.writeText(addr);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    }} title={addr}>
      {truncated}
      <span className="copy-hint">{copied ? '✓' : '⧉'}</span>
    </span>
  );
}

// -------------------- Network Indicator --------------------
function NetworkIndicator({ network = 'mainnet-beta' }) {
  const label = { 'mainnet-beta': 'Mainnet', 'devnet': 'Devnet', 'unknown': 'Unknown' }[network];
  const cls = network === 'devnet' ? 'devnet' : network === 'unknown' ? 'unknown' : '';
  return (
    <span className={`net-ind ${cls}`}>
      <span className="dot" /> {label}
    </span>
  );
}

// -------------------- Wallet Button --------------------
function WalletConnectButton({ state = 'connected', address = '7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU', onConnect }) {
  if (state === 'disconnected') {
    return (
      <button className="wallet-btn wallet-btn--disconnected" onClick={onConnect}>
        Connect wallet
      </button>
    );
  }
  if (state === 'connecting') {
    return <button className="wallet-btn" disabled>Connecting…</button>;
  }
  return (
    <button className="wallet-btn">
      <span className="avatar" />
      <span className="addr">{address.slice(0, 4)}…{address.slice(-4)}</span>
    </button>
  );
}

// -------------------- Alert --------------------
function Alert({ variant = 'info', title, children, icon }) {
  const defaults = { success: '✓', info: 'i', warning: '!', error: '×' };
  return (
    <div className={`hc-alert hc-alert--${variant}`}>
      <span className="hc-alert-icon">{icon ?? defaults[variant]}</span>
      <div className="hc-alert-body">
        {title && <strong>{title}</strong>}
        <span>{children}</span>
      </div>
    </div>
  );
}

// -------------------- Empty State --------------------
function EmptyState({ headline = "Nothing to see on the water.", hint, action }) {
  return (
    <div className="empty">
      <King size={64} color="var(--n-300)" />
      <p className="empty-headline serif-italic">{headline}</p>
      {hint && <p className="empty-hint">{hint}</p>}
      {action}
    </div>
  );
}

// -------------------- Skeleton --------------------
function Skeleton({ w = '100%', h = 12, style }) {
  return <span className="hc-skel" style={{ display: 'inline-block', width: w, height: h, ...style }} />;
}

// -------------------- Modal --------------------
function Modal({ open, onClose, title, children, footer }) {
  useEffect(() => {
    if (!open) return;
    const esc = e => e.key === 'Escape' && onClose?.();
    window.addEventListener('keydown', esc);
    return () => window.removeEventListener('keydown', esc);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="hc-modal-backdrop" onClick={onClose}>
      <div className="hc-modal" onClick={e => e.stopPropagation()}>
        <div className="hc-modal-head">
          <h3>{title}</h3>
        </div>
        <div className="hc-modal-body">{children}</div>
        {footer && <div className="hc-modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

// -------------------- Toast --------------------
function Toast({ variant = 'success', title, children }) {
  return (
    <div className={`hc-alert hc-alert--${variant}`} style={{ boxShadow: 'var(--shadow-3)', background: '#fff' }}>
      <span className="hc-alert-icon">{ {success: '✓', info: 'i', warning: '!', error: '×'}[variant] }</span>
      <div className="hc-alert-body">
        {title && <strong>{title}</strong>}
        <span>{children}</span>
      </div>
    </div>
  );
}

Object.assign(window, {
  King, Button, Input, NumberStepper, Select, Slider, Toggle, Checkbox, Tabs,
  Badge, StatBlock, AddressDisplay, NetworkIndicator, WalletConnectButton,
  Alert, Toast, EmptyState, Skeleton, Modal
});

/* global React, HalcyonMath */
// Halcyon — charts (real SVG, rendered from actual math)

const HM = window.HalcyonMath;

function scale(domain, range) {
  const [d0, d1] = domain, [r0, r1] = range;
  return v => r0 + (v - d0) / (d1 - d0) * (r1 - r0);
}

function PayoffChart({
  curves = [],                   // [{ data: [{x,y}], color, label, dashed }]
  annotations = [],              // [{ x, y, label, color }]
  xDomain, yDomain,
  xLabel = 'Worst-of performance', yLabel = 'Payoff',
  xFormat = v => `${(v*100).toFixed(0)}%`,
  yFormat = v => `${(v*100).toFixed(0)}%`,
  width = 560, height = 280,
  padding = { t: 20, r: 24, b: 36, l: 54 }
}) {
  const all = curves.flatMap(c => c.data);
  const xD = xDomain || [Math.min(...all.map(p=>p.x)), Math.max(...all.map(p=>p.x))];
  const yD = yDomain || [Math.min(...all.map(p=>p.y)), Math.max(...all.map(p=>p.y))];
  const sx = scale(xD, [padding.l, width - padding.r]);
  const sy = scale(yD, [height - padding.b, padding.t]);

  const xTicks = 5, yTicks = 4;
  const xt = Array.from({length: xTicks+1}, (_,i) => xD[0] + (xD[1]-xD[0]) * i/xTicks);
  const yt = Array.from({length: yTicks+1}, (_,i) => yD[0] + (yD[1]-yD[0]) * i/yTicks);

  return (
    <svg className="hc-chart" viewBox={`0 0 ${width} ${height}`} width="100%">
      {/* gridlines */}
      {yt.map((t,i) => (
        <line key={i} x1={padding.l} x2={width-padding.r} y1={sy(t)} y2={sy(t)} className="grid" />
      ))}
      {/* axes */}
      <line x1={padding.l} y1={padding.t} x2={padding.l} y2={height-padding.b} className="axis" />
      <line x1={padding.l} y1={height-padding.b} x2={width-padding.r} y2={height-padding.b} className="axis" />
      {/* ticks */}
      {xt.map((t,i) => (
        <g key={i}>
          <line x1={sx(t)} x2={sx(t)} y1={height-padding.b} y2={height-padding.b+4} className="axis-tick" />
          <text x={sx(t)} y={height-padding.b+16} textAnchor="middle" className="tick-label">{xFormat(t)}</text>
        </g>
      ))}
      {yt.map((t,i) => (
        <g key={i}>
          <text x={padding.l-8} y={sy(t)+3} textAnchor="end" className="tick-label">{yFormat(t)}</text>
        </g>
      ))}
      {/* axis labels */}
      <text x={(padding.l + width - padding.r)/2} y={height-6} textAnchor="middle"
            style={{fontSize: 10, fill: 'var(--n-500)', letterSpacing: '0.08em', textTransform: 'uppercase', fontWeight: 600}}>
        {xLabel}
      </text>
      {/* annotations (under curves) */}
      {annotations.map((a, i) => (
        <g key={i}>
          <line x1={sx(a.x)} x2={sx(a.x)} y1={padding.t} y2={height-padding.b}
                className="annotation-line" stroke={a.color || 'var(--n-300)'} />
          <text x={sx(a.x)+4} y={padding.t+12} className="ann-label"
                fill={a.color || 'var(--n-500)'}>{a.label}</text>
        </g>
      ))}
      {/* curves */}
      {curves.map((c, ci) => {
        const d = c.data.map((p, i) =>
          `${i===0?'M':'L'}${sx(p.x).toFixed(2)} ${sy(p.y).toFixed(2)}`
        ).join(' ');
        return (
          <path key={ci} d={d} className="series-line"
                stroke={c.color || 'var(--blue-500)'}
                strokeDasharray={c.dashed ? '4 3' : '0'} />
        );
      })}
    </svg>
  );
}

function HistogramChart({
  bins, annotations = [], width = 560, height = 240,
  xLabel = 'Max drawdown', xFormat = v => `${(v*100).toFixed(0)}%`,
  color = 'var(--blue-500)',
  padding = { t: 20, r: 24, b: 36, l: 48 }
}) {
  if (!bins || bins.length === 0) return null;
  const xD = [0, Math.max(...bins.map(b=>b.x))];
  const yD = [0, Math.max(...bins.map(b=>b.y)) * 1.15];
  const sx = scale(xD, [padding.l, width - padding.r]);
  const sy = scale(yD, [height - padding.b, padding.t]);
  const barW = (width - padding.l - padding.r) / bins.length - 1;
  const xt = [0, 0.25, 0.5, 0.75, 1].map(q => xD[0] + (xD[1]-xD[0])*q);
  return (
    <svg className="hc-chart" viewBox={`0 0 ${width} ${height}`} width="100%">
      <line x1={padding.l} y1={height-padding.b} x2={width-padding.r} y2={height-padding.b} className="axis" />
      {xt.map((t,i) => (
        <g key={i}>
          <line x1={sx(t)} x2={sx(t)} y1={height-padding.b} y2={height-padding.b+4} className="axis-tick" />
          <text x={sx(t)} y={height-padding.b+16} textAnchor="middle" className="tick-label">{xFormat(t)}</text>
        </g>
      ))}
      {bins.map((b, i) => {
        const x = sx(b.x) - barW/2;
        const y = sy(b.y);
        const h = height - padding.b - y;
        // color ramp based on x position
        const t = b.x / xD[1];
        const fill = `color-mix(in oklch, var(--blue-500) ${100 - t*100}%, var(--rust-500) ${t*100}%)`;
        return <rect key={i} x={x} y={y} width={barW} height={h} fill={fill} opacity="0.85" />;
      })}
      {annotations.map((a, i) => (
        <g key={i}>
          <line x1={sx(a.x)} x2={sx(a.x)} y1={padding.t} y2={height-padding.b}
                className="annotation-line" stroke={a.color || 'var(--ink)'} />
          <text x={sx(a.x)+4} y={padding.t+12} className="ann-label"
                fill={a.color || 'var(--ink)'}>{a.label}</text>
        </g>
      ))}
      <text x={(padding.l + width - padding.r)/2} y={height-6} textAnchor="middle"
            style={{fontSize: 10, fill: 'var(--n-500)', letterSpacing: '0.08em', textTransform: 'uppercase', fontWeight: 600}}>
        {xLabel}
      </text>
    </svg>
  );
}

function PriceChart({
  data, width = 560, height = 200, color = 'var(--blue-600)',
  padding = { t: 14, r: 12, b: 24, l: 44 }
}) {
  if (!data || !data.length) return null;
  const xD = [data[0].t, data[data.length-1].t];
  const yD = [Math.min(...data.map(d=>d.v))*0.98, Math.max(...data.map(d=>d.v))*1.02];
  const sx = scale(xD, [padding.l, width - padding.r]);
  const sy = scale(yD, [height - padding.b, padding.t]);
  const d = data.map((p,i) => `${i===0?'M':'L'}${sx(p.t).toFixed(1)} ${sy(p.v).toFixed(1)}`).join(' ');
  const dArea = d + ` L ${sx(xD[1])} ${height-padding.b} L ${sx(xD[0])} ${height-padding.b} Z`;
  const last = data[data.length-1];
  const first = data[0];
  const chg = (last.v/first.v - 1) * 100;

  return (
    <svg className="hc-chart" viewBox={`0 0 ${width} ${height}`} width="100%">
      <defs>
        <linearGradient id="priceFill" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.12" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      {[0.25, 0.5, 0.75].map((q, i) => {
        const y = padding.t + (height - padding.t - padding.b) * q;
        return <line key={i} x1={padding.l} x2={width-padding.r} y1={y} y2={y} className="grid" />;
      })}
      <path d={dArea} fill="url(#priceFill)" />
      <path d={d} stroke={color} strokeWidth="1.5" fill="none" />
      <circle cx={sx(last.t)} cy={sy(last.v)} r="3" fill={color} />
      <text x={width-padding.r} y={padding.t+2} textAnchor="end"
            style={{fontSize: 11, fill: chg >= 0 ? 'var(--success-700)' : 'var(--error-700)', fontFamily: 'var(--f-mono)'}}>
        {chg >= 0 ? '+' : ''}{chg.toFixed(2)}%
      </text>
    </svg>
  );
}

Object.assign(window, { PayoffChart, HistogramChart, PriceChart });

/**
 * Halcyon nav icons — ported from `app/app_shell.jsx` `NavIcon`.
 *
 * 16×16 grid, 1.5-stroke, round caps & joins, `currentColor`.
 * Deliberately hand-drawn; the small idiosyncrasies (the chart's
 * corner-arrow, the droplet's asymmetry, the vault's dial marks)
 * are part of the Halcyon visual vocabulary.
 */

type IconProps = {
  className?: string;
  size?: number;
};

function commonProps(size: number) {
  return {
    width: size,
    height: size,
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.5,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
}

/** Upward-trending chart with corner arrow. Equity Autocall. */
export function EquityIcon({ className, size = 16 }: IconProps) {
  return (
    <svg {...commonProps(size)} className={className} aria-hidden="true">
      <path d="M2 12l3-4 3 2 5-7" />
      <path d="M10 3h3v3" />
    </svg>
  );
}

/** Droplet silhouette. IL Protection. */
export function ILIcon({ className, size = 16 }: IconProps) {
  return (
    <svg {...commonProps(size)} className={className} aria-hidden="true">
      <path d="M8 2c3 4 4 6 4 8a4 4 0 01-8 0c0-2 1-4 4-8z" />
    </svg>
  );
}

/** Card with horizontal rule lines. SOL Autocall. */
export function SolIcon({ className, size = 16 }: IconProps) {
  return (
    <svg {...commonProps(size)} className={className} aria-hidden="true">
      <rect x="2" y="4" width="12" height="8" rx="1" />
      <path d="M4 8h8M4 6h6M4 10h6" />
    </svg>
  );
}

/** Vault / safe with dial and register marks. Vault page. */
export function VaultIcon({ className, size = 16 }: IconProps) {
  return (
    <svg {...commonProps(size)} className={className} aria-hidden="true">
      <rect x="2" y="3" width="12" height="10" rx="1" />
      <circle cx="8" cy="8" r="2" />
      <path d="M8 5v1M8 10v1M5 8h1M10 8h1" />
    </svg>
  );
}

/** Briefcase with handle and pocket. Portfolio. */
export function PortfolioIcon({ className, size = 16 }: IconProps) {
  return (
    <svg {...commonProps(size)} className={className} aria-hidden="true">
      <rect x="2" y="4" width="12" height="9" rx="1" />
      <path d="M6 4V2h4v2" />
      <path d="M2 8h12" />
    </svg>
  );
}

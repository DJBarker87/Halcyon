/**
 * Halcyon Kingfisher mark — ported from `app/kingfisher.js`.
 *
 * Original geometric silhouette: dagger beak, streamlined head,
 * swept-back wing, tail wedge. Body axis tilts ~25° down from
 * horizontal (the diving line). Eye is a negative-space circle
 * filled with `--paper` so it knocks through the mark when the
 * mark itself is coloured (e.g. blue on paper).
 *
 * `viewBox="0 0 24 24"`. Size in both CSS-pixel axes via `size`.
 */

const MARK_PATH =
  "M 22.2 7.4 L 13.8 8.5 L 11.4 8.2 " +
  "C 9.0 8.1, 6.9 9.0, 5.3 10.6 " +
  "L 8.2 10.7 L 5.8 12.2 L 3.6 11.7 L 2.4 13.6 " +
  "L 5.4 13.1 L 7.0 14.6 L 9.4 13.7 L 12.2 14.0 " +
  "C 14.0 13.6, 15.0 12.4, 15.0 10.8 " +
  "L 17.0 10.5 L 15.8 9.6 Z";

const EYE = { cx: 13.4, cy: 9.6, r: 0.42 };

interface KingfisherProps {
  size?: number;
  color?: string;
  eye?: boolean;
  title?: string;
  className?: string;
}

export function Kingfisher({
  size = 24,
  color = "currentColor",
  eye = true,
  title = "Halcyon",
  className,
}: KingfisherProps) {
  const showEye = eye && size >= 16;
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      role="img"
      aria-label={title}
      className={className}
      style={{ display: "inline-block", verticalAlign: "middle", flex: "none" }}
    >
      <g fill={color}>
        <path d={MARK_PATH} />
        {showEye && (
          <circle cx={EYE.cx} cy={EYE.cy} r={EYE.r} fill="var(--paper, #FAFAF7)" />
        )}
      </g>
    </svg>
  );
}

/**
 * Editorial variant — larger mark with a diagonal "dive line" beneath.
 * Used in landing hero / empty states, not in chrome.
 */
export function KingfisherEditorial({
  size = 240,
  color = "currentColor",
  className,
}: {
  size?: number;
  color?: string;
  className?: string;
}) {
  return (
    <svg
      viewBox="0 0 240 240"
      width={size}
      height={size}
      role="img"
      aria-label="Halcyon editorial"
      className={className}
    >
      <g transform="translate(48, 28) scale(6)" fill={color}>
        <path d={MARK_PATH} />
        <circle cx={EYE.cx} cy={EYE.cy} r={EYE.r} fill="var(--paper, #FAFAF7)" />
      </g>
      <line x1="60" y1="210" x2="200" y2="210" stroke={color} strokeWidth="1" opacity="0.25" />
      <line x1="90" y1="218" x2="170" y2="218" stroke={color} strokeWidth="1" opacity="0.15" />
    </svg>
  );
}

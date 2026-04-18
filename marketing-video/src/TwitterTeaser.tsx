import React from "react";
import {
  AbsoluteFill,
  Sequence,
  useCurrentFrame,
  useVideoConfig,
  interpolate,
  spring,
  Easing,
} from "remotion";

const BG = "#050507";
const RED = "#ff2020";
const GREEN = "#00ff88";
const WHITE = "#ffffff";
const DIM = "rgba(255,255,255,0.3)";
const LINE_GREEN = "#00cc66";
const LINE_RED = "#ff3333";

// ─── EKG Line ────────────────────────────────────────
// Draws a moving heartbeat waveform using canvas-style SVG
const EKGLine: React.FC<{
  color: string;
  speed?: number; // pixels per frame the line advances
  chaos?: number; // 0 = calm, 1 = panic
  flat?: boolean;
  glowColor?: string;
  width?: number;
  height?: number;
}> = ({
  color,
  speed = 12,
  chaos = 0,
  flat = false,
  glowColor,
  width = 1920,
  height = 300,
}) => {
  const frame = useCurrentFrame();

  // generate the waveform path
  const points: string[] = [];
  const mid = height / 2;
  const totalWidth = width + 200;
  const offset = frame * speed;

  for (let x = 0; x < totalWidth; x += 2) {
    const worldX = x + offset;
    let y = mid;

    if (flat) {
      y = mid;
    } else if (chaos > 0.5) {
      // Panic mode: erratic spikes
      const freq = 0.08 + chaos * 0.15;
      const amp = 40 + chaos * 80;
      const noise = Math.sin(worldX * freq) * amp;
      const spike =
        Math.sin(worldX * 0.3) > 0.7
          ? Math.sin(worldX * 0.6) * 120 * chaos
          : 0;
      const jitter = Math.sin(worldX * 1.2 + frame * 0.5) * 15 * chaos;
      y = mid + noise + spike + jitter;
    } else {
      // Normal heartbeat pattern repeating every ~200px
      const cycle = worldX % 200;
      if (cycle > 80 && cycle < 90) {
        y = mid - 8; // small P wave
      } else if (cycle > 95 && cycle < 100) {
        y = mid + 30; // Q dip
      } else if (cycle > 100 && cycle < 108) {
        y = mid - 100 + chaos * 20; // R spike (the big one)
      } else if (cycle > 108 && cycle < 115) {
        y = mid + 25; // S dip
      } else if (cycle > 130 && cycle < 145) {
        y = mid - 15; // T wave
      }
    }

    // clamp
    y = Math.max(10, Math.min(height - 10, y));
    points.push(`${x},${y}`);
  }

  // clip to reveal only what's been "drawn"
  const revealX = Math.min(totalWidth, frame * speed);

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      style={{ position: "absolute", left: 0 }}
    >
      {/* glow */}
      {glowColor && (
        <polyline
          points={points.join(" ")}
          fill="none"
          stroke={glowColor}
          strokeWidth={12}
          opacity={0.3}
          strokeLinecap="round"
          strokeLinejoin="round"
          style={{
            filter: "blur(8px)",
            clipPath: `inset(0 ${Math.max(0, width - revealX)}px 0 0)`,
          }}
        />
      )}
      {/* main line */}
      <polyline
        points={points.join(" ")}
        fill="none"
        stroke={color}
        strokeWidth={4}
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{
          clipPath: `inset(0 ${Math.max(0, width - revealX)}px 0 0)`,
        }}
      />
      {/* scanning dot */}
      {!flat && revealX < width && (
        <circle
          cx={Math.min(revealX, width - 20)}
          cy={
            points[Math.min(Math.floor(revealX / 2), points.length - 1)]
              ? Number(
                  points[
                    Math.min(Math.floor(revealX / 2), points.length - 1)
                  ].split(",")[1]
                )
              : mid
          }
          r={6}
          fill={color}
          opacity={0.9}
        >
          <animate
            attributeName="r"
            values="4;8;4"
            dur="0.6s"
            repeatCount="indefinite"
          />
        </circle>
      )}
    </svg>
  );
};

// ─── Grid overlay for medical feel ──────────────────
const GridOverlay: React.FC<{ opacity?: number }> = ({ opacity = 0.04 }) => (
  <AbsoluteFill style={{ opacity, pointerEvents: "none" }}>
    <svg width="100%" height="100%">
      <defs>
        <pattern id="grid" width="40" height="40" patternUnits="userSpaceOnUse">
          <path
            d="M 40 0 L 0 0 0 40"
            fill="none"
            stroke="white"
            strokeWidth="0.5"
          />
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill="url(#grid)" />
    </svg>
  </AbsoluteFill>
);

// ─── Scene 1: CALM — steady heartbeat ───────────────
const SceneCalm: React.FC = () => {
  const frame = useCurrentFrame();

  const labelOpacity = interpolate(frame, [10, 20], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      <GridOverlay />

      {/* EKG positioned center */}
      <div
        style={{
          position: "absolute",
          top: "50%",
          left: 0,
          width: "100%",
          transform: "translateY(-50%)",
        }}
      >
        <EKGLine color={LINE_GREEN} glowColor={GREEN} speed={10} chaos={0} />
      </div>

      {/* Label */}
      <div
        style={{
          position: "absolute",
          top: 80,
          left: 100,
          opacity: labelOpacity,
        }}
      >
        <div
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 28,
            color: DIM,
            textTransform: "uppercase",
            letterSpacing: "0.2em",
          }}
        >
          LP Position
        </div>
        <div
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 22,
            color: GREEN,
            marginTop: 8,
            opacity: 0.6,
          }}
        >
          SOL/USDC
        </div>
      </div>

      {/* Vital signs */}
      <div
        style={{
          position: "absolute",
          top: 80,
          right: 100,
          textAlign: "right",
          opacity: labelOpacity,
        }}
      >
        <div
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 64,
            fontWeight: 900,
            color: GREEN,
            lineHeight: 1,
          }}
        >
          STABLE
        </div>
      </div>
    </AbsoluteFill>
  );
};

// ─── Scene 2: CRASH — EKG goes haywire ──────────────
const SceneCrash: React.FC = () => {
  const frame = useCurrentFrame();

  // chaos ramps up
  const chaos = interpolate(frame, [0, 15, 50], [0, 0.8, 1], {
    extrapolateRight: "clamp",
    easing: Easing.in(Easing.cubic),
  });

  // screen shake
  const shakeX = frame < 50 ? Math.sin(frame * 8) * chaos * 12 : 0;
  const shakeY = frame < 50 ? Math.cos(frame * 6) * chaos * 8 : 0;

  // red flash at start
  const flash = interpolate(frame, [0, 6], [0.5, 0], {
    extrapolateRight: "clamp",
  });

  // SOL crash number
  const solDrop = interpolate(frame, [5, 25], [0, -30], {
    extrapolateRight: "clamp",
    easing: Easing.out(Easing.cubic),
  });
  const numOpacity = interpolate(frame, [5, 10], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // status text
  const statusOpacity = interpolate(frame, [10, 18], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const statusBlink = Math.sin(frame * 0.6) > 0 ? 1 : 0.3;

  return (
    <AbsoluteFill
      style={{
        backgroundColor: BG,
        transform: `translate(${shakeX}px, ${shakeY}px)`,
      }}
    >
      <GridOverlay opacity={0.06} />
      <AbsoluteFill style={{ backgroundColor: RED, opacity: flash }} />

      {/* red vignette */}
      <AbsoluteFill
        style={{
          background: `radial-gradient(ellipse at center, transparent 40%, ${RED}30 100%)`,
          opacity: chaos,
        }}
      />

      {/* EKG */}
      <div
        style={{
          position: "absolute",
          top: "50%",
          left: 0,
          width: "100%",
          transform: "translateY(-50%)",
        }}
      >
        <EKGLine
          color={LINE_RED}
          glowColor={RED}
          speed={18}
          chaos={chaos}
        />
      </div>

      {/* SOL crash number */}
      <div
        style={{
          position: "absolute",
          top: "15%",
          width: "100%",
          textAlign: "center",
          opacity: numOpacity,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 160,
            fontWeight: 900,
            color: RED,
            letterSpacing: "-0.04em",
          }}
        >
          SOL {Math.round(solDrop)}%
        </span>
      </div>

      {/* CRITICAL status */}
      <div
        style={{
          position: "absolute",
          bottom: 120,
          width: "100%",
          textAlign: "center",
          opacity: statusOpacity * statusBlink,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 36,
            fontWeight: 700,
            color: RED,
            textTransform: "uppercase",
            letterSpacing: "0.3em",
          }}
        >
          impermanent loss detected
        </span>
      </div>
    </AbsoluteFill>
  );
};

// ─── Scene 3: FLATLINE ──────────────────────────────
const SceneFlatline: React.FC = () => {
  const frame = useCurrentFrame();

  // line fades in flat
  const lineOpacity = interpolate(frame, [0, 8], [0.3, 0.8], {
    extrapolateRight: "clamp",
  });

  // everything dims
  const dimness = interpolate(frame, [0, 20], [1, 0.4], {
    extrapolateRight: "clamp",
  });

  return (
    <AbsoluteFill style={{ backgroundColor: BG, opacity: dimness }}>
      <GridOverlay opacity={0.03} />

      {/* flat red line */}
      <div
        style={{
          position: "absolute",
          top: "50%",
          left: 0,
          width: "100%",
          height: 4,
          backgroundColor: RED,
          opacity: lineOpacity,
          boxShadow: `0 0 20px ${RED}60`,
        }}
      />

      {/* UNPROTECTED */}
      <div
        style={{
          position: "absolute",
          top: "50%",
          width: "100%",
          textAlign: "center",
          transform: "translateY(-80px)",
          opacity: interpolate(frame, [8, 15], [0, 1], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
          }),
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 48,
            fontWeight: 300,
            color: "rgba(255,255,255,0.25)",
            textTransform: "lowercase",
            letterSpacing: "0.15em",
          }}
        >
          unprotected
        </span>
      </div>
    </AbsoluteFill>
  );
};

// ─── Scene 4: RESTART — protected heartbeat ─────────
const SceneRestart: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  // green flash at start
  const flash = interpolate(frame, [0, 8], [0.6, 0], {
    extrapolateRight: "clamp",
  });

  // EKG fades in
  const ekgOpacity = interpolate(frame, [3, 12], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // shield zone
  const shieldOpacity = interpolate(frame, [15, 30], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // coverage text
  const coverageOpacity = interpolate(frame, [35, 45], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // "you're covered" slam
  const slamP = spring({
    frame: frame - 20,
    fps,
    config: { damping: 16, stiffness: 180, mass: 0.5 },
  });
  const slamScale = interpolate(slamP, [0, 1], [2.5, 1], {
    extrapolateRight: "clamp",
  });
  const slamOpacity = interpolate(slamP, [0, 0.2], [0, 1], {
    extrapolateRight: "clamp",
  });

  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      <GridOverlay />
      <AbsoluteFill style={{ backgroundColor: GREEN, opacity: flash }} />

      {/* green shield zone — the 14-38% band */}
      <div
        style={{
          position: "absolute",
          top: "30%",
          left: "15%",
          right: "15%",
          bottom: "30%",
          border: `2px solid ${GREEN}40`,
          borderRadius: 12,
          background: `linear-gradient(180deg, ${GREEN}08 0%, ${GREEN}03 100%)`,
          opacity: shieldOpacity,
        }}
      >
        {/* left marker */}
        <div
          style={{
            position: "absolute",
            left: -1,
            top: -40,
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 28,
            color: GREEN,
            fontWeight: 700,
            opacity: coverageOpacity,
          }}
        >
          14%
        </div>
        {/* right marker */}
        <div
          style={{
            position: "absolute",
            right: -1,
            top: -40,
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 28,
            color: GREEN,
            fontWeight: 700,
            opacity: coverageOpacity,
          }}
        >
          38%
        </div>
        {/* bottom label */}
        <div
          style={{
            position: "absolute",
            bottom: -44,
            left: 0,
            right: 0,
            textAlign: "center",
            fontFamily: "'Inter', sans-serif",
            fontSize: 22,
            color: DIM,
            textTransform: "lowercase",
            letterSpacing: "0.05em",
            opacity: coverageOpacity,
          }}
        >
          sol move range — IL covered
        </div>
      </div>

      {/* EKG */}
      <div
        style={{
          position: "absolute",
          top: "50%",
          left: 0,
          width: "100%",
          transform: "translateY(-50%)",
          opacity: ekgOpacity,
        }}
      >
        <EKGLine color={LINE_GREEN} glowColor={GREEN} speed={10} chaos={0} />
      </div>

      {/* "YOU'RE COVERED" */}
      <div
        style={{
          position: "absolute",
          top: "12%",
          width: "100%",
          textAlign: "center",
          opacity: slamOpacity,
          transform: `scale(${slamScale})`,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 80,
            fontWeight: 900,
            color: GREEN,
            textTransform: "lowercase",
            letterSpacing: "-0.03em",
          }}
        >
          you're covered.
        </span>
      </div>
    </AbsoluteFill>
  );
};

// ─── Scene 5: CTA ───────────────────────────────────
const SceneCTA: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const p = spring({ frame, fps, config: { damping: 200 } });
  const opacity = interpolate(p, [0, 1], [0, 1], { extrapolateRight: "clamp" });

  // subtle heartbeat pulse on the logo
  const beat = Math.sin(frame * 0.25) * 0.03 + 1;

  return (
    <AbsoluteFill
      style={{
        backgroundColor: BG,
        justifyContent: "center",
        alignItems: "center",
      }}
    >
      <GridOverlay opacity={0.02} />

      {/* green glow */}
      <div
        style={{
          position: "absolute",
          width: 600,
          height: 600,
          borderRadius: "50%",
          background: `radial-gradient(circle, ${GREEN}10 0%, transparent 60%)`,
          top: "50%",
          left: "50%",
          transform: "translate(-50%, -50%)",
        }}
      />

      {/* small EKG behind the text */}
      <div
        style={{
          position: "absolute",
          top: "50%",
          left: 0,
          width: "100%",
          transform: "translateY(-50%)",
          opacity: 0.08,
        }}
      >
        <EKGLine color={GREEN} speed={8} chaos={0} height={200} />
      </div>

      <div style={{ textAlign: "center", opacity }}>
        <div
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 120,
            fontWeight: 900,
            color: WHITE,
            textTransform: "lowercase",
            letterSpacing: "-0.05em",
            lineHeight: 1,
            transform: `scale(${beat})`,
          }}
        >
          halcyon
        </div>
        <div
          style={{
            fontFamily: "'Inter', sans-serif",
            fontSize: 32,
            fontWeight: 300,
            color: DIM,
            marginTop: 24,
            textTransform: "lowercase",
            letterSpacing: "0.1em",
            opacity: interpolate(frame, [12, 22], [0, 1], {
              extrapolateLeft: "clamp",
              extrapolateRight: "clamp",
            }),
          }}
        >
          insure your LP. on-chain.
        </div>
      </div>
    </AbsoluteFill>
  );
};

// ─── Main: 15 seconds ───────────────────────────────
export const TwitterTeaser: React.FC = () => {
  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      {/* Scene 1: Calm heartbeat — 0-2.5s */}
      <Sequence from={0} durationInFrames={75}>
        <SceneCalm />
      </Sequence>

      {/* Scene 2: SOL crashes, EKG goes haywire — 2.5-5s */}
      <Sequence from={75} durationInFrames={75}>
        <SceneCrash />
      </Sequence>

      {/* Scene 3: Flatline — 5-6.5s */}
      <Sequence from={150} durationInFrames={45}>
        <SceneFlatline />
      </Sequence>

      {/* Scene 4: Restart, protected — 6.5-11s */}
      <Sequence from={195} durationInFrames={135}>
        <SceneRestart />
      </Sequence>

      {/* Scene 5: CTA — 11-15s */}
      <Sequence from={330} durationInFrames={120}>
        <SceneCTA />
      </Sequence>
    </AbsoluteFill>
  );
};

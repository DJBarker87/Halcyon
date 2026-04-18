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
const DIM = "rgba(255,255,255,0.25)";
const GOLD = "#ffaa00";

// ─── Utilities ───────────────────────────────────────

const slam = (
  frame: number,
  fps: number,
  delay: number = 0,
): { scale: number; opacity: number; raw: number } => {
  const p = spring({
    frame: frame - delay,
    fps,
    config: { damping: 14, stiffness: 350, mass: 0.3 },
  });
  return {
    scale: interpolate(p, [0, 1], [2.8, 1], { extrapolateRight: "clamp" }),
    opacity: interpolate(p, [0, 0.1], [0, 1], { extrapolateRight: "clamp" }),
    raw: p,
  };
};

const fadeIn = (frame: number, start: number, dur: number = 6): number =>
  interpolate(frame, [start, start + dur], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

const shake = (
  frame: number,
  intensity: number,
): { x: number; y: number } => ({
  x: Math.sin(frame * 13) * intensity,
  y: Math.cos(frame * 9) * intensity * 0.7,
});

// ─── Animated grid background ────────────────────────
const AnimGrid: React.FC<{ color?: string; opacity?: number; speed?: number }> = ({
  color = "white",
  opacity = 0.03,
  speed = 0.5,
}) => {
  const frame = useCurrentFrame();
  const offset = frame * speed;

  return (
    <AbsoluteFill style={{ opacity, pointerEvents: "none" }}>
      <svg width="100%" height="100%">
        <defs>
          <pattern
            id={`agrid-${color}`}
            width="60"
            height="60"
            patternUnits="userSpaceOnUse"
            patternTransform={`translate(${offset % 60}, ${offset % 60})`}
          >
            <path d="M 60 0 L 0 0 0 60" fill="none" stroke={color} strokeWidth="0.5" />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill={`url(#agrid-${color})`} />
      </svg>
    </AbsoluteFill>
  );
};

// ─── Pulse ring effect ───────────────────────────────
const PulseRings: React.FC<{ color: string; count?: number; delay?: number }> = ({
  color,
  count = 3,
  delay = 0,
}) => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{ pointerEvents: "none" }}>
      {Array.from({ length: count }).map((_, i) => {
        const age = frame - delay - i * 15;
        if (age < 0) return null;
        const progress = Math.min(age / 60, 1);
        const size = 100 + progress * 800;
        const op = (1 - progress) * 0.3;
        return (
          <div
            key={i}
            style={{
              position: "absolute",
              top: "50%",
              left: "50%",
              width: size,
              height: size,
              borderRadius: "50%",
              border: `2px solid ${color}`,
              opacity: op,
              transform: "translate(-50%, -50%)",
            }}
          />
        );
      })}
    </AbsoluteFill>
  );
};

// ─── Glitch bar effect ───────────────────────────────
const GlitchBars: React.FC<{ intensity?: number }> = ({ intensity = 1 }) => {
  const frame = useCurrentFrame();
  // deterministic pseudo-random bars
  const bars = Array.from({ length: 6 }).map((_, i) => {
    const seed = (frame * 7 + i * 137) % 100;
    const y = (seed / 100) * 1080;
    const h = 2 + (seed % 8);
    const w = 400 + (seed % 600);
    const x = (seed * 3) % 1920;
    const show = seed % 4 === 0;
    return show ? (
      <div
        key={i}
        style={{
          position: "absolute",
          top: y,
          left: x,
          width: w * intensity,
          height: h,
          backgroundColor: `rgba(255,32,32,${0.15 + (seed % 30) / 100})`,
          mixBlendMode: "screen",
        }}
      />
    ) : null;
  });

  return <AbsoluteFill style={{ pointerEvents: "none" }}>{bars}</AbsoluteFill>;
};

// ─── Horizontal rule animation ───────────────────────
const AnimatedRule: React.FC<{
  color: string;
  y: string;
  frame: number;
  delay: number;
}> = ({ color, y, frame, delay }) => {
  const width = interpolate(frame, [delay, delay + 10], [0, 100], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: Easing.out(Easing.cubic),
  });
  return (
    <div
      style={{
        position: "absolute",
        top: y,
        left: "50%",
        transform: "translateX(-50%)",
        width: `${width}%`,
        height: 1,
        background: `linear-gradient(90deg, transparent, ${color}, transparent)`,
        opacity: 0.3,
      }}
    />
  );
};

// ─── Number counter ──────────────────────────────────
const Counter: React.FC<{
  target: number;
  prefix?: string;
  suffix?: string;
  frame: number;
  delay: number;
  duration?: number;
  style: React.CSSProperties;
}> = ({ target, prefix = "", suffix = "", frame, delay, duration = 20, style }) => {
  const progress = interpolate(frame, [delay, delay + duration], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: Easing.out(Easing.cubic),
  });
  const current = Math.round(target * progress);
  return <span style={style}>{prefix}{current}{suffix}</span>;
};

// ─── Scene 1: THE NUMBER ─────────────────────────────
const SceneNumber: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const sk = shake(frame, frame < 10 ? 6 : 0);

  // flash on entry
  const flash = interpolate(frame, [3, 8], [0.3, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  const numScale = spring({
    frame: frame - 3,
    fps,
    config: { damping: 12, stiffness: 400, mass: 0.3 },
  });
  const numS = interpolate(numScale, [0, 1], [4, 1], { extrapolateRight: "clamp" });
  const numO = interpolate(numScale, [0, 0.08], [0, 1], { extrapolateRight: "clamp" });

  const sub = fadeIn(frame, 20, 8);

  return (
    <AbsoluteFill
      style={{
        backgroundColor: BG,
        transform: `translate(${sk.x}px, ${sk.y}px)`,
      }}
    >
      <AnimGrid color={RED} opacity={0.04} speed={0.8} />
      <AbsoluteFill style={{ backgroundColor: GOLD, opacity: flash }} />

      {/* radial glow behind number */}
      <div
        style={{
          position: "absolute",
          top: "50%",
          left: "50%",
          width: 1200,
          height: 600,
          transform: "translate(-50%, -55%)",
          background: `radial-gradient(ellipse, ${GOLD}12 0%, transparent 70%)`,
        }}
      />

      <div
        style={{
          position: "absolute",
          top: "50%",
          width: "100%",
          textAlign: "center",
          transform: `translateY(-65%) scale(${numS})`,
          opacity: numO,
        }}
      >
        <Counter
          target={185}
          prefix="$"
          suffix="B"
          frame={frame}
          delay={3}
          duration={18}
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 160,
            fontWeight: 900,
            color: GOLD,
            letterSpacing: "-0.04em",
          }}
        />
      </div>

      <div
        style={{
          position: "absolute",
          top: "50%",
          width: "100%",
          textAlign: "center",
          transform: "translateY(30px)",
          opacity: sub,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 38,
            fontWeight: 400,
            color: "rgba(255,255,255,0.35)",
            letterSpacing: "0.06em",
          }}
        >
          priced on servers you can't see
        </span>
      </div>

      <PulseRings color={GOLD} delay={5} count={2} />
    </AbsoluteFill>
  );
};

// ─── Scene 2: ACCUSATIONS ────────────────────────────
const SceneAccusation: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const lines = [
    { text: "the bank prices the note.", delay: 0 },
    { text: "the bank trades the note.", delay: 7 },
    { text: "the bank settles the note.", delay: 14 },
  ];

  const reveal = fadeIn(frame, 24, 6);

  // red pulse per slam
  const pulseOp = lines.reduce((acc, l) => {
    const d = frame - l.delay;
    if (d >= 0 && d < 5)
      return Math.max(
        acc,
        interpolate(d, [0, 5], [0.2, 0], { extrapolateRight: "clamp" }),
      );
    return acc;
  }, 0);

  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      <AnimGrid color={RED} opacity={0.05} speed={1.2} />
      <AbsoluteFill style={{ backgroundColor: RED, opacity: pulseOp }} />
      <GlitchBars intensity={frame < 30 ? 1.2 : 0.3} />

      <div style={{ position: "absolute", top: "22%", left: 140, right: 140 }}>
        {lines.map((line, i) => {
          const s = slam(frame, fps, line.delay);
          const sk = shake(frame, frame > line.delay && frame < line.delay + 6 ? 5 : 0);
          return (
            <div
              key={i}
              style={{
                opacity: s.opacity,
                transform: `translateX(${sk.x}px) scale(${s.scale})`,
                transformOrigin: "left center",
                marginBottom: 20,
              }}
            >
              <span
                style={{
                  fontFamily: "'JetBrains Mono', monospace",
                  fontSize: 62,
                  fontWeight: 900,
                  color: RED,
                  letterSpacing: "-0.02em",
                }}
              >
                {line.text}
              </span>
            </div>
          );
        })}

        {/* the punchline in dim */}
        <div
          style={{
            marginTop: 40,
            opacity: reveal,
          }}
        >
          <span
            style={{
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: 42,
              fontWeight: 300,
              color: "rgba(255,255,255,0.3)",
            }}
          >
            you see a coupon and a term sheet.
          </span>
        </div>
      </div>

      <AnimatedRule color={RED} y="82%" frame={frame} delay={30} />
    </AbsoluteFill>
  );
};

// ─── Scene 3: BLACK BOX ──────────────────────────────
const SceneBlackBox: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const s1 = slam(frame, fps, 2);
  const s2 = fadeIn(frame, 14, 6);
  const sk = shake(frame, frame < 8 ? 8 : 0);

  // animated "black box" rectangle
  const boxScale = spring({
    frame: frame - 22,
    fps,
    config: { damping: 20, stiffness: 200 },
  });
  const boxS = interpolate(boxScale, [0, 1], [0, 1], { extrapolateRight: "clamp" });

  return (
    <AbsoluteFill
      style={{
        backgroundColor: BG,
        transform: `translate(${sk.x}px, ${sk.y}px)`,
      }}
    >
      <AnimGrid color={RED} opacity={0.03} />
      <GlitchBars intensity={0.8} />

      {/* red vignette */}
      <AbsoluteFill
        style={{
          background: `radial-gradient(ellipse at center, transparent 20%, ${RED}18 100%)`,
        }}
      />

      <div
        style={{
          position: "absolute",
          top: "25%",
          width: "100%",
          textAlign: "center",
        }}
      >
        <div
          style={{
            opacity: s1.opacity,
            transform: `scale(${s1.scale})`,
            marginBottom: 30,
          }}
        >
          <span
            style={{
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: 88,
              fontWeight: 900,
              color: RED,
            }}
          >
            hidden fees: 1-3%
          </span>
        </div>

        <div style={{ opacity: s2 }}>
          <span
            style={{
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: 36,
              fontWeight: 300,
              color: "rgba(255,255,255,0.3)",
            }}
          >
            the model is a
          </span>
        </div>
      </div>

      {/* actual black box visual */}
      <div
        style={{
          position: "absolute",
          top: "58%",
          left: "50%",
          transform: `translate(-50%, -50%) scale(${boxS})`,
          width: 500,
          height: 220,
          border: `3px solid ${RED}`,
          borderRadius: 8,
          display: "flex",
          justifyContent: "center",
          alignItems: "center",
          background: `linear-gradient(135deg, #0a0a0a, #111)`,
          boxShadow: `0 0 60px ${RED}20, inset 0 0 40px ${RED}08`,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 64,
            fontWeight: 900,
            color: RED,
            letterSpacing: "0.15em",
            opacity: interpolate(boxScale, [0.5, 1], [0, 1], {
              extrapolateLeft: "clamp",
              extrapolateRight: "clamp",
            }),
          }}
        >
          BLACK BOX
        </span>
      </div>
    </AbsoluteFill>
  );
};

// ─── Scene 4: GRAVEYARD ──────────────────────────────
const SceneGraveyard: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const headerFade = fadeIn(frame, 0, 6);

  const protocols = [
    { name: "Friktion", year: "2023", delay: 5 },
    { name: "Ribbon", year: "2024", delay: 10 },
    { name: "Cega", year: "2024", delay: 15 },
    { name: "Thetanuts", year: "$909K", delay: 20 },
  ];

  const bottomFade = fadeIn(frame, 28, 6);

  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      <AnimGrid opacity={0.02} />

      <div
        style={{
          position: "absolute",
          top: "15%",
          width: "100%",
          textAlign: "center",
          opacity: headerFade,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 52,
            fontWeight: 300,
            color: "rgba(255,255,255,0.2)",
            letterSpacing: "0.12em",
          }}
        >
          DeFi tried.
        </span>
      </div>

      <div
        style={{
          position: "absolute",
          top: "32%",
          width: "100%",
          display: "flex",
          justifyContent: "center",
          gap: 50,
        }}
      >
        {protocols.map((p, i) => {
          const s = slam(frame, fps, p.delay);
          // strikethrough animates
          const strikeW = interpolate(frame, [p.delay + 4, p.delay + 12], [0, 100], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
          });
          return (
            <div
              key={i}
              style={{
                opacity: s.opacity,
                transform: `scale(${s.scale})`,
                textAlign: "center",
                width: 200,
              }}
            >
              <div style={{ position: "relative", display: "inline-block" }}>
                <span
                  style={{
                    fontFamily: "'JetBrains Mono', monospace",
                    fontSize: 44,
                    fontWeight: 700,
                    color: "rgba(255,255,255,0.12)",
                  }}
                >
                  {p.name}
                </span>
                {/* animated strikethrough */}
                <div
                  style={{
                    position: "absolute",
                    top: "50%",
                    left: 0,
                    height: 3,
                    width: `${strikeW}%`,
                    backgroundColor: RED,
                    boxShadow: `0 0 8px ${RED}`,
                  }}
                />
              </div>
              <div
                style={{
                  fontFamily: "'JetBrains Mono', monospace",
                  fontSize: 20,
                  color: "rgba(255,32,32,0.5)",
                  marginTop: 10,
                  letterSpacing: "0.1em",
                }}
              >
                {p.year}
              </div>
            </div>
          );
        })}
      </div>

      <div
        style={{
          position: "absolute",
          top: "62%",
          width: "100%",
          textAlign: "center",
          opacity: bottomFade,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 38,
            fontWeight: 400,
            color: "rgba(255,255,255,0.3)",
          }}
        >
          they priced off-chain too.
        </span>
      </div>

      <AnimatedRule color="rgba(255,255,255,0.1)" y="78%" frame={frame} delay={35} />
    </AbsoluteFill>
  );
};

// ─── Scene 5: THE BREAK ──────────────────────────────
const SceneBreak: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const greenFlash = interpolate(frame, [10, 13, 22], [0, 0.8, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  const textSlam = slam(frame, fps, 12);
  const sk = shake(frame, frame > 10 && frame < 18 ? 6 : 0);

  return (
    <AbsoluteFill
      style={{
        backgroundColor: BG,
        transform: `translate(${sk.x}px, ${sk.y}px)`,
      }}
    >
      <AbsoluteFill style={{ backgroundColor: GREEN, opacity: greenFlash }} />
      <PulseRings color={GREEN} delay={11} count={3} />

      <div
        style={{
          position: "absolute",
          top: "50%",
          width: "100%",
          textAlign: "center",
          transform: `translateY(-50%) scale(${textSlam.scale})`,
          opacity: textSlam.opacity,
          padding: "0 100px",
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 68,
            fontWeight: 900,
            color: GREEN,
            letterSpacing: "-0.02em",
            textShadow: `0 0 40px ${GREEN}40`,
          }}
        >
          what if you could verify the math?
        </span>
      </div>
    </AbsoluteFill>
  );
};

// ─── Scene 6: THE PROOF ──────────────────────────────
const SceneProof: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const items = [
    { label: "pricing model", value: "on-chain", delay: 0 },
    { label: "settlement", value: "same code", delay: 8 },
    { label: "parameters", value: "readable state", delay: 16 },
    { label: "fees", value: "published constants", delay: 24 },
    { label: "copula pricer", value: "1,220,000 CU", delay: 32 },
  ];

  const bottomSlam = slam(frame, fps, 44);

  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      <AnimGrid color={GREEN} opacity={0.03} speed={0.3} />

      {/* ambient glow */}
      <AbsoluteFill
        style={{
          background: `radial-gradient(ellipse at 50% 80%, ${GREEN}08 0%, transparent 50%)`,
        }}
      />

      <div style={{ position: "absolute", top: "8%", left: 180, right: 180 }}>
        {items.map((item, i) => {
          const s = slam(frame, fps, item.delay);
          // check mark springs in separately
          const checkP = spring({
            frame: frame - item.delay - 6,
            fps,
            config: { damping: 10, stiffness: 300, mass: 0.3 },
          });
          const checkScale = interpolate(checkP, [0, 1], [3, 1], {
            extrapolateRight: "clamp",
          });
          const checkOp = interpolate(checkP, [0, 0.1], [0, 1], {
            extrapolateRight: "clamp",
          });

          // line draws in
          const lineW = interpolate(frame, [item.delay + 2, item.delay + 10], [0, 100], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
            easing: Easing.out(Easing.cubic),
          });

          return (
            <div
              key={i}
              style={{
                opacity: s.opacity,
                marginBottom: 16,
                paddingBottom: 16,
                position: "relative",
              }}
            >
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  transform: `translateX(${interpolate(s.raw, [0, 1], [-30, 0], { extrapolateRight: "clamp" })}px)`,
                }}
              >
                <span
                  style={{
                    fontFamily: "'JetBrains Mono', monospace",
                    fontSize: 36,
                    fontWeight: 400,
                    color: "rgba(255,255,255,0.4)",
                  }}
                >
                  {item.label}
                </span>
                <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
                  <span
                    style={{
                      fontFamily: "'JetBrains Mono', monospace",
                      fontSize: 36,
                      fontWeight: 900,
                      color: GREEN,
                      textShadow: `0 0 20px ${GREEN}30`,
                    }}
                  >
                    {item.value}
                  </span>
                  <span
                    style={{
                      fontSize: 30,
                      color: GREEN,
                      opacity: checkOp,
                      transform: `scale(${checkScale})`,
                      display: "inline-block",
                      filter: `drop-shadow(0 0 6px ${GREEN})`,
                    }}
                  >
                    ✓
                  </span>
                </div>
              </div>
              {/* animated divider line */}
              <div
                style={{
                  position: "absolute",
                  bottom: 0,
                  left: 0,
                  height: 1,
                  width: `${lineW}%`,
                  background: `linear-gradient(90deg, ${GREEN}30, ${GREEN}08)`,
                }}
              />
            </div>
          );
        })}
      </div>

      {/* one solana transaction */}
      <div
        style={{
          position: "absolute",
          bottom: "10%",
          width: "100%",
          textAlign: "center",
          opacity: bottomSlam.opacity,
          transform: `scale(${bottomSlam.scale})`,
        }}
      >
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 56,
            fontWeight: 900,
            color: WHITE,
            textShadow: `0 0 30px ${GREEN}25`,
          }}
        >
          one solana transaction.
        </span>
      </div>

      <PulseRings color={GREEN} delay={40} count={2} />
    </AbsoluteFill>
  );
};

// ─── Scene 7: THE PRODUCT ────────────────────────────
const SceneProduct: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const lines = [
    { text: "worst-of autocall.", color: WHITE, size: 76, weight: 900, delay: 0 },
    { text: "SPY / QQQ / IWM", color: GOLD, size: 56, weight: 700, delay: 7 },
    { text: "20 years backtested.", color: "rgba(255,255,255,0.35)", size: 44, weight: 400, delay: 14 },
    { text: "zero insolvency.", color: GREEN, size: 60, weight: 900, delay: 21 },
    { text: "$100 USDC. no advisor. no bank.", color: "rgba(255,255,255,0.3)", size: 40, weight: 400, delay: 30 },
  ];

  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      <AnimGrid color={GREEN} opacity={0.02} speed={0.2} />

      {/* gradient accent stripe */}
      <div
        style={{
          position: "absolute",
          left: 100,
          top: "15%",
          bottom: "15%",
          width: 4,
          background: `linear-gradient(180deg, ${GOLD}, ${GREEN})`,
          borderRadius: 2,
          opacity: fadeIn(frame, 5, 15),
        }}
      />

      <div style={{ position: "absolute", top: "14%", left: 140, right: 140 }}>
        {lines.map((line, i) => {
          const s = slam(frame, fps, line.delay);
          return (
            <div
              key={i}
              style={{
                opacity: s.opacity,
                transform: `translateX(${interpolate(s.raw, [0, 1], [-40, 0], { extrapolateRight: "clamp" })}px)`,
                marginBottom: 18,
              }}
            >
              <span
                style={{
                  fontFamily: "'JetBrains Mono', monospace",
                  fontSize: line.size,
                  fontWeight: line.weight,
                  color: line.color,
                  letterSpacing: "-0.02em",
                  textShadow:
                    line.color === GREEN
                      ? `0 0 30px ${GREEN}35`
                      : line.color === GOLD
                        ? `0 0 20px ${GOLD}20`
                        : "none",
                }}
              >
                {line.text}
              </span>
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};

// ─── Scene 8: CTA ────────────────────────────────────
const SceneCTA: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const nameSlam = slam(frame, fps, 3);
  const tagFade = fadeIn(frame, 22, 10);
  const pulse = Math.sin(frame * 0.2) * 0.012 + 1;

  const glowOp = interpolate(frame, [3, 50], [0, 0.15], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  // subtle green line at bottom
  const lineW = interpolate(frame, [30, 55], [0, 60], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: Easing.out(Easing.cubic),
  });

  return (
    <AbsoluteFill
      style={{
        backgroundColor: BG,
        justifyContent: "center",
        alignItems: "center",
      }}
    >
      <AnimGrid color={GREEN} opacity={0.02} speed={0.15} />

      {/* green glow */}
      <div
        style={{
          position: "absolute",
          width: 900,
          height: 900,
          borderRadius: "50%",
          background: `radial-gradient(circle, ${GREEN} 0%, transparent 55%)`,
          opacity: glowOp,
          top: "50%",
          left: "50%",
          transform: "translate(-50%, -50%)",
        }}
      />

      <PulseRings color={GREEN} delay={5} count={3} />

      <div style={{ textAlign: "center", zIndex: 1 }}>
        <div
          style={{
            opacity: nameSlam.opacity,
            transform: `scale(${nameSlam.scale * pulse})`,
          }}
        >
          <span
            style={{
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: 150,
              fontWeight: 900,
              color: WHITE,
              letterSpacing: "-0.05em",
              textShadow: `0 0 60px ${GREEN}20`,
            }}
          >
            halcyon
          </span>
        </div>

        {/* divider line */}
        <div
          style={{
            width: `${lineW}%`,
            height: 2,
            background: `linear-gradient(90deg, transparent, ${GREEN}, transparent)`,
            margin: "20px auto",
            opacity: 0.5,
          }}
        />

        <div style={{ opacity: tagFade }}>
          <span
            style={{
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: 42,
              fontWeight: 700,
              color: GREEN,
              letterSpacing: "0.18em",
              textShadow: `0 0 20px ${GREEN}30`,
            }}
          >
            NOT TRUST. VERIFICATION.
          </span>
        </div>
      </div>
    </AbsoluteFill>
  );
};

// ─── Main: ~22s @ 30fps = 660 frames ─────────────────
export const VerifiedPricing: React.FC = () => {
  return (
    <AbsoluteFill style={{ backgroundColor: BG }}>
      {/* Scene 1: $185B — 0-2s */}
      <Sequence from={0} durationInFrames={60}>
        <SceneNumber />
      </Sequence>

      {/* Scene 2: Accusations — 2-4.3s */}
      <Sequence from={60} durationInFrames={70}>
        <SceneAccusation />
      </Sequence>

      {/* Scene 3: Black box — 4.3-6.3s */}
      <Sequence from={130} durationInFrames={60}>
        <SceneBlackBox />
      </Sequence>

      {/* Scene 4: Graveyard — 6.3-8.3s */}
      <Sequence from={190} durationInFrames={60}>
        <SceneGraveyard />
      </Sequence>

      {/* Scene 5: The break — 8.3-9.8s */}
      <Sequence from={250} durationInFrames={45}>
        <SceneBreak />
      </Sequence>

      {/* Scene 6: The proof — 9.8-15s */}
      <Sequence from={295} durationInFrames={155}>
        <SceneProof />
      </Sequence>

      {/* Scene 7: The product — 15-19s */}
      <Sequence from={450} durationInFrames={120}>
        <SceneProduct />
      </Sequence>

      {/* Scene 8: CTA — 19-22s */}
      <Sequence from={570} durationInFrames={90}>
        <SceneCTA />
      </Sequence>
    </AbsoluteFill>
  );
};

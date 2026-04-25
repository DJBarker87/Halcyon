import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./app/**/*.{ts,tsx}",
    "./components/**/*.{ts,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        display: ["var(--font-display)", "system-ui", "sans-serif"],
        body: ["var(--font-body)", "system-ui", "sans-serif"],
        mono: ["ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
      },
      colors: {
        // Halcyon Blue — kingfisher (brand)
        halcyon: {
          50:  "#EDF7FC",
          100: "#D4ECF7",
          200: "#A8D7EE",
          300: "#6FBCDF",
          400: "#3EA0CE",
          500: "#0A66A0", // signature — brand halcyon blue
          600: "#085485",
          700: "#074571",
          800: "#063657",
          900: "#052940",
          950: "#031C2D",
        },
        paper: "#F7F4EC",
        ink:   "#0B1024",
      },
      letterSpacing: {
        tightest: "-0.04em",
      },
      fontSize: {
        hero:    ["clamp(2.75rem, 7.2vw, 6.25rem)", { lineHeight: "1.02", letterSpacing: "-0.035em" }],
        section: ["clamp(2rem, 4.8vw, 3.75rem)",     { lineHeight: "1.04", letterSpacing: "-0.03em" }],
      },
    },
  },
  plugins: [],
};

export default config;

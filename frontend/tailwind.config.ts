import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./app/**/*.{ts,tsx}",
    "./components/**/*.{ts,tsx}",
    "./lib/**/*.{ts,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        /* Shadcn-style semantic aliases — now pointing at Halcyon tokens.
           Existing components using `bg-background`, `text-foreground`,
           `border-border`, `bg-card`, `bg-primary` etc. keep working and
           automatically render in Halcyon colours. */
        border: "var(--border)",
        input: "var(--input)",
        ring: "var(--ring)",
        background: "var(--background)",
        foreground: "var(--foreground)",
        primary: {
          DEFAULT: "var(--primary)",
          foreground: "var(--primary-foreground)",
        },
        secondary: {
          DEFAULT: "var(--secondary)",
          foreground: "var(--secondary-foreground)",
        },
        muted: {
          DEFAULT: "var(--muted)",
          foreground: "var(--muted-foreground)",
        },
        accent: {
          DEFAULT: "var(--accent)",
          foreground: "var(--accent-foreground)",
        },
        destructive: {
          DEFAULT: "var(--destructive)",
          foreground: "var(--destructive-foreground)",
        },
        card: {
          DEFAULT: "var(--card)",
          foreground: "var(--card-foreground)",
        },
        popover: {
          DEFAULT: "var(--popover)",
          foreground: "var(--popover-foreground)",
        },
        /* Halcyon brand ramps — use these for intentional colour work
           instead of Tailwind's defaults. e.g. `bg-halcyonBlue-50`,
           `text-rust-700`, `border-n-100`. */
        halcyonBlue: {
          50: "var(--blue-50)",
          100: "var(--blue-100)",
          200: "var(--blue-200)",
          300: "var(--blue-300)",
          400: "var(--blue-400)",
          500: "var(--blue-500)",
          600: "var(--blue-600)",
          700: "var(--blue-700)",
          800: "var(--blue-800)",
          900: "var(--blue-900)",
        },
        rust: {
          50: "var(--rust-50)",
          100: "var(--rust-100)",
          200: "var(--rust-200)",
          300: "var(--rust-300)",
          400: "var(--rust-400)",
          500: "var(--rust-500)",
          600: "var(--rust-600)",
          700: "var(--rust-700)",
          800: "var(--rust-800)",
          900: "var(--rust-900)",
        },
        n: {
          50: "var(--n-50)",
          100: "var(--n-100)",
          200: "var(--n-200)",
          300: "var(--n-300)",
          400: "var(--n-400)",
          500: "var(--n-500)",
          600: "var(--n-600)",
          700: "var(--n-700)",
          800: "var(--n-800)",
        },
        paper: "var(--paper)",
        ink: "var(--ink)",
        success: {
          50: "var(--success-50)",
          500: "var(--success-500)",
          700: "var(--success-700)",
        },
        warning: {
          50: "var(--warning-50)",
          500: "var(--warning-500)",
          700: "var(--warning-700)",
        },
        error: {
          50: "var(--error-50)",
          500: "var(--error-500)",
          700: "var(--error-700)",
        },
      },
      borderRadius: {
        lg: "12px",
        md: "6px",
        sm: "2px",
      },
      fontFamily: {
        sans: ["var(--font-sans)", "Instrument Sans", "ui-sans-serif", "system-ui", "sans-serif"],
        serif: ["var(--font-serif)", "Instrument Serif", "Iowan Old Style", "Georgia", "serif"],
        mono: ["var(--font-mono)", "JetBrains Mono", "ui-monospace", "SFMono-Regular", "monospace"],
      },
      boxShadow: {
        "h-1": "var(--shadow-1)",
        "h-2": "var(--shadow-2)",
        "h-3": "var(--shadow-3)",
        "h-4": "var(--shadow-4)",
      },
    },
  },
  plugins: [],
};

export default config;

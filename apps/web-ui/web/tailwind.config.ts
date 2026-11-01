import type { Config } from "tailwindcss";
import typography from "@tailwindcss/typography";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "#0e0e11",
        surface: "#141418",
        "surface-2": "#1d1d23",
        border: "#26262e",
        text: "#f1f1f4",
        muted: "#9a9ba6",
        faint: "#6b6c77",
        accent: "#6366f1",
        "accent-hover": "#7679f5",
        "accent-fg": "#ffffff",
        danger: "#f87171",
        "danger-bg": "#2a1416",
        success: "#4ade80",
        "success-bg": "#0f2417",
      },
      fontFamily: {
        sans: ['"Inter Variable"', "Inter", "system-ui", "sans-serif"],
      },
      borderRadius: {
        DEFAULT: "6px",
        sm: "4px",
        md: "6px",
        lg: "8px",
        xl: "12px",
      },
    },
  },
  plugins: [typography],
} satisfies Config;

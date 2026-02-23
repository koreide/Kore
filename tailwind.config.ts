import type { Config } from "tailwindcss";
import animate from "tailwindcss-animate";

const config: Config = {
  darkMode: "class",
  content: [
    "./index.html",
    "./src/**/*.{ts,tsx,js,jsx}"
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ["-apple-system", "BlinkMacSystemFont", "Inter", "Segoe UI", "sans-serif"],
        mono: ["SFMono-Regular", "Menlo", "Monaco", "Consolas", "monospace"]
      },
      colors: {
        background: "#0b1221",
        surface: "#101828",
        accent: "#58d0ff",
        muted: "#1d2939",
        "status-running": "#10b981",
        "status-pending": "#f59e0b",
        "status-failed": "#ef4444",
      }
    }
  },
  plugins: [animate]
};

export default config;

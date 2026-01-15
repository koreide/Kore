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
        mono: ["SFMono-Regular", "Menlo", "Monaco", "Consolas", "monospace"]
      },
      colors: {
        background: "#0b1221",
        surface: "#101828",
        accent: "#58d0ff",
        muted: "#1d2939"
      }
    }
  },
  plugins: [animate]
};

export default config;



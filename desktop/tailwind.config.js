/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,jsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Neutral brand scale (near-monochrome aerospace look).
        aonyx: {
          50: "#f6f6f7",
          100: "#ececee",
          200: "#d6d7da",
          300: "#b1b2b8",
          400: "#86878f",
          500: "#6a6b73",
          600: "#54555c",
          700: "#43444a",
          800: "#2a2b2f",
          900: "#161618",
          950: "#0a0a0b",
        },
        // Warm accent (active nav bar, highlights) — derived from the brand halo.
        primary: {
          50: "#fff4ed",
          100: "#ffe6d4",
          200: "#fecaa8",
          300: "#fda571",
          400: "#fb7438",
          500: "#f95416",
          600: "#ea3c0c",
          700: "#c22d0c",
          800: "#9a2712",
          900: "#7c2512",
          950: "#430f07",
        },
      },
      fontFamily: {
        sans: ["Saira", "ui-sans-serif", "system-ui", "sans-serif"],
        cond: ['"Saira Condensed"', "Saira", "sans-serif"],
        mono: ["ui-monospace", '"JetBrains Mono"', "Menlo", "monospace"],
      },
    },
  },
  plugins: [],
};

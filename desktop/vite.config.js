import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri-friendly Vite config: fixed port, relative asset base (tauri:// origin),
// no screen clearing so Rust logs stay visible.
export default defineConfig({
  plugins: [react()],
  base: "./",
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: "dist", target: "esnext", sourcemap: false, emptyOutDir: true },
});

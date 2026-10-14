import { defineConfig } from "vite";

// Set by `tauri ios dev` / `tauri android dev` so devices can reach the dev server.
const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  root: "ui",
  // Keep Rust errors visible in the terminal.
  clearScreen: false,
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "safari16",
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
});

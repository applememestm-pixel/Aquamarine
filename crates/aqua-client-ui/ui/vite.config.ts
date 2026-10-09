import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri opens the frontend from vite's dev server, on a fixed port and
// with strict-port so its bootstrap doesn't race a moved port. The Tauri
// CLI reads TAURI_ENV_* at build time; we don't need to inject anything
// ourselves here.
// `tauri android dev` / `tauri ios dev` set TAURI_DEV_HOST so a phone can
// reach this dev server over the LAN; desktop keeps the loopback-only setup.
const mobileHost = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: mobileHost || "127.0.0.1",
    hmr: mobileHost
      ? { protocol: "ws", host: mobileHost, port: 5174 }
      : { host: "127.0.0.1", port: 5173 },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "es2022",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
}));

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri dev server conventions
const host = process.env.TAURI_DEV_HOST;
const port = Number(process.env.TAURI_DEV_PORT ?? 1420);

export default defineConfig({
  plugins: [react(), tailwindcss()],
  publicDir: "public",
  clearScreen: false,
  server: {
    port,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: port + 1 }
      : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});

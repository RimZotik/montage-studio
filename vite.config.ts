import { fileURLToPath, URL } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import pkg from "./package.json" with { type: "json" };

// Порт фиксированный — его ждет Tauri (`tauri.conf.json` → devUrl).
export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  server: { port: 1430, strictPort: true, host: "127.0.0.1", watch: { ignored: ["**/src-tauri/**"] } },
});

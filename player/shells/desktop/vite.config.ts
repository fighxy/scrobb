import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Порт и поведение — как ждёт Tauri (`src-tauri/tauri.conf.json`, build.devUrl).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  // Окно грузит сборку с диска, а не из сети: один файл на ~200 КБ gzip не помеха.
  build: { target: "es2022", chunkSizeWarningLimit: 800 },
});

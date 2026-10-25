import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Порт и поведение — как ждёт Tauri (`src-tauri/tauri.conf.json`, build.devUrl).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: { target: "es2022" },
});

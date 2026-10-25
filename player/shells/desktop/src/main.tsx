import { isTauri } from "@tauri-apps/api/core";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import type { Backend } from "./core";
import "./styles.css";

// В приложении — настоящее ядро; в браузере (`pnpm dev`) — подмена для вёрстки.
async function backend(): Promise<Backend> {
  if (isTauri()) return (await import("./tauri")).tauriBackend;
  return (await import("./mock")).createMockBackend();
}

backend().then((b) => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App backend={b} />
    </StrictMode>,
  );
});

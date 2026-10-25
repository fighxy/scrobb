// Бэкенд в приложении: команды Tauri из `src-tauri/src/bridge.rs`.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Added, Backend, CoreEvent, LibraryEvent, PlayRecord, Snapshot } from "./core";

const AUDIO = ["mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "aiff", "aif", "alac", "caf", "mka"];

export const tauriBackend: Backend = {
  dispatch: (command) => invoke("dispatch", { command }),
  snapshot: () => invoke<Snapshot>("snapshot"),
  async pickFiles() {
    const picked = await open({
      multiple: true,
      title: "Добавить музыку",
      filters: [{ name: "Аудио", extensions: AUDIO }],
    });
    return picked ?? [];
  },
  addFiles: (paths) => invoke<Added>("add_files", { paths }),
  async pickFolder() {
    return open({ directory: true, title: "Папка с музыкой" });
  },
  addFolder: (path) => invoke<boolean>("add_folder", { path }),
  removeFolder: (path) => invoke("remove_folder", { path }),
  rescan: () => invoke("rescan_library"),
  onLibraryEvent: (handler) => listen<LibraryEvent>("library://event", (e) => handler(e.payload)),
  recentPlays: (beforeMs, limit) =>
    invoke<PlayRecord[]>("recent_plays", { before_ms: beforeMs, limit }),
  onEvent: (handler) => listen<CoreEvent>("core://event", (e) => handler(e.payload)),
  openUrl: (url) => openUrl(url),
};

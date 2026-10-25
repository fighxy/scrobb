// Бэкенд в приложении: команды Tauri из `src-tauri/src/bridge.rs`.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemsInDir } from "@tauri-apps/plugin-opener";
import type {
  Added,
  Backend,
  CoreEvent,
  Exported,
  ImportReport,
  LibraryEvent,
  Playlist,
  PlaylistCard,
  PlaylistView,
  PlayRecord,
  Snapshot,
  TrackId,
} from "./core";
import { EXPORT_FORMATS } from "./core";

const AUDIO = ["mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "aiff", "aif", "alac", "caf", "mka"];
const PLAYLISTS = ["m3u", "m3u8", "pls", "xspf", "jspf", "json", "csv", "txt"];

// Схема обложек `src-tauri/src/covers.rs`: `cover://localhost/` или, на Windows,
// `http://cover.localhost/`. convertFileSrc кодирует путь целиком (и «/» тоже),
// поэтому берём от него только основу адреса.
const COVERS = convertFileSrc("", "cover");

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
  queue: () => invoke<TrackId[]>("queue"),
  coverUrl: (id, size) => `${COVERS}${id}/${size}`,
  coverColor: (id) => invoke<string | null>("cover_color", { id }),
  revealInFolder: (path) => revealItemsInDir(path),
  playlists: () => invoke<PlaylistCard[]>("playlists"),
  playlist: (id) => invoke<PlaylistView>("playlist", { id }),
  createPlaylist: (title, tracks) => invoke<Playlist>("create_playlist", { title, tracks }),
  updatePlaylist: (id, title, description) => invoke("update_playlist", { id, title, description }),
  deletePlaylist: (id) => invoke("delete_playlist", { id }),
  addToPlaylist: (id, tracks, at) => invoke<number>("add_to_playlist", { id, tracks, at }),
  removeFromPlaylist: (id, items) => invoke("remove_from_playlist", { id, items }),
  moveInPlaylist: (id, item, to) => invoke("move_in_playlist", { id, item, to }),
  async pickPlaylistFile() {
    const picked = await open({
      title: "Импорт плейлиста",
      filters: [{ name: "Плейлисты", extensions: PLAYLISTS }],
    });
    return picked ?? null;
  },
  importPlaylist: (path) => invoke<ImportReport>("import_playlist", { path }),
  async pickExportPath(defaultName, format) {
    const ext = EXPORT_FORMATS.find((f) => f.format === format)?.ext ?? format;
    return save({
      title: "Экспорт плейлиста",
      defaultPath: `${defaultName}.${ext}`,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
  },
  exportPlaylist: (id, format, path) => invoke<Exported>("export_playlist", { id, format, path }),
};

// Подменное ядро для разработки интерфейса в обычном браузере (`pnpm dev`)
// и для снимков экрана. Ведёт себя как настоящее в общих чертах: очередь,
// пауза, перемотка, история, привязка Scrobb с кодом.
//
// Параметры адреса: `?demo=empty` — пустая библиотека; `?demo=playing` —
// сразу играет трек; `?demo=linking` — открыто окно привязки;
// `?demo=scanning` — идёт сканирование папки; `?demo=big` — 10 000 треков
// (проверка плавности прокрутки); `?demo=playlist` — запуск с плейлистом,
// в котором есть треки не из библиотеки.
//
// Обложки — SVG-градиенты с оттенком от названия альбома; у одного альбома
// обложки нет, чтобы была видна заглушка.

import type {
  Added,
  Backend,
  Command,
  CoreEvent,
  ItemId,
  LibraryEvent,
  Playlist,
  PlaylistEntry,
  PlaylistFormat,
  PlayerState,
  PlayRecord,
  Scanning,
  ScrobbleStatus,
  Snapshot,
  Track,
  TrackId,
  TrackSnapshot,
} from "./core";
import { EXPORT_FORMATS } from "./core";

const DEMO: Array<[string, string, string, number]> = [
  ["Кукушка", "Кино", "Чёрный альбом", 397_000],
  ["Звезда по имени Солнце", "Кино", "Звезда по имени Солнце", 225_000],
  ["Группа крови", "Кино", "Группа крови", 286_000],
  ["Восьмиклассница", "Кино", "Начальник Камчатки", 186_000],
  ["Спокойная ночь", "Кино", "Группа крови", 375_000],
  ["Я хочу быть с тобой", "Наутилус Помпилиус", "Князь тишины", 345_000],
  ["Прогулки по воде", "Наутилус Помпилиус", "Крылья", 289_000],
  ["Город золотой", "Аквариум", "Десять стрел", 261_000],
  ["Поезд в огне", "Аквариум", "Равноденствие", 335_000],
  ["Всё идёт по плану", "Гражданская оборона", "Всё идёт по плану", 214_000],
  ["Перемен", "Кино", "Последний герой", 293_000],
  ["Пачка сигарет", "Кино", "Звезда по имени Солнце", 268_000],
];

function track(i: number, [title, artist, album, duration]: (typeof DEMO)[number]): Track {
  return {
    id: i + 1,
    uri: `/music/${artist}/${album}/${title}.flac`,
    title,
    artists: [artist],
    album,
    album_artist: artist,
    duration_ms: duration,
    track_no: (i % 9) + 1,
    disc_no: null,
    recording_mbid: null,
    isrc: null,
    replay_gain: { track_gain_db: null, track_peak: null, album_gain_db: null, album_peak: null },
  };
}

/** Альбом без обложки: на нём видна заглушка. */
const NO_COVER = "Всё идёт по плану";

/** Оттенок от названия (FNV-1a): у похожих названий — разные цвета. */
function hue(text: string): number {
  let h = 0x811c9dc5;
  for (const ch of text) h = Math.imul(h ^ ch.codePointAt(0)!, 16777619) >>> 0;
  return h % 360;
}

function hslToHex(h: number, s: number, l: number): string {
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const c = l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k - 3, 9 - k, 1));
    return Math.round(c * 255).toString(16).padStart(2, "0");
  };
  return `#${f(0)}${f(8)}${f(4)}`;
}

function coverSvg(album: string, artist: string): string {
  const h = hue(album);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
<stop offset="0" stop-color="hsl(${h} 70% 58%)"/><stop offset="1" stop-color="hsl(${(h + 50) % 360} 65% 22%)"/>
</linearGradient></defs>
<rect width="100" height="100" fill="url(#g)"/>
<circle cx="${30 + (h % 40)}" cy="${25 + (h % 30)}" r="${14 + (h % 12)}" fill="white" fill-opacity="0.28"/>
<text x="8" y="80" font-family="Inter, sans-serif" font-size="9" font-weight="700" fill="white">${album.slice(0, 22)}</text>
<text x="8" y="91" font-family="Inter, sans-serif" font-size="7" fill="white" opacity="0.8">${artist.slice(0, 26)}</text>
</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** 10 000 треков: 400 артистов по 2 альбома по 12–13 треков. */
function bigLibrary(): Track[] {
  return Array.from({ length: 10_000 }, (_, i) => {
    const [title] = DEMO[i % DEMO.length];
    const artist = `Артист ${String(Math.floor(i / 25) + 1).padStart(3, "0")}`;
    const album = `Альбом ${Math.floor(i / 12.5) + 1}`;
    return track(i, [`${title} (${i + 1})`, artist, album, 150_000 + ((i * 7919) % 240_000)]);
  });
}

function snapshotOf(t: Track): TrackSnapshot {
  return {
    title: t.title,
    artists: t.artists,
    album: t.album,
    album_artist: t.album_artist,
    duration_ms: t.duration_ms,
    recording_mbid: t.recording_mbid,
    ext_ids: {},
  };
}

/** Трек «из стриминга», которого нет в библиотеке. */
function foreign(title: string, artist: string, album: string, ms: number): TrackSnapshot {
  return { title, artists: [artist], album, album_artist: null, duration_ms: ms, recording_mbid: null, ext_ids: {} };
}

interface MockPlaylist {
  playlist: Playlist;
  items: Array<Omit<PlaylistEntry, "track_id">>;
}

declare global {
  interface Window {
    /** Последний экспорт подменного ядра — для проверок в Playwright. */
    __lastExport?: { format: PlaylistFormat; path: string; text: string };
  }
}

export function createMockBackend(search = location.search): Backend {
  const demo = new URLSearchParams(search).get("demo");
  const listeners = new Set<(e: CoreEvent) => void>();
  const libraryListeners = new Set<(e: LibraryEvent) => void>();
  let tracks: Track[] = demo === "empty" ? [] : demo === "big" ? bigLibrary() : DEMO.map((d, i) => track(i, d));
  const byId = (id: TrackId) => tracks.find((t) => t.id === id);
  let folders: string[] = demo === "empty" ? [] : ["/home/ivan/Музыка"];
  let scanning: Scanning | null =
    demo === "scanning" ? { folder: "/home/ivan/Музыка/Новое", found: 1240, read: 386 } : null;
  if (demo === "scanning") folders = [...folders, "/home/ivan/Музыка/Новое"];
  const emitLibrary = (event: LibraryEvent) => setTimeout(() => libraryListeners.forEach((l) => l(event)), 0);
  // Имитация сканирования: прогресс, затем треки, которых ещё нет.
  const simulateScan = (path: string) => {
    const total = DEMO.length;
    for (let read = 0; read <= total; read += 3) {
      setTimeout(() => {
        scanning = { folder: path, found: total, read: Math.min(read, total) };
        emitLibrary({ type: "scanning", data: scanning });
      }, read * 60);
    }
    setTimeout(() => {
      const known = new Set(tracks.map((t) => t.uri));
      const found = DEMO.map((d, i) => track(i, d)).filter((t) => !known.has(t.uri));
      tracks = [...tracks, ...found.map((t, i) => ({ ...t, id: tracks.length + i + 1 }))];
      scanning = null;
      emitLibrary({ type: "changed" });
    }, total * 60 + 100);
  };
  let queue: number[] = tracks.map((t) => t.id);
  let index = 0;
  let state: PlayerState = {
    status: "stopped",
    current: null,
    position_ms: 0,
    queue_position: null,
    repeat: "off",
    shuffle: false,
    volume: 0.8,
  };
  let scrobbling: ScrobbleStatus = { pending: 0, state: "ready" };
  const now = Date.now();
  const history: PlayRecord[] = tracks.slice(4, 10).map((t, i) => ({
    id: `mock-${i}`,
    started_at_ms: now - (i + 1) * 7 * 60_000,
    ended_at_ms: now - (i + 1) * 7 * 60_000 + (i === 2 ? 40_000 : (t.duration_ms ?? 0)),
    played_ms: i === 2 ? 40_000 : (t.duration_ms ?? 0),
    outcome: i === 2 ? "skipped" : "finished",
    scrobbled: i !== 2,
    track: {
      title: t.title,
      artists: t.artists,
      album: t.album,
      album_artist: t.album_artist,
      duration_ms: t.duration_ms,
      recording_mbid: null,
      ext_ids: {},
    },
    track_id: t.id,
    device_id: "mock",
    origin: "player",
  }));
  let startedAt = performance.now();

  // Плейлисты: в одном есть треки «из Spotify», которых нет в библиотеке.
  let nextId = 1;
  const id = () => `mock-${nextId++}`;
  const entry = (track: TrackSnapshot, location: string | null): MockPlaylist["items"][number] => ({
    id: id(),
    position: String(nextId).padStart(6, "0"),
    added_at_ms: now,
    location,
    track,
  });
  const fromLibrary = (ids: number[]) =>
    ids.flatMap((i) => {
      const t = tracks.find((x) => x.id === i);
      return t ? [entry(snapshotOf(t), t.uri)] : [];
    });
  const makePlaylist = (title: string, items: MockPlaylist["items"], origin: string | null = null, age = 0): MockPlaylist => ({
    playlist: { id: id(), title, description: null, origin, created_at_ms: now - age, updated_at_ms: now - age },
    items,
  });
  let playlists: MockPlaylist[] =
    demo === "empty"
      ? []
      : [
          makePlaylist("Вечер на кухне", fromLibrary([8, 6, 2, 9, 4]), null, 1000),
          makePlaylist(
            "Импорт из Spotify",
            [
              ...fromLibrary([1]),
              entry(foreign("Bohemian Rhapsody", "Queen", "A Night at the Opera", 354_000), null),
              ...fromLibrary([7]),
              entry(foreign("Blinding Lights", "The Weeknd", "After Hours", 200_000), null),
            ],
            "import:csv",
            2000,
          ),
          makePlaylist("Кино — лучшее", fromLibrary([1, 2, 3, 5, 11, 12]), null, 3000),
        ];
  const resolve = (item: MockPlaylist["items"][number]): TrackId | null =>
    tracks.find((t) => t.uri === item.location)?.id ??
    tracks.find(
      (t) =>
        t.title.toLowerCase() === item.track.title.toLowerCase() &&
        t.artists[0]?.toLowerCase() === item.track.artists[0]?.toLowerCase(),
    )?.id ??
    null;
  const findPlaylist = (pid: string) => {
    const p = playlists.find((x) => x.playlist.id === pid);
    if (!p) throw { type: "playlist", data: `нет плейлиста ${pid}` };
    return p;
  };
  const touch = (p: MockPlaylist) => {
    p.playlist = { ...p.playlist, updated_at_ms: Date.now() };
  };
  const covers = (p: MockPlaylist) => {
    const albums = new Set<string>();
    const out: TrackId[] = [];
    for (const item of p.items) {
      const tid = resolve(item);
      const t = tracks.find((x) => x.id === tid);
      if (!t || out.length === 4) continue;
      const album = t.album ?? `#${t.id}`;
      if (albums.has(album)) continue;
      albums.add(album);
      out.push(t.id);
    }
    return out;
  };

  const emit = (event: CoreEvent) => setTimeout(() => listeners.forEach((l) => l(event)), 0);
  const position = () =>
    state.status === "playing"
      ? Math.min(state.position_ms + (performance.now() - startedAt), state.current?.duration_ms ?? Infinity)
      : state.position_ms;
  const publish = (patch: Partial<PlayerState>) => {
    state = { ...state, position_ms: position(), ...patch };
    startedAt = performance.now();
    emit({ type: "state", data: state });
  };
  const playAt = (i: number) => {
    index = i;
    const current = tracks.find((t) => t.id === queue[i]) ?? null;
    history.unshift({
      ...history[0],
      id: `mock-${Date.now()}`,
      started_at_ms: Date.now(),
      ended_at_ms: null,
      played_ms: 0,
      outcome: "playing",
      scrobbled: false,
      track: { ...history[0]?.track, title: current?.title ?? "", artists: current?.artists ?? [] },
      track_id: current?.id ?? null,
    });
    publish({ status: "playing", current, position_ms: 0, queue_position: [i, queue.length] });
  };
  const setScrobbling = (s: ScrobbleStatus) => {
    scrobbling = s;
    emit({ type: "scrobbling", data: s });
  };

  if (demo === "playing" || demo === "linking") {
    index = 2;
    state = { ...state, status: "playing", current: tracks[2] ?? null, position_ms: 72_000, queue_position: [2, queue.length] };
  }
  if (demo === "linking") {
    scrobbling = { pending: 1, state: "linking" };
    setTimeout(
      () => emit({ type: "scrobb_link_code", data: { user_code: "WDJB-MJHT", verification_uri_complete: "https://scrobb.ru/link?code=WDJB-MJHT" } }),
      50,
    );
  }

  return {
    async dispatch(command: Command) {
      switch (command.type) {
        case "play_queue":
          queue = command.data.tracks;
          return playAt(command.data.start);
        case "pause":
          return publish({ status: "paused" });
        case "resume":
          return state.current ? publish({ status: "playing" }) : playAt(index);
        case "stop":
          return publish({ status: "stopped", position_ms: 0 });
        case "next":
          return index + 1 < queue.length ? playAt(index + 1) : publish({ status: "stopped", position_ms: 0 });
        case "previous":
          return position() > 3000 ? publish({ position_ms: 0 }) : playAt(Math.max(0, index - 1));
        case "seek":
          return publish({ position_ms: command.data.position_ms });
        case "set_repeat":
          return publish({ repeat: command.data });
        case "set_volume":
          return publish({ volume: command.data });
        case "link_scrobb_account":
          setScrobbling({ pending: scrobbling.pending, state: "linking" });
          emit({ type: "scrobb_link_code", data: { user_code: "WDJB-MJHT", verification_uri_complete: "https://scrobb.ru/link?code=WDJB-MJHT" } });
          setTimeout(() => scrobbling.state === "linking" && setScrobbling({ pending: 0, state: "ready" }), 4000);
          return;
        case "cancel_scrobb_link":
        case "unlink_scrobb_account":
          return setScrobbling({ pending: scrobbling.pending, state: "not_linked" });
      }
    },
    async snapshot(): Promise<Snapshot> {
      return {
        state: { ...state, position_ms: position() },
        tracks,
        folders,
        scanning,
        scrobbling,
        scrobbling_available: true,
        opened_playlist: demo === "playlist" ? (playlists.find((p) => p.playlist.title === "Импорт из Spotify")?.playlist.id ?? null) : null,
      };
    },
    async pickFiles() {
      return ["/music/Кино/Последний герой/Бездельник.flac", "/music/Земфира/Земфира/Ариведерчи.mp3"];
    },
    async addFiles(paths: string[]): Promise<Added> {
      const added = paths.map((p, i) => {
        const name = p.split("/").pop()?.replace(/\.[^.]+$/, "") ?? p;
        return track(tracks.length + i, [name, p.split("/")[2] ?? "", p.split("/")[3] ?? "", 240_000]);
      });
      tracks = [...tracks, ...added];
      return { tracks: added, failed: [] };
    },
    async recentPlays(beforeMs, limit) {
      return history.filter((h) => beforeMs === null || h.started_at_ms < beforeMs).slice(0, limit);
    },
    async pickFolder() {
      return "/home/ivan/Музыка";
    },
    async addFolder(path) {
      if (folders.some((f) => path === f || path.startsWith(`${f}/`))) return false;
      folders = [...folders.filter((f) => !f.startsWith(`${path}/`)), path];
      simulateScan(path);
      return true;
    },
    async removeFolder(path) {
      folders = folders.filter((f) => f !== path);
      tracks = tracks.filter((t) => !t.uri.startsWith(`${path}/`) && !t.uri.startsWith("/music/"));
      emitLibrary({ type: "changed" });
    },
    async rescan() {
      folders.forEach(simulateScan);
    },
    async onLibraryEvent(handler) {
      libraryListeners.add(handler);
      return () => libraryListeners.delete(handler);
    },
    async onEvent(handler) {
      listeners.add(handler);
      return () => listeners.delete(handler);
    },
    async openUrl(url) {
      window.open(url, "_blank", "noopener");
    },
    async queue() {
      return queue;
    },
    coverUrl(id) {
      const t = byId(id);
      // Пустой data-адрес не раскодируется — как 404 у настоящей схемы.
      if (!t?.album || t.album === NO_COVER) return "data:,";
      return coverSvg(t.album, t.artists[0] ?? "");
    },
    async coverColor(id) {
      const t = byId(id);
      if (!t?.album || t.album === NO_COVER) return null;
      return hslToHex(hue(t.album), 0.55, 0.3);
    },
    async revealInFolder() {},
    async playlists() {
      return [...playlists]
        .sort((a, b) => b.playlist.updated_at_ms - a.playlist.updated_at_ms)
        .map((p) => ({
          playlist: p.playlist,
          items: p.items.length,
          duration_ms: p.items.reduce((sum, i) => sum + (i.track.duration_ms ?? 0), 0),
          covers: covers(p),
        }));
    },
    async playlist(pid) {
      const p = findPlaylist(pid);
      return { playlist: p.playlist, items: p.items.map((i) => ({ ...i, track_id: resolve(i) })) };
    },
    async createPlaylist(title, ids) {
      const p = makePlaylist(title.trim(), fromLibrary(ids));
      p.playlist.updated_at_ms = Date.now();
      playlists = [...playlists, p];
      return p.playlist;
    },
    async updatePlaylist(pid, title, description) {
      const p = findPlaylist(pid);
      p.playlist = { ...p.playlist, title, description };
      touch(p);
    },
    async deletePlaylist(pid) {
      playlists = playlists.filter((p) => p.playlist.id !== pid);
    },
    async addToPlaylist(pid, ids, at) {
      const p = findPlaylist(pid);
      const items = fromLibrary(ids);
      p.items.splice(at ?? p.items.length, 0, ...items);
      touch(p);
      return items.length;
    },
    async removeFromPlaylist(pid, items: ItemId[]) {
      const p = findPlaylist(pid);
      p.items = p.items.filter((i) => !items.includes(i.id));
      touch(p);
    },
    async moveInPlaylist(pid, item, to) {
      const p = findPlaylist(pid);
      const from = p.items.findIndex((i) => i.id === item);
      const [moved] = p.items.splice(from, 1);
      p.items.splice(Math.min(to, p.items.length), 0, moved);
      touch(p);
    },
    async pickPlaylistFile() {
      return "/home/ivan/Плейлисты/Дорога.m3u8";
    },
    async importPlaylist(path) {
      const title = path.split("/").pop()?.replace(/\.[^.]+$/, "") ?? "Импорт";
      const p = makePlaylist(title, [...fromLibrary([3, 10, 6]), entry(foreign("Нет в библиотеке", "Кто-то", "", 180_000), "/home/ivan/Музыка/нет.flac")], "import:m3u");
      p.playlist.updated_at_ms = Date.now();
      playlists = [...playlists, p];
      return { playlist: p.playlist, total: 4, matched: 3 };
    },
    async pickExportPath(name, format) {
      return `/home/ivan/Плейлисты/${name}.${EXPORT_FORMATS.find((f) => f.format === format)?.ext ?? format}`;
    },
    async exportPlaylist(pid, format, path) {
      const p = findPlaylist(pid);
      const label = (t: TrackSnapshot) => (t.artists.length ? `${t.artists.join(", ")} - ${t.title}` : t.title);
      let text: string;
      let skipped = 0;
      if (format === "m3u") {
        const lines = ["#EXTM3U", `#PLAYLIST:${p.playlist.title}`];
        for (const i of p.items) {
          const location = i.location ?? tracks.find((t) => t.id === resolve(i))?.uri;
          if (!location) {
            skipped++;
            continue;
          }
          lines.push(`#EXTINF:${Math.ceil((i.track.duration_ms ?? -1000) / 1000)},${label(i.track)}`, location);
        }
        text = `${lines.join("\n")}\n`;
      } else if (format === "txt") {
        text = p.items.map((i) => label(i.track)).join("\n") + "\n";
      } else {
        text = JSON.stringify({ playlist: { title: p.playlist.title, track: p.items.map((i) => i.track) } }, null, 2);
      }
      window.__lastExport = { format, path, text };
      return { text, skipped };
    },
  };
}

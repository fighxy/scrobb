// Подменное ядро для разработки интерфейса в обычном браузере (`pnpm dev`)
// и для снимков экрана. Ведёт себя как настоящее в общих чертах: очередь,
// пауза, перемотка, история, привязка Scrobb с кодом.
//
// Параметры адреса: `?demo=empty` — пустая библиотека; `?demo=playing` —
// сразу играет трек; `?demo=linking` — открыто окно привязки.

import type {
  Added,
  Backend,
  Command,
  CoreEvent,
  PlayerState,
  PlayRecord,
  ScrobbleStatus,
  Snapshot,
  Track,
} from "./core";

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
    replay_gain: { track_gain_db: null, track_peak: null, album_gain_db: null, album_peak: null },
  };
}

export function createMockBackend(search = location.search): Backend {
  const demo = new URLSearchParams(search).get("demo");
  const listeners = new Set<(e: CoreEvent) => void>();
  let tracks: Track[] = demo === "empty" ? [] : DEMO.map((d, i) => track(i, d));
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
      return { state: { ...state, position_ms: position() }, tracks, scrobbling, scrobbling_available: true };
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
    async onEvent(handler) {
      listeners.add(handler);
      return () => listeners.delete(handler);
    },
    async openUrl(url) {
      window.open(url, "_blank", "noopener");
    },
  };
}

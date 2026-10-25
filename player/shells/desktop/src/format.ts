import type { Outcome, PlayerState } from "./core";

/** `3:07`, `1:02:05`; неизвестно — `—`. */
export function duration(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/**
 * Позиция «сейчас»: ядро присылает снимок не чаще нескольких раз в секунду,
 * а между снимками окно двигает ползунок само (`docs/player-architecture.md` §3.3).
 */
export function livePosition(state: PlayerState, receivedAt: number, now: number): number {
  if (state.status !== "playing") return state.position_ms;
  const moved = state.position_ms + Math.max(0, now - receivedAt);
  return Math.min(moved, state.current?.duration_ms ?? Number.POSITIVE_INFINITY);
}

/** Время прослушивания в истории: сегодня — `14:05`, иначе — `3 окт., 14:05`. */
export function playedAt(ms: number, now = Date.now()): string {
  const date = new Date(ms);
  const time = date.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  return new Date(now).toDateString() === date.toDateString()
    ? time
    : `${date.toLocaleDateString("ru-RU", { day: "numeric", month: "short" })}, ${time}`;
}

export const OUTCOME: Record<Outcome, string> = {
  playing: "играет",
  finished: "дослушан",
  skipped: "пропущен",
  stopped: "остановлен",
  error: "ошибка",
  interrupted: "прервано",
};

/** Имя папки для подписи: последняя часть пути. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? path;
}

export function artists(list: string[]): string {
  return list.length ? list.join(", ") : "Неизвестный артист";
}

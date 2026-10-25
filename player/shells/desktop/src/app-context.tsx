// Общее для экранов: бэкенд, библиотека, состояние плеера и действия.
// Через контекст, чтобы не протаскивать десяток свойств через каждый экран.

import { createContext, useContext } from "react";
import type { Backend, CoverSize, PlayerState, Track, TrackId } from "./core";
import type { Library } from "./lib/library";
import type { View } from "./lib/nav";

export interface AppApi {
  backend: Backend;
  lib: Library;
  tracks: Track[];
  state: PlayerState;
  queue: TrackId[];
  go(view: View): void;
  /** Играть список с трека `index` (очередь — весь список). */
  play(list: Track[], index: number): void;
  /** Пауза или продолжение текущего трека. */
  toggle(): void;
  /** Играет ли сейчас очередь, собранная из этого списка. */
  isActive(list: Track[]): boolean;
  /** Обложка трека (общая для альбома). */
  cover(t: Track, size?: CoverSize): string;
  reveal(path: string): void;
}

export const AppContext = createContext<AppApi | null>(null);

export function useApp(): AppApi {
  const api = useContext(AppContext);
  if (!api) throw new Error("useApp вне AppContext");
  return api;
}

/** Совпадает ли очередь со списком треков по порядку. */
export function sameQueue(queue: TrackId[], list: Track[]): boolean {
  return queue.length === list.length && list.every((t, i) => t.id === queue[i]);
}

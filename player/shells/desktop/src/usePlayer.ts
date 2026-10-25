import { useCallback, useEffect, useState } from "react";
import { toast as notify } from "sonner";
import type { Backend, Command, PlayerState, PlaylistId, PlayRecord, Scanning, ScrobbleStatus, Track, TrackId } from "./core";
import { describeError } from "./core";
import { livePosition } from "./format";

const EMPTY: PlayerState = {
  status: "stopped",
  current: null,
  position_ms: 0,
  queue_position: null,
  repeat: "off",
  shuffle: false,
  volume: 1,
};

export interface LinkCode {
  user_code: string;
  verification_uri_complete: string;
}

/** Состояние окна: снимок при открытии, дальше — события ядра. */
export function usePlayer(backend: Backend) {
  const [state, setState] = useState<PlayerState>(EMPTY);
  const [receivedAt, setReceivedAt] = useState(() => performance.now());
  const [tracks, setTracks] = useState<Track[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [scanning, setScanning] = useState<Scanning | null>(null);
  const [scrobbling, setScrobbling] = useState<ScrobbleStatus | null>(null);
  const [scrobblingAvailable, setScrobblingAvailable] = useState(false);
  const [openedPlaylist, setOpenedPlaylist] = useState<PlaylistId | null>(null);
  const [linkCode, setLinkCode] = useState<LinkCode | null>(null);
  const [history, setHistory] = useState<PlayRecord[]>([]);
  const [queue, setQueue] = useState<TrackId[]>([]);

  const toast = useCallback((text: string) => {
    notify(text);
  }, []);

  const refreshHistory = useCallback(() => {
    backend.recentPlays(null, 50).then(setHistory, (e) => toast(describeError(e)));
  }, [backend, toast]);

  // Библиотеку целиком перечитываем из снимка: после сканирования и смены папок.
  const refreshLibrary = useCallback(() => {
    backend.snapshot().then((snap) => {
      setTracks(snap.tracks);
      setFolders(snap.folders);
      setScanning(snap.scanning);
    }, (e) => toast(describeError(e)));
  }, [backend, toast]);

  useEffect(() => {
    let cancelled = false;
    let historyTimer: ReturnType<typeof setTimeout> | undefined;
    const unlisten = backend.onEvent((event) => {
      switch (event.type) {
        case "state":
          setState(event.data);
          setReceivedAt(performance.now());
          // История меняется на смене трека и остановке; не чаще раза в полсекунды.
          clearTimeout(historyTimer);
          historyTimer = setTimeout(refreshHistory, 500);
          break;
        case "scrobbling":
          setScrobbling(event.data);
          if (event.data.state !== "linking") setLinkCode(null);
          break;
        case "scrobb_link_code":
          setLinkCode(event.data);
          break;
        case "error":
          toast(event.data);
          break;
      }
    });
    const unlistenLibrary = backend.onLibraryEvent((event) => {
      switch (event.type) {
        case "scanning":
          setScanning(event.data);
          break;
        case "changed":
          refreshLibrary();
          break;
        case "error":
          toast(event.data);
          break;
      }
    });
    backend.snapshot().then((snap) => {
      if (cancelled) return;
      setState(snap.state);
      setReceivedAt(performance.now());
      setTracks(snap.tracks);
      setFolders(snap.folders);
      setScanning(snap.scanning);
      setScrobbling((current) => current ?? snap.scrobbling);
      setScrobblingAvailable(snap.scrobbling_available);
      setOpenedPlaylist(snap.opened_playlist);
    }, (e) => toast(describeError(e)));
    refreshHistory();
    return () => {
      cancelled = true;
      clearTimeout(historyTimer);
      unlisten.then((stop) => stop());
      unlistenLibrary.then((stop) => stop());
    };
  }, [backend, refreshHistory, refreshLibrary, toast]);

  // Очередь меняет только `play_queue`: из окна она известна сразу, а очередь,
  // собранную оболочкой (файлы из командной строки), перечитываем, если
  // снимок с ней не сходится.
  const length = state.queue_position?.[1];
  const currentId = state.current?.id;
  const queueStale = length !== undefined && (length !== queue.length || (currentId !== undefined && !queue.includes(currentId)));
  useEffect(() => {
    if (queueStale) backend.queue().then(setQueue, () => {});
  }, [backend, queueStale]);

  const run = useCallback(
    (command: Command) => {
      if (command.type === "play_queue") setQueue(command.data.tracks);
      backend.dispatch(command).catch((e) => toast(describeError(e)));
    },
    [backend, toast],
  );

  const addFiles = useCallback(async () => {
    try {
      const paths = await backend.pickFiles();
      if (!paths.length) return;
      const added = await backend.addFiles(paths);
      setTracks((all) => [...all, ...added.tracks]);
      if (added.failed.length) toast(`Не удалось открыть: ${added.failed.join(", ")}`);
    } catch (e) {
      toast(describeError(e));
    }
  }, [backend, toast]);

  const addFolder = useCallback(async () => {
    try {
      const path = await backend.pickFolder();
      if (!path) return;
      const added = await backend.addFolder(path);
      if (added) refreshLibrary();
      else toast("Эта папка уже в библиотеке");
    } catch (e) {
      toast(describeError(e));
    }
  }, [backend, refreshLibrary, toast]);

  const removeFolder = useCallback(
    (path: string) => {
      backend.removeFolder(path).catch((e) => toast(describeError(e)));
    },
    [backend, toast],
  );

  const rescan = useCallback(() => {
    backend.rescan().catch((e) => toast(describeError(e)));
  }, [backend, toast]);

  return {
    state,
    receivedAt,
    tracks,
    folders,
    scanning,
    scrobbling,
    scrobblingAvailable,
    openedPlaylist,
    linkCode,
    history,
    queue,
    toast,
    run,
    addFiles,
    addFolder,
    removeFolder,
    rescan,
  };
}

/** Позиция, которая движется, пока трек играет: 4 раза в секунду хватает
 *  ползунку и счётчику и не тратит батарею ноутбука. */
export function useLivePosition(state: PlayerState, receivedAt: number): number {
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    if (state.status !== "playing") return;
    const timer = setInterval(() => setNow(performance.now()), 250);
    return () => clearInterval(timer);
  }, [state.status, receivedAt]);
  return livePosition(state, receivedAt, Math.max(now, receivedAt));
}

const colors = new Map<TrackId, string | null>();

/** Цвет обложки для фона; пока не пришёл (или обложки нет) — `null`. */
export function useCoverColor(backend: Backend, id: TrackId | null): string | null {
  const [color, setColor] = useState<string | null>(() => (id === null ? null : (colors.get(id) ?? null)));
  useEffect(() => {
    if (id === null) return setColor(null);
    if (colors.has(id)) return setColor(colors.get(id)!);
    let cancelled = false;
    backend.coverColor(id).then(
      (c) => {
        colors.set(id, c);
        if (!cancelled) setColor(c);
      },
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, [backend, id]);
  return color;
}

/** Совпадает ли медиазапрос — для раскладки, которой мало одного CSS. */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => matchMedia(query).matches);
  useEffect(() => {
    const list = matchMedia(query);
    const update = () => setMatches(list.matches);
    update();
    list.addEventListener("change", update);
    return () => list.removeEventListener("change", update);
  }, [query]);
  return matches;
}

import { useCallback, useEffect, useRef, useState } from "react";
import type { Backend, Command, PlayerState, PlayRecord, Scanning, ScrobbleStatus, Track } from "./core";
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

export interface Toast {
  id: number;
  text: string;
}

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
  const [linkCode, setLinkCode] = useState<LinkCode | null>(null);
  const [history, setHistory] = useState<PlayRecord[]>([]);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextToast = useRef(1);

  const toast = useCallback((text: string) => {
    const id = nextToast.current++;
    setToasts((all) => [...all.slice(-3), { id, text }]);
    setTimeout(() => setToasts((all) => all.filter((t) => t.id !== id)), 5000);
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
    }, (e) => toast(describeError(e)));
    refreshHistory();
    return () => {
      cancelled = true;
      clearTimeout(historyTimer);
      unlisten.then((stop) => stop());
      unlistenLibrary.then((stop) => stop());
    };
  }, [backend, refreshHistory, refreshLibrary, toast]);

  const run = useCallback(
    (command: Command) => {
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
    linkCode,
    history,
    toasts,
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

import { ChevronLeft, ChevronRight, Search } from "lucide-react";
import { domAnimation, LazyMotion, m, MotionConfig } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { useDefaultLayout } from "react-resizable-panels";
import { AppContext, type AppApi, sameQueue } from "./app-context";
import { IconButton } from "./components/common";
import { NowPlayingFullscreen, NowPlayingPanel } from "./components/NowPlaying";
import { TopBarContext } from "./components/Page";
import { PlayerBar } from "./components/PlayerBar";
import { ScrobbLink } from "./components/ScrobbLink";
import { SearchPalette } from "./components/SearchPalette";
import { Sidebar } from "./components/Sidebar";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "./components/ui/resizable";
import { Toaster } from "./components/ui/sonner";
import { TooltipProvider } from "./components/ui/tooltip";
import { type Backend, describeError } from "./core";
import { coverOf, groupLibrary } from "./lib/library";
import { useNav, type View } from "./lib/nav";
import { useTheme } from "./lib/theme";
import { useMediaQuery, usePlayer } from "./usePlayer";
import { AlbumsView, AlbumView, ArtistsView, ArtistView } from "./views/Albums";
import { HistoryView } from "./views/History";
import { PlaylistView } from "./views/Playlist";
import { usePlaylists } from "./usePlaylists";
import { TracksView } from "./views/Tracks";

const IS_MAC = /Mac OS/.test(navigator.userAgent);

/** Размеры панелей запоминаются; если хранилище недоступно — просто нет. */
const layoutStorage = {
  getItem(key: string) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  setItem(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      // Размеры не запомнятся — не страшно.
    }
  },
};

function viewKey(view: View): string {
  return "key" in view ? `${view.kind}:${view.key}` : view.kind;
}

export function App({ backend }: { backend: Backend }) {
  const p = usePlayer(backend);
  const playlists = usePlaylists(backend, p.toast, p.tracks);
  const { theme, resolved, setTheme } = useTheme();
  const nav = useNav();
  // ≥ 1100 px — три колонки; 760–1100 — «Сейчас играет» по кнопке; уже — полоса значков.
  const wide = useMediaQuery("(min-width: 1100px)");
  const medium = useMediaQuery("(min-width: 760px)");
  const lib = useMemo(() => groupLibrary(p.tracks), [p.tracks]);
  const [nowPlaying, setNowPlaying] = useState(wide);
  const [fullscreen, setFullscreen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [sidebarCompact, setSidebarCompact] = useState(false);
  const panels = nowPlaying ? ["sidebar", "main", "now-playing"] : ["sidebar", "main"];
  const layout = useDefaultLayout({ id: "scrobb-layout", panelIds: panels, storage: layoutStorage });

  useEffect(() => setNowPlaying(wide), [wide]);
  // Запустили с файлом плейлиста — сразу показать его.
  const { go } = nav;
  useEffect(() => {
    if (p.openedPlaylist) go({ kind: "playlist", key: p.openedPlaylist });
  }, [p.openedPlaylist, go]);
  useEffect(() => {
    if (medium) setFullscreen(false);
  }, [medium]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setSearchOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const { run, state, queue, tracks } = p;
  const api = useMemo<AppApi>(
    () => ({
      backend,
      lib,
      tracks,
      state,
      queue,
      go: nav.go,
      play: (list, index) => run({ type: "play_queue", data: { tracks: list.map((t) => t.id), start: index } }),
      toggle: () => run({ type: state.status === "playing" || state.status === "buffering" ? "pause" : "resume" }),
      isActive: (list) => list.length > 0 && state.current !== null && state.status !== "stopped" && sameQueue(queue, list),
      cover: (t, size = "s") => backend.coverUrl(coverOf(lib, t), size),
      reveal: (path) => {
        backend.revealInFolder(path).catch((e) => p.toast(describeError(e)));
      },
      playlists,
    }),
    [backend, lib, tracks, state, queue, nav.go, run, p.toast, playlists],
  );

  const topBar = (
    <>
      <IconButton label="Назад" disabled={!nav.canBack} onClick={nav.back} className="bg-background/55 text-foreground">
        <ChevronLeft />
      </IconButton>
      <IconButton label="Вперёд" disabled={!nav.canForward} onClick={nav.forward} className="bg-background/55 text-foreground">
        <ChevronRight />
      </IconButton>
      <button
        type="button"
        onClick={() => setSearchOpen(true)}
        className="ml-1 flex h-9 w-full max-w-[360px] min-w-0 items-center gap-2 rounded-full bg-background/55 px-3 text-[13px] text-muted-foreground transition-colors hover:bg-background/75 hover:text-foreground"
      >
        <Search className="size-4 shrink-0" />
        <span className="flex-1 truncate text-left">Поиск</span>
        <kbd className="hidden rounded bg-surface-raised px-1.5 py-0.5 font-sans text-[11px] sm:inline">{IS_MAC ? "⌘K" : "Ctrl K"}</kbd>
      </button>
      <div className="ml-auto flex shrink-0 items-center">
        <ScrobbLink status={p.scrobbling} available={p.scrobblingAvailable} code={p.linkCode} run={run} openUrl={(url) => void backend.openUrl(url)} />
      </div>
    </>
  );

  const library = { scanning: p.scanning, onAddFolder: p.addFolder, onAddFiles: p.addFiles };
  const view = nav.view;
  const page = (() => {
    switch (view.kind) {
      case "tracks":
        return <TracksView {...library} />;
      case "albums":
        return <AlbumsView />;
      case "album":
        return <AlbumView albumKey={view.key} />;
      case "artists":
        return <ArtistsView />;
      case "artist":
        return <ArtistView artistKey={view.key} />;
      case "history":
        return <HistoryView plays={p.history} />;
      case "playlist":
        return <PlaylistView id={view.key} />;
    }
  })();

  const main = (
    <main className="h-full min-w-0 flex-1" aria-label="Содержимое">
      <TopBarContext.Provider value={topBar}>
        <m.div key={viewKey(view)} className="h-full" initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.15 }}>
          {page}
        </m.div>
      </TopBarContext.Provider>
    </main>
  );

  const sidebar = (compact: boolean) => (
    <Sidebar
      compact={compact}
      view={view}
      go={nav.go}
      folders={p.folders}
      scanning={p.scanning}
      theme={theme}
      setTheme={setTheme}
      onAddFolder={p.addFolder}
      onAddFiles={p.addFiles}
      onRemoveFolder={p.removeFolder}
      onRescan={p.rescan}
    />
  );

  return (
    <AppContext.Provider value={api}>
      {/* Анимации подгружаются частью (domAnimation), а не целиком: так меньше сборка. */}
      <LazyMotion features={domAnimation} strict>
      <MotionConfig reducedMotion="user">
        <TooltipProvider delayDuration={500}>
          <div className="flex h-full flex-col gap-2 p-2 pb-0">
            {medium ? (
              <ResizablePanelGroup
                orientation="horizontal"
                className="min-h-0 flex-1"
                defaultLayout={layout.defaultLayout}
                onLayoutChanged={layout.onLayoutChanged}
              >
                <ResizablePanel
                  id="sidebar"
                  defaultSize={wide ? 300 : 240}
                  minSize={220}
                  maxSize={420}
                  collapsible
                  collapsedSize={64}
                  onResize={(size) => setSidebarCompact(size.inPixels < 120)}
                >
                  {sidebar(sidebarCompact)}
                </ResizablePanel>
                <PanelGap />
                <ResizablePanel id="main" minSize={360}>
                  {main}
                </ResizablePanel>
                {nowPlaying && (
                  <>
                    <PanelGap />
                    <ResizablePanel id="now-playing" defaultSize={320} minSize={280} maxSize={460}>
                      <NowPlayingPanel onClose={() => setNowPlaying(false)} />
                    </ResizablePanel>
                  </>
                )}
              </ResizablePanelGroup>
            ) : (
              <div className="flex min-h-0 flex-1 gap-2">
                <div className="w-16 shrink-0">{sidebar(true)}</div>
                {main}
              </div>
            )}
            <PlayerBar
              compact={!medium}
              receivedAt={p.receivedAt}
              run={run}
              nowPlayingOpen={medium ? nowPlaying : fullscreen}
              onToggleNowPlaying={() => (medium ? setNowPlaying((open) => !open) : setFullscreen((open) => !open))}
            />
          </div>
          <NowPlayingFullscreen open={fullscreen} onClose={() => setFullscreen(false)} receivedAt={p.receivedAt} run={run} />
          <SearchPalette open={searchOpen} onOpenChange={setSearchOpen} />
          <Toaster theme={resolved} position="bottom-right" offset={{ bottom: 96, right: 16 }} />
        </TooltipProvider>
      </MotionConfig>
      </LazyMotion>
    </AppContext.Provider>
  );
}

/** Промежуток между панелями: за него тянут, при наведении проступает линия. */
function PanelGap() {
  return (
    <ResizableHandle className="w-2 bg-transparent after:w-0.5 after:rounded-full after:transition-colors hover:after:bg-foreground/25 data-[separator=active]:after:bg-primary focus-visible:ring-0 focus-visible:after:bg-ring" />
  );
}

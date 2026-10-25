import { useState } from "react";
import type { Backend } from "./core";
import { History } from "./components/History";
import { Library } from "./components/Library";
import { ScrobbLink } from "./components/ScrobbLink";
import { Transport } from "./components/Transport";
import { usePlayer } from "./usePlayer";

export function App({ backend }: { backend: Backend }) {
  const p = usePlayer(backend);
  // На узком окне библиотека и история — вкладками, на широком — рядом.
  const [tab, setTab] = useState<"library" | "history">("library");
  const currentId = p.state.current?.id ?? null;

  return (
    <div className="app" data-tab={tab}>
      <header className="topbar">
        <div className="brand">Scrobb Player</div>
        <nav className="tabs" aria-label="Разделы">
          <button aria-pressed={tab === "library"} onClick={() => setTab("library")}>
            Библиотека
          </button>
          <button aria-pressed={tab === "history"} onClick={() => setTab("history")}>
            История
          </button>
        </nav>
        <div className="scrobb">
          <ScrobbLink
            status={p.scrobbling}
            available={p.scrobblingAvailable}
            code={p.linkCode}
            run={p.run}
            openUrl={(url) => void backend.openUrl(url)}
          />
        </div>
      </header>
      <main className="content">
        <Library
          tracks={p.tracks}
          folders={p.folders}
          scanning={p.scanning}
          currentId={currentId}
          status={p.state.status}
          onAddFiles={p.addFiles}
          onAddFolder={p.addFolder}
          onRemoveFolder={p.removeFolder}
          onRescan={p.rescan}
          onPlay={(start) => p.run({ type: "play_queue", data: { tracks: p.tracks.map((t) => t.id), start } })}
        />
        <History plays={p.history} />
      </main>
      <Transport state={p.state} receivedAt={p.receivedAt} run={p.run} />
      <div className="toasts" aria-live="polite">
        {p.toasts.map((t) => (
          <div key={t.id} className="toast">
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}

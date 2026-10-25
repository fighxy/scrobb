import { ChevronDown, MoreHorizontal, X } from "lucide-react";
import { AnimatePresence, m } from "motion/react";
import type { CSSProperties } from "react";
import { useApp } from "@/app-context";
import { Cover, Equalizer, IconButton, InlineLink } from "@/components/common";
import { Controls, Progress } from "@/components/PlayerBar";
import { AddToPlaylist } from "@/components/playlist-ui";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import type { Command, Track } from "@/core";
import { artists } from "@/format";
import { albumKey, artistKey, coverOf, primaryArtist } from "@/lib/library";
import { useCoverColor } from "@/usePlayer";

/** Сколько треков «Далее» показывать: дальше — уже не «скоро». */
const UPCOMING = 30;

/** Большая обложка; при смене альбома — плавная смена картинки. */
function BigCover({ track }: { track: Track }) {
  const app = useApp();
  const id = coverOf(app.lib, track);
  return (
    <div className="relative aspect-square w-full overflow-hidden rounded-lg shadow-2xl shadow-black/40">
      <AnimatePresence initial={false}>
        <m.div
          key={id}
          className="absolute inset-0"
          initial={{ opacity: 0, scale: 1.03 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.25, ease: [0.2, 0.8, 0.2, 1] }}
        >
          <Cover src={app.backend.coverUrl(id, "l")} className="size-full rounded-none" />
        </m.div>
      </AnimatePresence>
    </div>
  );
}

function TrackTitle({ track, large }: { track: Track; large?: boolean }) {
  const app = useApp();
  const album = albumKey(track);
  const artist = primaryArtist(track);
  return (
    <div className="flex items-start gap-2">
      <div className="min-w-0 flex-1">
        <div className={large ? "truncate text-2xl font-bold" : "truncate text-xl font-bold"}>
          <InlineLink onClick={album ? () => app.go({ kind: "album", key: album }) : undefined}>{track.title}</InlineLink>
        </div>
        <div className="truncate text-muted-foreground">
          <InlineLink onClick={artist ? () => app.go({ kind: "artist", key: artistKey(artist) }) : undefined}>{artists(track.artists)}</InlineLink>
        </div>
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <IconButton label="Ещё о треке" className="mt-0.5 shrink-0">
            <MoreHorizontal />
          </IconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-60">
          <AddToPlaylist kind="dropdown" tracks={[track.id]} />
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

/** «Далее»: треки очереди после текущего. Щелчок — играть с него. */
function UpNext() {
  const app = useApp();
  const [index, length] = app.state.queue_position ?? [-1, 0];
  if (index < 0 || length === 0) return null;
  const byId = new Map(app.tracks.map((t) => [t.id, t]));
  const upcoming = app.queue
    .slice(index + 1, index + 1 + UPCOMING)
    .map((id, i) => ({ track: byId.get(id), at: index + 1 + i }))
    .filter((x): x is { track: Track; at: number } => !!x.track);
  const queueTracks = app.queue.map((id) => byId.get(id)).filter((t): t is Track => !!t);
  return (
    <section aria-label="Далее" className="rounded-lg bg-surface-raised/70 p-3">
      <h3 className="mb-2 px-1 font-semibold">Далее</h3>
      {upcoming.length === 0 ? (
        <p className="px-1 text-[13px] text-muted-foreground">
          {app.state.repeat === "all" ? "Дальше очередь начнётся сначала." : "Это последний трек очереди."}
        </p>
      ) : (
        <ol className="flex flex-col">
          {upcoming.map(({ track, at }) => (
            <li key={`${at}-${track.id}`}>
              <button
                type="button"
                onClick={() =>
                  // Обычно все треки очереди в библиотеке; если какой-то убрали — ищем по id.
                  app.play(queueTracks, queueTracks.length === app.queue.length ? at : queueTracks.findIndex((t) => t.id === track.id))
                }
                className="flex w-full items-center gap-3 rounded-md p-1.5 text-left hover:bg-surface-hover"
              >
                <Cover src={app.cover(track)} className="size-10 rounded" />
                <span className="min-w-0">
                  <span className="block truncate text-[13px] font-medium">{track.title}</span>
                  <span className="block truncate text-xs text-muted-foreground">{artists(track.artists)}</span>
                </span>
              </button>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

/** Правая панель «Сейчас играет»: обложка, трек, очередь. */
export function NowPlayingPanel({ onClose }: { onClose: () => void }) {
  const app = useApp();
  const track = app.state.current;
  const tint = useCoverColor(app.backend, track ? coverOf(app.lib, track) : null);
  return (
    <aside
      aria-label="Сейчас играет"
      className="tinted flex h-full min-h-0 flex-col overflow-hidden rounded-lg bg-surface"
      style={{ "--tint": tint ?? "var(--surface-raised)" } as CSSProperties}
    >
      <div className="flex h-14 shrink-0 items-center justify-between pr-2 pl-4">
        <h2 className="flex items-center gap-2 font-bold">
          {track && app.state.status !== "stopped" && <Equalizer paused={app.state.status !== "playing"} />}
          Сейчас играет
        </h2>
        <IconButton label="Скрыть" onClick={onClose}>
          <X />
        </IconButton>
      </div>
      <div className="tinted-fade min-h-0 flex-1 overflow-y-auto px-4 pb-4">
        {track ? (
          <div className="flex flex-col gap-4">
            <BigCover track={track} />
            <TrackTitle track={track} />
            <UpNext />
          </div>
        ) : (
          <p className="pt-8 text-center text-muted-foreground">Двойной щелчок по треку — и он появится здесь.</p>
        )}
      </div>
    </aside>
  );
}

/** «Сейчас играет» на всё окно — для узкого окна. */
export function NowPlayingFullscreen({
  open,
  onClose,
  receivedAt,
  run,
}: {
  open: boolean;
  onClose: () => void;
  receivedAt: number;
  run: (c: Command) => void;
}) {
  const app = useApp();
  const track = app.state.current;
  const tint = useCoverColor(app.backend, track ? coverOf(app.lib, track) : null);
  return (
    <AnimatePresence>
      {open && (
        <m.div
          role="dialog"
          aria-label="Сейчас играет"
          className="tinted fixed inset-0 z-40 flex flex-col overflow-y-auto bg-surface"
          style={{ "--tint": tint ?? "var(--surface-raised)" } as CSSProperties}
          initial={{ y: "100%" }}
          animate={{ y: 0 }}
          exit={{ y: "100%" }}
          transition={{ type: "spring", damping: 32, stiffness: 320 }}
        >
          <div className="tinted-header flex min-h-full flex-col gap-6 px-6 pt-3 pb-8">
            <div className="flex items-center justify-between">
              <IconButton label="Свернуть" onClick={onClose} className="size-10">
                <ChevronDown />
              </IconButton>
              <span className="text-xs font-semibold tracking-wide uppercase">Сейчас играет</span>
              <span className="size-10" />
            </div>
            {track ? (
              <>
                <div className="mx-auto w-full max-w-[min(100%,420px)]">
                  <BigCover track={track} />
                </div>
                <div className="mx-auto flex w-full max-w-[420px] flex-col gap-4">
                  <TrackTitle track={track} large />
                  <Progress state={app.state} receivedAt={receivedAt} run={run} />
                  <div className="flex justify-center">
                    <Controls state={app.state} run={run} large />
                  </div>
                  <UpNext />
                </div>
              </>
            ) : (
              <p className="pt-16 text-center text-muted-foreground">Ничего не играет.</p>
            )}
          </div>
        </m.div>
      )}
    </AnimatePresence>
  );
}

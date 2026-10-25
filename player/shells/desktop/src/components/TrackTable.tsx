import { useVirtualizer } from "@tanstack/react-virtual";
import { Clock3, Disc3, FolderOpen, Pause, Play, UserRound } from "lucide-react";
import { useRef, useState } from "react";
import { useApp } from "@/app-context";
import { Cover, Equalizer, InlineLink } from "@/components/common";
import { useScrollElement, useScrollMargin } from "@/components/Page";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { Track } from "@/core";
import { artists, duration } from "@/format";
import { albumKey, artistKey, primaryArtist } from "@/lib/library";
import { cn } from "@/lib/utils";

interface Props {
  tracks: Track[];
  /** Номер в альбоме вместо позиции в списке; без колонки альбома и обложек. */
  albumMode?: boolean;
}

const ROW = 56;

/**
 * Таблица треков: виртуализирована (в окне только видимые строки), поэтому
 * и десятки тысяч треков не тормозят. Прокручивается не таблица, а страница
 * целиком: шапка уезжает вместе со списком. Щелчок выделяет, двойной щелчок или
 * Enter — играть, стрелки — по строкам, правая кнопка — меню.
 */
export function TrackTable({ tracks, albumMode }: Props) {
  const app = useApp();
  const list = useRef<HTMLDivElement>(null);
  const scroller = useScrollElement();
  const margin = useScrollMargin(list);
  const [selected, setSelected] = useState<number | null>(null);

  const virtual = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => scroller,
    estimateSize: () => ROW,
    overscan: 10,
    scrollMargin: margin,
  });

  const current = app.state.current?.id;
  const active = app.isActive(tracks);
  const status = app.state.status;

  const focusRow = (index: number) => {
    const i = Math.max(0, Math.min(tracks.length - 1, index));
    setSelected(i);
    virtual.scrollToIndex(i, { align: "auto" });
    requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-index="${i}"]`)?.focus());
  };

  return (
    <div className="@container" role="grid" aria-rowcount={tracks.length} aria-label="Треки">
      <div
        role="row"
        className={cn(
          "sticky top-14 z-10 grid h-9 items-center gap-4 border-b bg-surface px-4 text-xs text-muted-foreground",
          albumMode ? "grid-cols-[32px_minmax(0,1fr)_56px]" : "grid-cols-[32px_minmax(0,1fr)_56px] @xl:grid-cols-[32px_minmax(0,6fr)_minmax(0,4fr)_56px]",
        )}
      >
        <span className="text-right">#</span>
        <span>Название</span>
        {!albumMode && <span className="hidden @xl:block">Альбом</span>}
        <span className="flex justify-end pr-1" aria-label="Длительность">
          <Clock3 className="size-4" />
        </span>
      </div>
      <div ref={list} className="relative px-2 pt-2" style={{ height: virtual.getTotalSize() + 16 }}>
        {virtual.getVirtualItems().map((item) => {
          const t = tracks[item.index];
          const isCurrent = t.id === current && status !== "stopped";
          const playingHere = isCurrent && active && (status === "playing" || status === "buffering");
          const album = albumKey(t);
          const artist = primaryArtist(t);
          const playOrToggle = () => (isCurrent && active ? app.toggle() : app.play(tracks, item.index));
          return (
            <ContextMenu key={t.id}>
              <ContextMenuTrigger asChild>
                <div
                  role="row"
                  data-index={item.index}
                  tabIndex={0}
                  aria-selected={selected === item.index}
                  onClick={() => setSelected(item.index)}
                  onDoubleClick={() => app.play(tracks, item.index)}
                  onContextMenu={() => setSelected(item.index)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") app.play(tracks, item.index);
                    else if (e.key === "ArrowDown") focusRow(item.index + 1);
                    else if (e.key === "ArrowUp") focusRow(item.index - 1);
                    else return;
                    e.preventDefault();
                  }}
                  className={cn(
                    "group absolute inset-x-2 grid items-center gap-4 rounded-md px-2 outline-none transition-colors hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:ring-2 focus-visible:ring-ring",
                    albumMode ? "grid-cols-[32px_minmax(0,1fr)_56px]" : "grid-cols-[32px_minmax(0,1fr)_56px] @xl:grid-cols-[32px_minmax(0,6fr)_minmax(0,4fr)_56px]",
                    selected === item.index && "bg-surface-raised hover:bg-surface-raised",
                  )}
                  style={{ height: ROW, transform: `translateY(${item.start - margin}px)` }}
                >
                  <span className="relative flex h-full items-center justify-end text-muted-foreground tabular-nums">
                    <span className={cn("group-hover:invisible", isCurrent && "text-primary")}>
                      {isCurrent ? (
                        <Equalizer paused={!playingHere} />
                      ) : albumMode ? (
                        (t.track_no ?? item.index + 1)
                      ) : (
                        item.index + 1
                      )}
                    </span>
                    <button
                      type="button"
                      tabIndex={-1}
                      aria-label={playingHere ? "Пауза" : `Играть «${t.title}»`}
                      onClick={(e) => {
                        e.stopPropagation();
                        playOrToggle();
                      }}
                      className="invisible absolute right-0 grid size-6 place-items-center text-foreground group-hover:visible"
                    >
                      {playingHere ? <Pause className="size-4" fill="currentColor" /> : <Play className="size-4" fill="currentColor" />}
                    </button>
                  </span>
                  <span className="flex min-w-0 items-center gap-3">
                    {!albumMode && <Cover src={app.cover(t)} className="size-10 rounded" />}
                    <span className="flex min-w-0 flex-col">
                      <span className={cn("truncate font-medium", isCurrent && "text-primary")}>{t.title}</span>
                      <span className="truncate text-[13px] text-muted-foreground">
                        <InlineLink onClick={artist ? () => app.go({ kind: "artist", key: artistKey(artist) }) : undefined}>
                          {artists(t.artists)}
                        </InlineLink>
                      </span>
                    </span>
                  </span>
                  {!albumMode && (
                    <span className="hidden min-w-0 truncate text-[13px] text-muted-foreground @xl:block">
                      <InlineLink onClick={album ? () => app.go({ kind: "album", key: album }) : undefined}>{t.album ?? ""}</InlineLink>
                    </span>
                  )}
                  <span className="pr-1 text-right text-[13px] text-muted-foreground tabular-nums">{duration(t.duration_ms)}</span>
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent className="w-56">
                <ContextMenuItem onSelect={playOrToggle}>
                  {playingHere ? <Pause /> : <Play />}
                  {playingHere ? "Пауза" : "Играть"}
                </ContextMenuItem>
                {album && !albumMode && (
                  <ContextMenuItem onSelect={() => app.go({ kind: "album", key: album })}>
                    <Disc3 />К альбому
                  </ContextMenuItem>
                )}
                {artist && (
                  <ContextMenuItem onSelect={() => app.go({ kind: "artist", key: artistKey(artist) })}>
                    <UserRound />К артисту
                  </ContextMenuItem>
                )}
                <ContextMenuSeparator />
                <ContextMenuItem onSelect={() => app.reveal(t.uri)}>
                  <FolderOpen />
                  Показать в папке
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          );
        })}
      </div>
    </div>
  );
}

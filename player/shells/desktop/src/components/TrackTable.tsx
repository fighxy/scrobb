import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp, Clock3, Disc3, FolderOpen, ListX, Pause, Play, UserRound } from "lucide-react";
import { useRef, useState } from "react";
import { useApp } from "@/app-context";
import { Cover, Equalizer, InlineLink } from "@/components/common";
import { useScrollElement, useScrollMargin } from "@/components/Page";
import { AddToPlaylist } from "@/components/playlist-ui";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { Track } from "@/core";
import { artists, duration } from "@/format";
import { albumKey, artistKey, primaryArtist } from "@/lib/library";
import { cn } from "@/lib/utils";

/** Перетаскиваемые треки (id библиотеки) — для плейлистов в левой панели. */
export const DRAG_TRACKS = "application/x-scrobb-tracks";
/** Перетаскиваемая строка плейлиста — для перестановки. */
const DRAG_ROW = "application/x-scrobb-row";

interface PlaylistRows {
  /** Ключи строк: один трек может стоять в плейлисте дважды. */
  keys: string[];
  onRemove: (index: number) => void;
  /** Переставить строку `from` так, чтобы она стала `to`-й. */
  onMove: (from: number, to: number) => void;
}

interface Props {
  tracks: Track[];
  /** Номер в альбоме вместо позиции в списке; без колонки альбома и обложек. */
  albumMode?: boolean;
  /** Строки, которых нет в библиотеке: показываются бледно и не играют. */
  unavailable?: (index: number) => boolean;
  /** Строки плейлиста: убрать, переставить. */
  playlist?: PlaylistRows;
}

const ROW = 56;
/** Отступ списка сверху (`pt-2`). */
const TOP = 8;

/**
 * Таблица треков: виртуализирована (в окне только видимые строки), поэтому
 * и десятки тысяч треков не тормозят. Прокручивается не таблица, а страница
 * целиком: шапка уезжает вместе со списком. Щелчок выделяет, двойной щелчок или
 * Enter — играть, стрелки — по строкам, правая кнопка — меню. Треки можно
 * перетащить в плейлист слева, строки плейлиста — переставить.
 */
export function TrackTable({ tracks, albumMode, unavailable, playlist }: Props) {
  const app = useApp();
  const list = useRef<HTMLDivElement>(null);
  const scroller = useScrollElement();
  const margin = useScrollMargin(list);
  const [selected, setSelected] = useState<number | null>(null);
  const [dropAt, setDropAt] = useState<number | null>(null);

  const virtual = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => scroller,
    estimateSize: () => ROW,
    overscan: 10,
    scrollMargin: margin,
  });

  // Очередь — только из того, что можно сыграть.
  const missing = (i: number) => unavailable?.(i) ?? false;
  const playable = tracks.filter((_, i) => !missing(i));
  const playIndex = (row: number) => tracks.slice(0, row).filter((_, i) => !missing(i)).length;
  const play = (row: number) => !missing(row) && app.play(playable, playIndex(row));

  const current = app.state.current?.id;
  const active = app.isActive(playable);
  const status = app.state.status;

  const focusRow = (index: number) => {
    const i = Math.max(0, Math.min(tracks.length - 1, index));
    setSelected(i);
    virtual.scrollToIndex(i, { align: "auto" });
    requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-index="${i}"]`)?.focus());
  };

  const move = (from: number, to: number) => {
    if (!playlist || to < 0 || to >= tracks.length || from === to) return;
    playlist.onMove(from, to);
    focusRow(to);
  };

  /** Куда упадёт строка: перед строкой под курсором или после неё. */
  const dropIndex = (e: React.DragEvent) => {
    const top = list.current?.getBoundingClientRect().top ?? 0;
    return Math.max(0, Math.min(tracks.length, Math.round((e.clientY - top - TOP) / ROW)));
  };

  const columns = albumMode
    ? "grid-cols-[32px_minmax(0,1fr)_56px]"
    : "grid-cols-[32px_minmax(0,1fr)_56px] @xl:grid-cols-[32px_minmax(0,6fr)_minmax(0,4fr)_56px]";

  return (
    <div className="@container" role="grid" aria-rowcount={tracks.length} aria-label="Треки">
      <div role="row" className={cn("sticky top-14 z-10 grid h-9 items-center gap-4 border-b bg-surface px-4 text-xs text-muted-foreground", columns)}>
        <span className="text-right">#</span>
        <span>Название</span>
        {!albumMode && <span className="hidden @xl:block">Альбом</span>}
        <span className="flex justify-end pr-1" aria-label="Длительность">
          <Clock3 className="size-4" />
        </span>
      </div>
      <div
        ref={list}
        className="relative px-2 pt-2"
        style={{ height: virtual.getTotalSize() + 16 }}
        onDragOver={(e) => {
          if (!playlist || !e.dataTransfer.types.includes(DRAG_ROW)) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          setDropAt(dropIndex(e));
        }}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropAt(null);
        }}
        onDrop={(e) => {
          const raw = e.dataTransfer.getData(DRAG_ROW);
          setDropAt(null);
          if (!playlist || raw === "") return;
          e.preventDefault();
          const from = Number(raw);
          const at = dropIndex(e);
          move(from, at > from ? at - 1 : at);
        }}
      >
        {dropAt !== null && (
          <div
            className="pointer-events-none absolute inset-x-4 z-10 h-0.5 rounded-full bg-primary"
            style={{ top: TOP + dropAt * ROW - 1 }}
            aria-hidden
          />
        )}
        {virtual.getVirtualItems().map((item) => {
          const i = item.index;
          const t = tracks[i];
          const gone = missing(i);
          const isCurrent = !gone && t.id === current && status !== "stopped";
          const playingHere = isCurrent && active && (status === "playing" || status === "buffering");
          const album = gone ? null : albumKey(t);
          const artist = gone ? null : primaryArtist(t);
          const playOrToggle = () => (isCurrent && active ? app.toggle() : play(i));
          return (
            <ContextMenu key={playlist?.keys[i] ?? t.id}>
              <ContextMenuTrigger asChild>
                <div
                  role="row"
                  data-index={i}
                  tabIndex={0}
                  aria-selected={selected === i}
                  aria-disabled={gone || undefined}
                  title={gone ? "Нет в библиотеке: файла с этим треком нет на этом компьютере" : undefined}
                  draggable={!gone || !!playlist}
                  onDragStart={(e) => {
                    if (!gone) e.dataTransfer.setData(DRAG_TRACKS, JSON.stringify([t.id]));
                    if (playlist) e.dataTransfer.setData(DRAG_ROW, String(i));
                    e.dataTransfer.effectAllowed = "copyMove";
                  }}
                  onDragEnd={() => setDropAt(null)}
                  onClick={() => setSelected(i)}
                  onDoubleClick={() => play(i)}
                  onContextMenu={() => setSelected(i)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") play(i);
                    else if (e.key === "ArrowDown" && e.altKey) move(i, i + 1);
                    else if (e.key === "ArrowUp" && e.altKey) move(i, i - 1);
                    else if (e.key === "ArrowDown") focusRow(i + 1);
                    else if (e.key === "ArrowUp") focusRow(i - 1);
                    else if (e.key === "Delete" && playlist) playlist.onRemove(i);
                    else return;
                    e.preventDefault();
                  }}
                  className={cn(
                    "group absolute inset-x-2 grid items-center gap-4 rounded-md px-2 outline-none transition-colors hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:ring-2 focus-visible:ring-ring",
                    columns,
                    selected === i && "bg-surface-raised hover:bg-surface-raised",
                  )}
                  style={{ height: ROW, transform: `translateY(${item.start - margin}px)` }}
                >
                  <span className={cn("relative flex h-full items-center justify-end text-muted-foreground tabular-nums", gone && "opacity-50")}>
                    <span className={cn(!gone && "group-hover:invisible", isCurrent && "text-primary")}>
                      {isCurrent ? <Equalizer paused={!playingHere} /> : albumMode ? (t.track_no ?? i + 1) : i + 1}
                    </span>
                    {!gone && (
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
                    )}
                  </span>
                  <span className={cn("flex min-w-0 items-center gap-3", gone && "opacity-50")}>
                    {!albumMode && <Cover src={app.cover(t)} className="size-10 rounded" />}
                    <span className="flex min-w-0 flex-col">
                      <span className={cn("truncate font-medium", isCurrent && "text-primary")}>{t.title}</span>
                      <span className="truncate text-[13px] text-muted-foreground">
                        {gone && <span className="mr-1.5 rounded-sm bg-surface-raised px-1 text-[11px]">нет в библиотеке</span>}
                        <InlineLink onClick={artist ? () => app.go({ kind: "artist", key: artistKey(artist) }) : undefined}>
                          {artists(t.artists)}
                        </InlineLink>
                      </span>
                    </span>
                  </span>
                  {!albumMode && (
                    <span className={cn("hidden min-w-0 truncate text-[13px] text-muted-foreground @xl:block", gone && "opacity-50")}>
                      <InlineLink onClick={album ? () => app.go({ kind: "album", key: album }) : undefined}>{t.album ?? ""}</InlineLink>
                    </span>
                  )}
                  <span className={cn("pr-1 text-right text-[13px] text-muted-foreground tabular-nums", gone && "opacity-50")}>
                    {duration(t.duration_ms)}
                  </span>
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent className="w-60">
                {!gone && (
                  <ContextMenuItem onSelect={playOrToggle}>
                    {playingHere ? <Pause /> : <Play />}
                    {playingHere ? "Пауза" : "Играть"}
                  </ContextMenuItem>
                )}
                {!gone && <AddToPlaylist kind="context" tracks={[t.id]} />}
                {playlist && (
                  <>
                    <ContextMenuSeparator />
                    <ContextMenuItem onSelect={() => playlist.onRemove(i)}>
                      <ListX />
                      Убрать из плейлиста
                      <ContextMenuShortcut>Del</ContextMenuShortcut>
                    </ContextMenuItem>
                    <ContextMenuItem disabled={i === 0} onSelect={() => move(i, i - 1)}>
                      <ArrowUp />
                      Выше
                      <ContextMenuShortcut>Alt ↑</ContextMenuShortcut>
                    </ContextMenuItem>
                    <ContextMenuItem disabled={i === tracks.length - 1} onSelect={() => move(i, i + 1)}>
                      <ArrowDown />
                      Ниже
                      <ContextMenuShortcut>Alt ↓</ContextMenuShortcut>
                    </ContextMenuItem>
                  </>
                )}
                {(album || artist) && <ContextMenuSeparator />}
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
                {!gone && (
                  <>
                    <ContextMenuSeparator />
                    <ContextMenuItem onSelect={() => app.reveal(t.uri)}>
                      <FolderOpen />
                      Показать в папке
                    </ContextMenuItem>
                  </>
                )}
              </ContextMenuContent>
            </ContextMenu>
          );
        })}
      </div>
    </div>
  );
}

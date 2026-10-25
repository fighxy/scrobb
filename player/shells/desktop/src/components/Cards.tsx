import { useVirtualizer } from "@tanstack/react-virtual";
import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import { useApp } from "@/app-context";
import { Cover, PlayButton } from "@/components/common";
import { useScrollElement, useScrollMargin } from "@/components/Page";
import { ALBUMS, count } from "@/format";
import type { Album, Artist } from "@/lib/library";
import { cn } from "@/lib/utils";

const GAP = 8;
/** Высота подписи под обложкой плитки и внутренние отступы. */
const CAPTION = 64;

/**
 * Сетка плиток, виртуализированная по рядам: столбцов столько, сколько
 * влезает при ширине плитки не меньше `minWidth`.
 */
export function CardGrid<T>({
  items,
  render,
  minWidth = 176,
}: {
  items: T[];
  render: (item: T) => ReactNode;
  minWidth?: number;
}) {
  const box = useRef<HTMLDivElement>(null);
  const scroller = useScrollElement();
  const margin = useScrollMargin(box);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, []);
  const cols = Math.max(2, Math.floor((width + GAP) / (minWidth + GAP)));
  const rowHeight = Math.max(1, (width - GAP * (cols - 1)) / cols) - 24 + CAPTION + GAP;
  const rows = Math.ceil(items.length / cols);
  const virtual = useVirtualizer({
    count: rows,
    getScrollElement: () => scroller,
    estimateSize: () => rowHeight,
    overscan: 3,
    scrollMargin: margin,
  });
  // Ширина сменилась — сменилась и высота ряда.
  useLayoutEffect(() => virtual.measure(), [virtual, rowHeight]);

  return (
    <div ref={box} className="relative mx-4" style={{ height: rows * rowHeight }}>
      {width > 0 &&
        virtual.getVirtualItems().map((row) => (
          <div
            key={row.index}
            className="absolute inset-x-0 grid"
            style={{
              gap: GAP,
              gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
              transform: `translateY(${row.index * rowHeight}px)`,
            }}
          >
            {items.slice(row.index * cols, row.index * cols + cols).map(render)}
          </div>
        ))}
    </div>
  );
}

function Card({
  cover,
  title,
  subtitle,
  round,
  label,
  onOpen,
  onPlay,
  playing,
}: {
  cover: string;
  title: string;
  subtitle: string;
  round?: boolean;
  /** Подпись для экранного чтеца: «Альбом „Группа крови“». */
  label: string;
  onOpen: () => void;
  onPlay: () => void;
  playing: boolean;
}) {
  // Открывает плитку растянутая на всю площадь кнопка, а «играть» лежит
  // поверх неё, а не внутри: вложенные кнопки недоступны с клавиатуры.
  return (
    <div className="group relative min-w-0 rounded-lg p-3 transition-colors hover:bg-surface-hover has-[>button:focus-visible]:ring-2 has-[>button:focus-visible]:ring-ring" title={title}>
      <button type="button" aria-label={label} onClick={onOpen} className="absolute inset-0 rounded-lg outline-none" />
      <div className="pointer-events-none relative">
        <Cover src={cover} round={round} className={cn("aspect-square w-full shadow-lg shadow-black/30", !round && "rounded-md")} />
        <PlayButton
          size="md"
          playing={playing}
          onClick={onPlay}
          className={cn(
            "pointer-events-auto absolute right-2 bottom-2 translate-y-2 opacity-0 group-focus-within:translate-y-0 group-focus-within:opacity-100 group-hover:translate-y-0 group-hover:opacity-100",
            playing && "translate-y-0 opacity-100",
          )}
        />
      </div>
      <div className={cn("pointer-events-none relative mt-2 truncate font-semibold", round && "text-center")}>{title}</div>
      <div className={cn("pointer-events-none relative truncate text-[13px] text-muted-foreground", round && "text-center")}>{subtitle}</div>
    </div>
  );
}

export function AlbumCard({ album }: { album: Album }) {
  const app = useApp();
  const active = app.isActive(album.tracks);
  const playing = active && app.state.status === "playing";
  return (
    <Card
      cover={app.backend.coverUrl(album.cover, "s")}
      title={album.title}
      subtitle={album.artist || "Неизвестный артист"}
      label={`Альбом «${album.title}»`}
      onOpen={() => app.go({ kind: "album", key: album.key })}
      onPlay={() => (active ? app.toggle() : app.play(album.tracks, 0))}
      playing={playing}
    />
  );
}

export function ArtistCard({ artist }: { artist: Artist }) {
  const app = useApp();
  const active = app.isActive(artist.tracks);
  const playing = active && app.state.status === "playing";
  return (
    <Card
      round
      cover={app.backend.coverUrl(artist.cover, "s")}
      title={artist.name}
      subtitle={artist.albums.length ? count(artist.albums.length, ALBUMS) : "Артист"}
      label={`Артист «${artist.name}»`}
      onOpen={() => app.go({ kind: "artist", key: artist.key })}
      onPlay={() => (active ? app.toggle() : app.play(artist.tracks, 0))}
      playing={playing}
    />
  );
}

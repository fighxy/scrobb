import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import type { PlaybackStatus, Track, TrackId } from "../core";
import { artists, duration } from "../format";
import { Icon } from "./icons";

interface Props {
  tracks: Track[];
  currentId: TrackId | null;
  status: PlaybackStatus;
  onPlay: (index: number) => void;
  onAdd: () => void;
}

const ROW = 40;

/** Список треков: виртуализирован, поэтому и сто тысяч строк не тормозят окно. */
export function Library({ tracks, currentId, status, onPlay, onAdd }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW,
    overscan: 12,
  });

  return (
    <section className="panel library" aria-label="Библиотека">
      <header className="panel-head">
        <h2>Библиотека</h2>
        <span className="muted">{tracks.length ? `${tracks.length} треков` : ""}</span>
        <button className="button" onClick={onAdd}>
          <Icon name="plus" size={18} /> Добавить файлы
        </button>
      </header>
      {tracks.length === 0 ? (
        <div className="empty">
          <p>Здесь будет ваша музыка.</p>
          <p className="muted">Добавьте файлы — FLAC, MP3, AAC, Ogg, WAV, ALAC.</p>
          <button className="button primary" onClick={onAdd}>
            Добавить файлы
          </button>
        </div>
      ) : (
        <>
          <div className="row head" role="row">
            <span className="col-no">#</span>
            <span>Название</span>
            <span className="col-artist">Артист</span>
            <span className="col-album">Альбом</span>
            <span className="col-time">Время</span>
          </div>
          <div className="rows" ref={scroller} role="grid" aria-rowcount={tracks.length}>
            <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
              {virtual.getVirtualItems().map((item) => {
                const t = tracks[item.index];
                // После остановки трек остаётся текущим (с него начнётся «играть»), но без значка.
                const current = t.id === currentId && status !== "stopped";
                return (
                  <div
                    key={t.id}
                    role="row"
                    tabIndex={0}
                    aria-selected={current}
                    className={`row${current ? " current" : ""}`}
                    style={{ position: "absolute", top: 0, left: 0, right: 0, height: ROW, transform: `translateY(${item.start}px)` }}
                    onDoubleClick={() => onPlay(item.index)}
                    onKeyDown={(e) => e.key === "Enter" && onPlay(item.index)}
                    title="Двойной щелчок — играть"
                  >
                    <span className="col-no muted">
                      {current ? (status === "paused" ? "❚❚" : "▶") : item.index + 1}
                    </span>
                    <span className="ellipsis title">{t.title}</span>
                    <span className="ellipsis col-artist muted">{artists(t.artists)}</span>
                    <span className="ellipsis col-album muted">{t.album ?? ""}</span>
                    <span className="col-time muted">{duration(t.duration_ms)}</span>
                  </div>
                );
              })}
            </div>
          </div>
        </>
      )}
    </section>
  );
}

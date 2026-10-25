import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import type { PlaybackStatus, Scanning, Track, TrackId } from "../core";
import { artists, duration, folderName } from "../format";
import { Icon } from "./icons";

interface Props {
  tracks: Track[];
  folders: string[];
  scanning: Scanning | null;
  currentId: TrackId | null;
  status: PlaybackStatus;
  onPlay: (index: number) => void;
  onAddFiles: () => void;
  onAddFolder: () => void;
  onRemoveFolder: (path: string) => void;
  onRescan: () => void;
}

const ROW = 40;

/** Список треков: виртуализирован, поэтому и сто тысяч строк не тормозят окно. */
export function Library(props: Props) {
  const { tracks, folders, scanning, currentId, status, onPlay, onAddFiles, onAddFolder } = props;
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
        <div className="head-actions">
          <button className="button" onClick={onAddFolder} title="Добавить папку" aria-label="Добавить папку">
            <Icon name="folder" size={18} /> <span className="label">Добавить папку</span>
          </button>
          <button className="button" onClick={onAddFiles} title="Добавить файлы" aria-label="Добавить файлы">
            <Icon name="plus" size={18} /> <span className="label">Файлы</span>
          </button>
        </div>
      </header>
      {(folders.length > 0 || scanning) && <Folders {...props} />}
      {tracks.length === 0 ? (
        <div className="empty">
          {scanning ? (
            <p>Ищу музыку в папке «{folderName(scanning.folder)}»…</p>
          ) : (
            <>
              <p>Здесь будет ваша музыка.</p>
              <p className="muted">
                Добавьте папку — плеер найдёт в ней все треки (FLAC, MP3, AAC, ALAC, Ogg, WAV) и запомнит её.
              </p>
              <div className="empty-actions">
                <button className="button primary" onClick={onAddFolder}>
                  Добавить папку
                </button>
                <button className="button" onClick={onAddFiles}>
                  или отдельные файлы
                </button>
              </div>
            </>
          )}
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

/** Папки-источники: имя, полный путь в подсказке, «убрать»; ход сканирования. */
function Folders({ folders, scanning, onRemoveFolder, onRescan }: Props) {
  return (
    <div className="folders" aria-label="Папки библиотеки">
      {folders.map((f) => (
        <span key={f} className="folder" title={f}>
          <Icon name="folder" size={14} />
          {folderName(f)}
          <button className="remove" aria-label={`Убрать папку ${folderName(f)}`} title="Убрать из библиотеки" onClick={() => onRemoveFolder(f)}>
            <Icon name="close" size={12} />
          </button>
        </span>
      ))}
      {scanning ? (
        <span className="scanning muted" role="status">
          <span className="spinner" aria-hidden="true" />
          {scanning.read > 0
            ? `«${folderName(scanning.folder)}»: прочитано ${scanning.read} из ${scanning.found}`
            : `«${folderName(scanning.folder)}»: найдено ${scanning.found}`}
        </span>
      ) : (
        <button className="link" onClick={onRescan} title="Найти новые и удалённые файлы">
          Обновить
        </button>
      )}
    </div>
  );
}

// Плейлисты в окне: список, версия для перечитывания открытого плейлиста
// и действия с уведомлениями. Ядро хранит и связывает с библиотекой, окно
// только показывает.

import { useCallback, useEffect, useState } from "react";
import type { Backend, ItemId, Playlist, PlaylistCard, PlaylistFormat, PlaylistId, Track, TrackId } from "./core";
import { describeError, EXPORT_FORMATS } from "./core";
import { count, TRACKS } from "./format";

export function usePlaylists(backend: Backend, toast: (text: string) => void, tracks: Track[]) {
  const [playlists, setPlaylists] = useState<PlaylistCard[]>([]);
  // Растёт при каждой правке: открытый плейлист перечитывается.
  const [version, setVersion] = useState(0);

  const refresh = useCallback(() => {
    setVersion((v) => v + 1);
    backend.playlists().then(setPlaylists, (e) => toast(describeError(e)));
  }, [backend, toast]);

  // Библиотека изменилась — треки плейлистов могли найтись или пропасть.
  useEffect(refresh, [refresh, tracks]);

  // Импорт из командной строки («Открыть с помощью» для .m3u8) и правки,
  // пришедшие с других устройств.
  useEffect(() => {
    const stopLibrary = backend.onLibraryEvent((event) => {
      if (event.type === "playlists") refresh();
    });
    const stopCore = backend.onEvent((event) => {
      if (event.type === "playlists_changed") refresh();
    });
    return () => {
      stopLibrary.then((f) => f());
      stopCore.then((f) => f());
    };
  }, [backend, refresh]);

  /** Правка: выполнить, перечитать, ошибку — уведомлением. */
  const edit = useCallback(
    async <T,>(action: () => Promise<T>): Promise<T | null> => {
      try {
        return await action();
      } catch (e) {
        toast(describeError(e));
        return null;
      } finally {
        refresh();
      }
    },
    [refresh, toast],
  );

  const title = useCallback((id: PlaylistId) => playlists.find((p) => p.playlist.id === id)?.playlist.title ?? "плейлист", [playlists]);

  return {
    playlists,
    version,
    refresh,
    /** Новый плейлист; название по умолчанию — «Плейлист №N». */
    create: useCallback(
      (tracks: TrackId[] = [], name?: string): Promise<Playlist | null> =>
        edit(async () => {
          const playlist = await backend.createPlaylist(name?.trim() || `Плейлист №${playlists.length + 1}`, tracks);
          if (tracks.length) toast(`${count(tracks.length, TRACKS)} — в новом плейлисте «${playlist.title}»`);
          return playlist;
        }),
      [backend, edit, playlists.length, toast],
    ),
    add: useCallback(
      (id: PlaylistId, tracks: TrackId[], at: number | null = null) =>
        edit(async () => {
          const added = await backend.addToPlaylist(id, tracks, at);
          toast(`Добавлено в «${title(id)}»: ${count(added, TRACKS)}`);
          return added;
        }),
      [backend, edit, title, toast],
    ),
    remove: useCallback((id: PlaylistId, items: ItemId[]) => edit(() => backend.removeFromPlaylist(id, items)), [backend, edit]),
    move: useCallback((id: PlaylistId, item: ItemId, to: number) => edit(() => backend.moveInPlaylist(id, item, to)), [backend, edit]),
    update: useCallback(
      (id: PlaylistId, name: string, description: string | null) => edit(() => backend.updatePlaylist(id, name, description)),
      [backend, edit],
    ),
    destroy: useCallback(
      (id: PlaylistId) =>
        edit(async () => {
          const name = title(id);
          await backend.deletePlaylist(id);
          toast(`Плейлист «${name}» удалён`);
        }),
      [backend, edit, title, toast],
    ),
    /** Импорт файла: найденные треки и недостающие — в уведомлении. */
    import: useCallback(
      () =>
        edit(async () => {
          const path = await backend.pickPlaylistFile();
          if (!path) return null;
          const report = await backend.importPlaylist(path);
          const missing = report.total - report.matched;
          toast(
            missing
              ? `«${report.playlist.title}»: найдено ${report.matched} из ${report.total}, остальные отмечены «нет в библиотеке»`
              : `«${report.playlist.title}»: все ${count(report.total, TRACKS)} найдены`,
          );
          return report.playlist;
        }),
      [backend, edit, toast],
    ),
    export: useCallback(
      async (playlist: Playlist, format: PlaylistFormat) => {
        try {
          const path = await backend.pickExportPath(playlist.title, format);
          if (!path) return;
          const out = await backend.exportPlaylist(playlist.id, format, path);
          const label = EXPORT_FORMATS.find((f) => f.format === format)?.label ?? format;
          toast(
            out.skipped
              ? `Сохранено в ${label}. Без файла на этом компьютере не попало: ${count(out.skipped, TRACKS)}`
              : `Сохранено в ${label}: ${path.split(/[\\/]/).pop()}`,
          );
        } catch (e) {
          toast(describeError(e));
        }
      },
      [backend, toast],
    ),
  };
}

export type Playlists = ReturnType<typeof usePlaylists>;

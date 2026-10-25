// Альбомы и артисты из списка треков. Пока база библиотеки не появилась,
// группировка — на стороне окна: на десятках тысяч треков это миллисекунды.

import type { Track, TrackId } from "@/core";

export interface Album {
  key: string;
  title: string;
  artist: string;
  /** Треки по диску и номеру. */
  tracks: Track[];
  /** У кого из треков просить обложку: одна выборка на альбом. */
  cover: TrackId;
  durationMs: number;
}

export interface Artist {
  key: string;
  name: string;
  albums: Album[];
  /** Все треки, где артист — исполнитель или артист альбома. */
  tracks: Track[];
  cover: TrackId;
}

export interface Library {
  albums: Album[];
  artists: Artist[];
  albumByKey: Map<string, Album>;
  artistByKey: Map<string, Artist>;
  /** Альбом трека; у треков без альбома его нет. */
  albumOf: Map<TrackId, Album>;
}

/** Главный артист трека: артист альбома, иначе первый исполнитель. */
export function primaryArtist(t: Track): string | null {
  const name = (t.album_artist ?? t.artists[0] ?? "").trim();
  return name || null;
}

/** Ключ без учёта регистра: «Кино» и «КИНО» — один артист. */
export function artistKey(name: string): string {
  return normalize(name);
}

/** Ключ альбома: артист альбома + название; `null` — у трека нет альбома. */
export function albumKey(t: Track): string | null {
  const title = t.album?.trim();
  if (!title) return null;
  return `${normalize(primaryArtist(t) ?? "")}\u0001${normalize(title)}`;
}

/** Для сравнения и поиска: регистр, «ё» = «е», лишние пробелы. */
export function normalize(s: string): string {
  return s.toLocaleLowerCase("ru").replaceAll("ё", "е").replace(/\s+/g, " ").trim();
}

const collator = new Intl.Collator("ru", { sensitivity: "base", numeric: true });

export function groupLibrary(tracks: Track[]): Library {
  const albumByKey = new Map<string, Album>();
  const albumOf = new Map<TrackId, Album>();
  for (const t of tracks) {
    const key = albumKey(t);
    if (!key) continue;
    let album = albumByKey.get(key);
    if (!album) {
      album = { key, title: t.album!.trim(), artist: primaryArtist(t) ?? "", tracks: [], cover: t.id, durationMs: 0 };
      albumByKey.set(key, album);
    }
    album.tracks.push(t);
    album.durationMs += t.duration_ms ?? 0;
    albumOf.set(t.id, album);
  }
  for (const album of albumByKey.values()) {
    album.tracks.sort((a, b) => (a.disc_no ?? 1) - (b.disc_no ?? 1) || (a.track_no ?? 0) - (b.track_no ?? 0));
  }
  const albums = [...albumByKey.values()].sort(
    (a, b) => collator.compare(a.artist, b.artist) || collator.compare(a.title, b.title),
  );

  const artistByKey = new Map<string, Artist>();
  const artistFor = (name: string, cover: TrackId) => {
    const key = artistKey(name);
    let artist = artistByKey.get(key);
    if (!artist) {
      artist = { key, name, albums: [], tracks: [], cover };
      artistByKey.set(key, artist);
    }
    return artist;
  };
  for (const album of albums) {
    if (album.artist) artistFor(album.artist, album.cover).albums.push(album);
  }
  // Артистом в списке становится только главный артист трека; приглашённые
  // попадают в треки известных артистов, но не плодят отдельных записей.
  for (const t of tracks) {
    const name = primaryArtist(t);
    if (name) artistFor(name, t.id);
  }
  for (const t of tracks) {
    const keys = new Set([primaryArtist(t), ...t.artists].filter((n): n is string => !!n?.trim()).map(artistKey));
    for (const key of keys) artistByKey.get(key)?.tracks.push(t);
  }
  const artists = [...artistByKey.values()].sort((a, b) => collator.compare(a.name, b.name));
  return { albums, artists, albumByKey, artistByKey, albumOf };
}

/** Обложка трека: у альбома — общая, иначе своя. */
export function coverOf(lib: Library, t: Track): TrackId {
  return lib.albumOf.get(t.id)?.cover ?? t.id;
}

export interface SearchResults {
  tracks: Track[];
  albums: Album[];
  artists: Artist[];
}

/**
 * Поиск по подстроке без учёта регистра и «ё». Совпадения с начала слова —
 * выше. Выдача ограничена: палитре нужны первые строки, а не тысячи.
 */
export function search(tracks: Track[], lib: Library, query: string, limit = 8): SearchResults {
  const q = normalize(query);
  if (!q) return { tracks: [], albums: [], artists: [] };
  const score = (text: string) => {
    const n = normalize(text);
    if (n.startsWith(q)) return 0;
    if (n.includes(` ${q}`)) return 1;
    return n.includes(q) ? 2 : -1;
  };
  const best = <T>(items: T[], texts: (item: T) => string[]) =>
    items
      .map((item) => {
        const scores = texts(item).map(score).filter((s) => s >= 0);
        return { item, s: scores.length ? Math.min(...scores) : -1 };
      })
      .filter((x) => x.s >= 0)
      .sort((a, b) => a.s - b.s)
      .slice(0, limit)
      .map((x) => x.item);
  return {
    tracks: best(tracks, (t) => [t.title, ...t.artists, t.album ?? ""]),
    albums: best(lib.albums, (a) => [a.title, a.artist]),
    artists: best(lib.artists, (a) => [a.name]),
  };
}

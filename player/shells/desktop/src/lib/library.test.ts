import { describe, expect, it } from "vitest";
import type { Track } from "@/core";
import { albumKey, coverOf, groupLibrary, normalize, search } from "./library";

let next = 1;
function t(title: string, artists: string[], album: string | null, extra: Partial<Track> = {}): Track {
  return {
    id: next++,
    uri: `/m/${title}.flac`,
    title,
    artists,
    album,
    album_artist: null,
    duration_ms: 200_000,
    track_no: null,
    disc_no: null,
    recording_mbid: null,
    isrc: null,
    replay_gain: { track_gain_db: null, track_peak: null, album_gain_db: null, album_peak: null },
    ...extra,
  };
}

describe("albumKey", () => {
  it("ignores case, «ё» and spaces; separates artists; no album — no key", () => {
    expect(albumKey(t("a", ["Кино"], "Группа крови"))).toBe(albumKey(t("b", ["КИНО"], " группа  крови ")));
    expect(albumKey(t("a", ["Ёлка"], "Ёлки"))).toBe(albumKey(t("b", ["елка"], "елки")));
    expect(albumKey(t("a", ["Кино"], "Лучшее"))).not.toBe(albumKey(t("b", ["Аквариум"], "Лучшее")));
    expect(albumKey(t("a", ["Кино"], null))).toBeNull();
    expect(albumKey(t("a", ["Кино"], "   "))).toBeNull();
  });

  it("uses album artist so a compilation stays one album", () => {
    const a = t("a", ["Кино"], "Рок-сборник", { album_artist: "Разные" });
    const b = t("b", ["Аквариум"], "Рок-сборник", { album_artist: "Разные" });
    expect(albumKey(a)).toBe(albumKey(b));
  });
});

describe("groupLibrary", () => {
  const tracks = [
    t("Спокойная ночь", ["Кино"], "Группа крови", { track_no: 5 }),
    t("Группа крови", ["Кино"], "Группа крови", { track_no: 1 }),
    t("Город золотой", ["Аквариум"], "Десять стрел", { track_no: 2, disc_no: 1 }),
    t("Без альбома", ["Кино"], null),
    t("Вместе", ["Аквариум", "Кино", "Гость"], "Десять стрел", { track_no: 1, disc_no: 2 }),
    t("Безымянный", [], null),
  ];
  const lib = groupLibrary(tracks);

  it("groups albums, orders tracks by disc and number and albums by artist", () => {
    expect(lib.albums.map((a) => [a.artist, a.title])).toEqual([
      ["Аквариум", "Десять стрел"],
      ["Кино", "Группа крови"],
    ]);
    const blood = lib.albums[1];
    expect(blood.tracks.map((x) => x.title)).toEqual(["Группа крови", "Спокойная ночь"]);
    expect(blood.durationMs).toBe(400_000);
    // Обложку просим у первого трека альбома в порядке библиотеки — одна на альбом.
    expect(blood.cover).toBe(tracks[0].id);
    expect(lib.albums[0].tracks.map((x) => x.title)).toEqual(["Город золотой", "Вместе"]);
  });

  it("artists come from main artists; featured ones get the track but no own entry", () => {
    expect(lib.artists.map((a) => a.name)).toEqual(["Аквариум", "Кино"]);
    const kino = lib.artistByKey.get("кино")!;
    expect(kino.albums.map((a) => a.title)).toEqual(["Группа крови"]);
    // «Вместе» — трек Аквариума, где Кино и Гость приглашённые; у Гостя своей записи нет.
    expect(lib.artistByKey.has("гость")).toBe(false);
    expect(kino.tracks.map((x) => x.title)).toEqual(["Спокойная ночь", "Группа крови", "Без альбома", "Вместе"]);
  });

  it("cover of a track: the album's, or its own without an album", () => {
    expect(coverOf(lib, tracks[1])).toBe(tracks[0].id);
    expect(coverOf(lib, tracks[3])).toBe(tracks[3].id);
  });
});

describe("search", () => {
  const tracks = [
    t("Звезда по имени Солнце", ["Кино"], "Звезда по имени Солнце"),
    t("Солнечные дни", ["Кино"], "Ночь"),
    t("Пачка сигарет", ["Кино"], "Звезда по имени Солнце"),
    t("Ёжик", ["Сплин"], null),
  ];
  const lib = groupLibrary(tracks);

  it("matches words regardless of case and «ё», word starts first", () => {
    const found = search(tracks, lib, "солн");
    expect(found.tracks.map((x) => x.title)).toEqual(["Солнечные дни", "Звезда по имени Солнце", "Пачка сигарет"]);
    expect(search(tracks, lib, "ЕЖИК").tracks.map((x) => x.title)).toEqual(["Ёжик"]);
    expect(search(tracks, lib, "кино").artists.map((a) => a.name)).toEqual(["Кино"]);
    expect(search(tracks, lib, "звезда").albums.map((a) => a.title)).toEqual(["Звезда по имени Солнце"]);
  });

  it("empty query finds nothing; results are limited", () => {
    expect(search(tracks, lib, "  ").tracks).toEqual([]);
    expect(search(tracks, lib, "кино", 2).tracks).toHaveLength(2);
  });

  it("normalizes like the keys", () => {
    expect(normalize("  Ёлки   Палки ")).toBe("елки палки");
  });
});

import { describe, expect, it } from "vitest";
import type { PlayerState } from "./core";
import { artists, duration, livePosition, playedAt } from "./format";

const state = (patch: Partial<PlayerState>): PlayerState => ({
  status: "playing",
  current: null,
  position_ms: 10_000,
  queue_position: null,
  repeat: "off",
  shuffle: false,
  volume: 1,
  ...patch,
});

describe("duration", () => {
  it("formats minutes and hours", () => {
    expect(duration(187_400)).toBe("3:07");
    expect(duration(3_725_000)).toBe("1:02:05");
    expect(duration(0)).toBe("0:00");
    expect(duration(null)).toBe("—");
    expect(duration(-5)).toBe("—");
  });
});

describe("livePosition", () => {
  it("moves only while playing and stops at the end of the track", () => {
    expect(livePosition(state({}), 1000, 3500)).toBe(12_500);
    expect(livePosition(state({ status: "paused" }), 1000, 3500)).toBe(10_000);
    const short = state({
      current: {
        id: 1, uri: "", title: "", artists: [], album: null, album_artist: null,
        duration_ms: 11_000, track_no: null, disc_no: null, recording_mbid: null,
        replay_gain: { track_gain_db: null, track_peak: null, album_gain_db: null, album_peak: null },
      },
    });
    expect(livePosition(short, 0, 60_000)).toBe(11_000);
    // Часы окна «отстали» от момента снимка — назад не едем.
    expect(livePosition(state({}), 5000, 4000)).toBe(10_000);
  });
});

describe("playedAt", () => {
  it("shows only time for today and adds the date otherwise", () => {
    const now = new Date(2026, 9, 25, 18, 0).getTime();
    expect(playedAt(new Date(2026, 9, 25, 14, 5).getTime(), now)).toBe("14:05");
    expect(playedAt(new Date(2026, 9, 3, 9, 7).getTime(), now)).toMatch(/^3 окт.*09:07$/);
  });
});

describe("artists", () => {
  it("joins and has a fallback", () => {
    expect(artists(["Кино", "Виктор Цой"])).toBe("Кино, Виктор Цой");
    expect(artists([])).toBe("Неизвестный артист");
  });
});

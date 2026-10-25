import { describe, expect, it } from "vitest";
import { navigate, START, type Nav } from "./nav";
import { resolveTheme } from "./theme";

const at = (nav: Nav) => nav.stack[nav.index];

describe("navigate", () => {
  it("goes, backs and forwards like a browser", () => {
    let nav = navigate(START, { type: "go", view: { kind: "albums" } });
    nav = navigate(nav, { type: "go", view: { kind: "album", key: "a" } });
    expect(at(nav)).toEqual({ kind: "album", key: "a" });
    nav = navigate(nav, { type: "back" });
    expect(at(nav)).toEqual({ kind: "albums" });
    nav = navigate(nav, { type: "forward" });
    expect(at(nav)).toEqual({ kind: "album", key: "a" });
  });

  it("a new step cuts the forward history", () => {
    let nav = navigate(START, { type: "go", view: { kind: "albums" } });
    nav = navigate(nav, { type: "back" });
    nav = navigate(nav, { type: "go", view: { kind: "history" } });
    expect(nav.stack.map((v) => v.kind)).toEqual(["tracks", "history"]);
    expect(navigate(nav, { type: "forward" })).toBe(nav);
  });

  it("the same screen again is not a new step; ends of history stay put", () => {
    expect(navigate(START, { type: "go", view: { kind: "tracks" } })).toBe(START);
    expect(navigate(START, { type: "back" })).toBe(START);
    const album = navigate(START, { type: "go", view: { kind: "album", key: "a" } });
    expect(navigate(album, { type: "go", view: { kind: "album", key: "b" } }).stack).toHaveLength(3);
  });

  it("remembers at most 50 screens", () => {
    let nav = START;
    for (let i = 0; i < 80; i++) nav = navigate(nav, { type: "go", view: { kind: "album", key: String(i) } });
    expect(nav.stack).toHaveLength(50);
    expect(at(nav)).toEqual({ kind: "album", key: "79" });
  });
});

describe("resolveTheme", () => {
  it("system follows the OS, explicit choice wins", () => {
    expect(resolveTheme("system", true)).toBe("light");
    expect(resolveTheme("system", false)).toBe("dark");
    expect(resolveTheme("dark", true)).toBe("dark");
    expect(resolveTheme("light", false)).toBe("light");
  });
});

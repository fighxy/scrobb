// Экраны окна и переходы «назад/вперёд», как в браузере. Роутер не нужен:
// адресов у экранов нет, есть только стек.

import { useCallback, useReducer } from "react";

export type View =
  | { kind: "tracks" }
  | { kind: "albums" }
  | { kind: "artists" }
  | { kind: "history" }
  | { kind: "album"; key: string }
  | { kind: "artist"; key: string };

export interface Nav {
  stack: View[];
  index: number;
}

export type NavAction = { type: "go"; view: View } | { type: "back" } | { type: "forward" };

/** Сколько экранов помнить позади. */
const DEPTH = 50;

export const START: Nav = { stack: [{ kind: "tracks" }], index: 0 };

export function sameView(a: View, b: View): boolean {
  return a.kind === b.kind && ("key" in a ? a.key : null) === ("key" in b ? b.key : null);
}

export function navigate(nav: Nav, action: NavAction): Nav {
  switch (action.type) {
    case "go": {
      if (sameView(nav.stack[nav.index], action.view)) return nav;
      // Новый переход отрезает «вперёд», как в браузере.
      const stack = [...nav.stack.slice(0, nav.index + 1), action.view].slice(-DEPTH);
      return { stack, index: stack.length - 1 };
    }
    case "back":
      return nav.index > 0 ? { ...nav, index: nav.index - 1 } : nav;
    case "forward":
      return nav.index < nav.stack.length - 1 ? { ...nav, index: nav.index + 1 } : nav;
  }
}

export function useNav() {
  const [nav, dispatch] = useReducer(navigate, START);
  return {
    view: nav.stack[nav.index],
    canBack: nav.index > 0,
    canForward: nav.index < nav.stack.length - 1,
    go: useCallback((view: View) => dispatch({ type: "go", view }), []),
    back: useCallback(() => dispatch({ type: "back" }), []),
    forward: useCallback(() => dispatch({ type: "forward" }), []),
  };
}

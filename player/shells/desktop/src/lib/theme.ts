// Тема окна: тёмная по умолчанию, светлая или как в системе. Выбор хранится
// в localStorage — удобство одного окна; если хранилище недоступно, тема
// просто не запоминается.

import { useCallback, useEffect, useState } from "react";

export type Theme = "dark" | "light" | "system";

const KEY = "scrobb.theme";

export function loadTheme(): Theme {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "dark" || saved === "light" || saved === "system") return saved;
  } catch {
    // Хранилище недоступно — тема по умолчанию.
  }
  return "dark";
}

function saveTheme(theme: Theme) {
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Не запомнится — не страшно.
  }
}

/** Какая тема на самом деле: «как в системе» раскрывается по настройке ОС. */
export function resolveTheme(theme: Theme, prefersLight: boolean): "dark" | "light" {
  if (theme === "system") return prefersLight ? "light" : "dark";
  return theme;
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(loadTheme);
  const [prefersLight, setPrefersLight] = useState(() => matchMedia("(prefers-color-scheme: light)").matches);
  useEffect(() => {
    const query = matchMedia("(prefers-color-scheme: light)");
    const update = () => setPrefersLight(query.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  const setTheme = useCallback((next: Theme) => {
    setThemeState(next);
    saveTheme(next);
  }, []);
  return { theme, resolved: resolveTheme(theme, prefersLight), setTheme };
}

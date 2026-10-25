import { createContext, type CSSProperties, type ReactNode, type RefObject, useContext, useLayoutEffect, useState } from "react";
import { cn } from "@/lib/utils";

/** Верхняя полоса страницы (назад/вперёд, поиск, Scrobb) — её задаёт приложение. */
export const TopBarContext = createContext<ReactNode>(null);

/**
 * Прокручиваемый элемент страницы — для виртуальных списков внутри неё.
 * Через состояние, а не ref: ref родителя подключается позже эффектов
 * вложенных компонентов, и список не узнал бы о нём без лишней перерисовки.
 */
const ScrollContext = createContext<HTMLDivElement | null>(null);

export function useScrollElement(): HTMLDivElement | null {
  return useContext(ScrollContext);
}

interface Props {
  /** Цвет шапки: обычно цвет обложки; без него — нейтральный. */
  tint?: string | null;
  header: ReactNode;
  children?: ReactNode;
}

/**
 * Страница главной области: прокручивается целиком, шапка подкрашена цветом
 * обложки и плавно переходит в фон, верхняя полоса прилипает и при прокрутке
 * получает цвет шапки.
 */
export function Page({ tint, header, children }: Props) {
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null);
  const [scrolled, setScrolled] = useState(false);
  const topBar = useContext(TopBarContext);
  const style = { "--tint": tint ?? "var(--surface-raised)" } as CSSProperties;
  return (
    <div
      ref={setScroller}
      onScroll={(e) => setScrolled(e.currentTarget.scrollTop > 8)}
      className="tinted h-full overflow-y-auto rounded-lg bg-surface"
      style={style}
    >
      <div className="min-h-full">
        <div
          className={cn(
            "sticky top-0 z-20 flex h-14 items-center gap-2 px-3 transition-[background-color,box-shadow] duration-200",
            scrolled ? "bg-[var(--tint-strong)] shadow-md" : "bg-transparent",
          )}
        >
          {topBar}
        </div>
        <header className="tinted-header @container -mt-14 px-6 pt-20 pb-6">{header}</header>
        <div className="tinted-fade @container pb-8">
          <ScrollContext.Provider value={scroller}>{children}</ScrollContext.Provider>
        </div>
      </div>
    </div>
  );
}

/**
 * Где внутри прокручиваемой страницы начинается список: виртуализатору нужен
 * этот отступ (`scrollMargin`), ведь над списком — шапка.
 */
export function useScrollMargin(list: RefObject<HTMLElement | null>): number {
  const page = useScrollElement();
  const [margin, setMargin] = useState(0);
  useLayoutEffect(() => {
    const el = list.current;
    if (!el || !page) return;
    const measure = () =>
      setMargin(Math.round(el.getBoundingClientRect().top - page.getBoundingClientRect().top + page.scrollTop));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(page.firstElementChild ?? page);
    return () => observer.disconnect();
  }, [list, page]);
  return margin;
}

/** Подпись над заголовком шапки: «Альбом», «Артист». */
export function Eyebrow({ children }: { children: ReactNode }) {
  return <div className="text-xs font-semibold tracking-wide text-foreground/80 uppercase">{children}</div>;
}

/**
 * Заголовок шапки. Длинные русские названия не ломают раскладку: размер
 * уменьшается с длиной, перенос — по словам, максимум три строки.
 */
export function PageTitle({ children, text }: { children?: ReactNode; text: string }) {
  const size =
    text.length > 40 ? "text-3xl @3xl:text-4xl" : text.length > 18 ? "text-4xl @3xl:text-5xl" : "text-5xl @3xl:text-7xl";
  return (
    <h1 className={cn("my-2 line-clamp-3 font-extrabold tracking-tight break-words", size)} title={text}>
      {children ?? text}
    </h1>
  );
}

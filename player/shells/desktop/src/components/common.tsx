// Мелкие общие части интерфейса: обложка, эквалайзер, кнопки.

import { Music, Pause, Play } from "lucide-react";
import { type ComponentProps, useState } from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/** Обложка; если картинка не загрузилась (обложки нет) — спокойная заглушка. */
export function Cover({ src, className, round }: { src: string; className?: string; round?: boolean }) {
  const [failed, setFailed] = useState<string | null>(null);
  return (
    <div
      className={cn(
        "relative shrink-0 overflow-hidden bg-surface-raised",
        round ? "rounded-full" : "rounded-md",
        className,
      )}
    >
      {failed === src ? (
        <div className="grid size-full place-items-center bg-gradient-to-br from-surface-hover to-surface-raised text-muted-foreground">
          <Music className="size-[38%]" strokeWidth={1.5} />
        </div>
      ) : (
        <img
          src={src}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
          onError={() => setFailed(src)}
          className="size-full object-cover"
        />
      )}
    </div>
  );
}

/** Три полоски у играющего трека; на паузе замирают. */
export function Equalizer({ paused, className }: { paused: boolean; className?: string }) {
  return (
    <span className={cn("equalizer text-primary", className)} data-paused={paused} role="img" aria-label={paused ? "На паузе" : "Играет"}>
      <span />
      <span />
      <span />
    </span>
  );
}

/** Кнопка-значок с подсказкой: подпись нужна и для подсказки, и для экранного чтеца. */
export function IconButton({
  label,
  active,
  className,
  children,
  ...props
}: ComponentProps<"button"> & { label: string; active?: boolean }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          aria-pressed={active}
          className={cn(
            "relative inline-grid size-8 shrink-0 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-surface-hover hover:text-foreground disabled:pointer-events-none disabled:opacity-40 [&_svg]:size-[18px]",
            active && "text-primary hover:text-primary",
            className,
          )}
          {...props}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/** Большая круглая кнопка «играть/пауза» в шапках и на плитках. */
export function PlayButton({
  playing,
  onClick,
  className,
  size = "lg",
}: {
  playing: boolean;
  onClick: (e: React.MouseEvent) => void;
  className?: string;
  size?: "md" | "lg";
}) {
  const Icon = playing ? Pause : Play;
  return (
    <button
      type="button"
      aria-label={playing ? "Пауза" : "Играть"}
      onClick={onClick}
      className={cn(
        "grid shrink-0 place-items-center rounded-full bg-primary text-primary-foreground shadow-lg transition-transform duration-150 hover:scale-105 active:scale-95",
        size === "lg" ? "size-14 [&_svg]:size-6" : "size-11 [&_svg]:size-5",
        className,
      )}
    >
      <Icon fill="currentColor" strokeWidth={0} className={playing ? "" : "translate-x-px"} />
    </button>
  );
}

/** Ссылка внутри окна (артист, альбом): выглядит текстом, подчёркивается при наведении. */
export function InlineLink({ onClick, children, className }: { onClick?: () => void; children: React.ReactNode; className?: string }) {
  if (!onClick) return <span className={className}>{children}</span>;
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className={cn("min-w-0 truncate text-left hover:text-foreground hover:underline", className)}
    >
      {children}
    </button>
  );
}

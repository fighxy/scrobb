import { ListMusic, Pause, Play, Repeat, Repeat1, SkipBack, SkipForward, Volume1, Volume2, VolumeX } from "lucide-react";
import { useRef, useState } from "react";
import { useApp } from "@/app-context";
import { Cover, IconButton, InlineLink } from "@/components/common";
import { Slider } from "@/components/ui/slider";
import type { Command, PlayerState, RepeatMode } from "@/core";
import { artists, duration } from "@/format";
import { albumKey, artistKey, primaryArtist } from "@/lib/library";
import { cn } from "@/lib/utils";
import { useLivePosition } from "@/usePlayer";

const NEXT_REPEAT: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };
const REPEAT_LABEL: Record<RepeatMode, string> = {
  off: "Повтор выключен",
  all: "Повтор очереди",
  one: "Повтор трека",
};

interface Props {
  receivedAt: number;
  run: (command: Command) => void;
  nowPlayingOpen: boolean;
  onToggleNowPlaying: () => void;
  /** Узкое окно: обложка, название и главные кнопки, ползунок под ними. */
  compact: boolean;
}

/** Панель воспроизведения внизу окна. */
export function PlayerBar({ receivedAt, run, nowPlayingOpen, onToggleNowPlaying, compact }: Props) {
  const app = useApp();
  const { state } = app;
  const track = state.current;
  const album = track ? albumKey(track) : null;
  const artist = track ? primaryArtist(track) : null;

  const info = track ? (
    <div className="flex min-w-0 items-center gap-3">
      <button type="button" onClick={onToggleNowPlaying} aria-label="Сейчас играет" className="shrink-0 rounded-md focus-visible:ring-2 focus-visible:ring-ring">
        <Cover src={app.cover(track)} className="size-14 rounded-md shadow-md" />
      </button>
      <div className="min-w-0">
        <div className="truncate font-semibold">
          <InlineLink onClick={album ? () => app.go({ kind: "album", key: album }) : undefined}>{track.title}</InlineLink>
        </div>
        <div className="truncate text-[13px] text-muted-foreground">
          <InlineLink onClick={artist ? () => app.go({ kind: "artist", key: artistKey(artist) }) : undefined}>{artists(track.artists)}</InlineLink>
        </div>
      </div>
    </div>
  ) : (
    <div className="text-[13px] text-muted-foreground">Ничего не играет</div>
  );

  return (
    <footer
      aria-label="Воспроизведение"
      className={cn(
        "grid shrink-0 items-center gap-x-4 px-2",
        compact ? "grid-cols-[minmax(0,1fr)_auto] gap-y-1 py-1" : "h-[76px] grid-cols-[minmax(180px,1fr)_minmax(0,2fr)_minmax(180px,1fr)]",
      )}
    >
      {info}
      <div className={cn("flex flex-col items-center gap-1", compact && "contents")}>
        <Controls state={state} run={run} compact={compact} />
        <Progress state={state} receivedAt={receivedAt} run={run} className={compact ? "col-span-2" : "max-w-[640px]"} />
      </div>
      {!compact && (
        <div className="flex items-center justify-end gap-1">
          <IconButton label={nowPlayingOpen ? "Скрыть «Сейчас играет»" : "Сейчас играет и очередь"} active={nowPlayingOpen} onClick={onToggleNowPlaying}>
            <ListMusic />
          </IconButton>
          <Volume volume={state.volume} run={run} />
        </div>
      )}
    </footer>
  );
}

export function Controls({ state, run, compact, large }: { state: PlayerState; run: (c: Command) => void; compact?: boolean; large?: boolean }) {
  const playing = state.status === "playing" || state.status === "buffering";
  const RepeatIcon = state.repeat === "one" ? Repeat1 : Repeat;
  return (
    <div className={cn("flex items-center", large ? "gap-6" : "gap-2")}>
      {!compact && !large && (
        <IconButton label={REPEAT_LABEL[state.repeat]} active={state.repeat !== "off"} onClick={() => run({ type: "set_repeat", data: NEXT_REPEAT[state.repeat] })}>
          <RepeatIcon />
        </IconButton>
      )}
      <IconButton label="Предыдущий" disabled={!state.current} onClick={() => run({ type: "previous" })} className={cn(large && "size-12 [&_svg]:size-7")}>
        <SkipBack fill="currentColor" />
      </IconButton>
      <button
        type="button"
        aria-label={playing ? "Пауза" : "Играть"}
        disabled={!state.current}
        onClick={() => run({ type: playing ? "pause" : "resume" })}
        className={cn(
          "grid place-items-center rounded-full bg-foreground text-background transition-transform hover:scale-105 active:scale-95 disabled:opacity-40",
          large ? "size-16 [&_svg]:size-7" : "size-9 [&_svg]:size-4",
        )}
      >
        {playing ? <Pause fill="currentColor" strokeWidth={0} /> : <Play fill="currentColor" strokeWidth={0} className="translate-x-px" />}
      </button>
      <IconButton label="Следующий" disabled={!state.current} onClick={() => run({ type: "next" })} className={cn(large && "size-12 [&_svg]:size-7")}>
        <SkipForward fill="currentColor" />
      </IconButton>
      {large && (
        <IconButton label={REPEAT_LABEL[state.repeat]} active={state.repeat !== "off"} onClick={() => run({ type: "set_repeat", data: NEXT_REPEAT[state.repeat] })}>
          <RepeatIcon />
        </IconButton>
      )}
    </div>
  );
}

/** Ползунок позиции: между снимками ядра движется сам, `Seek` — по отпусканию. */
export function Progress({ state, receivedAt, run, className }: { state: PlayerState; receivedAt: number; run: (c: Command) => void; className?: string }) {
  const live = useLivePosition(state, receivedAt);
  const [dragging, setDragging] = useState<number | null>(null);
  const track = state.current;
  const length = track?.duration_ms ?? 0;
  const position = Math.min(dragging ?? live, length || Infinity);
  return (
    <div className={cn("flex w-full items-center gap-2 text-xs text-muted-foreground tabular-nums", className)}>
      <span className="w-10 text-right">{duration(track ? position : null)}</span>
      <Slider
        aria-label="Позиция"
        min={0}
        max={Math.max(length, 1)}
        step={1000}
        value={[track && length ? position : 0]}
        disabled={!track || !length}
        onValueChange={([v]) => setDragging(v)}
        onValueCommit={([v]) => {
          run({ type: "seek", data: { position_ms: v } });
          setDragging(null);
        }}
      />
      <span className="w-10">{duration(track?.duration_ms)}</span>
    </div>
  );
}

function Volume({ volume, run }: { volume: number; run: (c: Command) => void }) {
  // Громкость до выключения звука — чтобы вернуть её той же кнопкой.
  const before = useRef(volume || 1);
  const Icon = volume === 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2;
  return (
    <div className="flex w-36 items-center gap-1">
      <IconButton
        label={volume === 0 ? "Включить звук" : "Выключить звук"}
        onClick={() => {
          if (volume > 0) before.current = volume;
          run({ type: "set_volume", data: volume > 0 ? 0 : before.current });
        }}
      >
        <Icon />
      </IconButton>
      <Slider aria-label="Громкость" min={0} max={1} step={0.01} value={[volume]} onValueChange={([v]) => run({ type: "set_volume", data: v })} />
    </div>
  );
}

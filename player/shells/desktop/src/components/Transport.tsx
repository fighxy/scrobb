import { useState } from "react";
import type { Command, PlayerState, RepeatMode } from "../core";
import { artists, duration } from "../format";
import { useLivePosition } from "../usePlayer";
import { Icon } from "./icons";

interface Props {
  state: PlayerState;
  receivedAt: number;
  run: (command: Command) => void;
}

const NEXT_REPEAT: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };
const REPEAT_LABEL: Record<RepeatMode, string> = {
  off: "Повтор выключен",
  all: "Повтор очереди",
  one: "Повтор трека",
};

/** Панель воспроизведения внизу окна. */
export function Transport({ state, receivedAt, run }: Props) {
  const live = useLivePosition(state, receivedAt);
  // Пока ползунок тянут, показываем его значение, а `Seek` — по отпусканию.
  const [dragging, setDragging] = useState<number | null>(null);
  const track = state.current;
  const length = track?.duration_ms ?? 0;
  const position = dragging ?? live;
  const playing = state.status === "playing" || state.status === "buffering";

  return (
    <footer className="transport" aria-label="Воспроизведение">
      <div className="now">
        {track ? (
          <>
            <div className="ellipsis title">{track.title}</div>
            <div className="ellipsis muted">
              {artists(track.artists)}
              {track.album ? ` — ${track.album}` : ""}
            </div>
          </>
        ) : (
          <div className="muted">Ничего не играет</div>
        )}
      </div>

      <div className="controls">
        <div className="buttons">
          <button className="icon" aria-label="Предыдущий" onClick={() => run({ type: "previous" })}>
            <Icon name="previous" />
          </button>
          <button
            className="icon play"
            aria-label={playing ? "Пауза" : "Играть"}
            onClick={() => run({ type: playing ? "pause" : "resume" })}
            disabled={!track}
          >
            <Icon name={playing ? "pause" : "play"} size={24} />
          </button>
          <button className="icon" aria-label="Следующий" onClick={() => run({ type: "next" })}>
            <Icon name="next" />
          </button>
        </div>
        <div className="progress">
          <span className="time">{duration(track ? position : null)}</span>
          <input
            type="range"
            aria-label="Позиция"
            min={0}
            max={Math.max(length, 1)}
            step={1000}
            value={Math.min(position, length)}
            disabled={!track || !length}
            onChange={(e) => setDragging(Number(e.target.value))}
            onPointerUp={() => {
              if (dragging != null) run({ type: "seek", data: { position_ms: dragging } });
              setDragging(null);
            }}
            onKeyUp={() => {
              if (dragging != null) run({ type: "seek", data: { position_ms: dragging } });
              setDragging(null);
            }}
          />
          <span className="time">{duration(track?.duration_ms)}</span>
        </div>
      </div>

      <div className="extras">
        <button
          className={`icon${state.repeat !== "off" ? " active" : ""}`}
          aria-label={REPEAT_LABEL[state.repeat]}
          title={REPEAT_LABEL[state.repeat]}
          onClick={() => run({ type: "set_repeat", data: NEXT_REPEAT[state.repeat] })}
        >
          <Icon name="repeat" />
          {state.repeat === "one" && <span className="badge">1</span>}
        </button>
        <label className="volume" title="Громкость">
          <Icon name="volume" />
          <input
            type="range"
            aria-label="Громкость"
            min={0}
            max={1}
            step={0.01}
            value={state.volume}
            onChange={(e) => run({ type: "set_volume", data: Number(e.target.value) })}
          />
        </label>
      </div>
    </footer>
  );
}

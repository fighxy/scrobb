import type { PlayRecord } from "../core";
import { artists, duration, OUTCOME, playedAt } from "../format";

/** Последние прослушивания из локальной истории ядра — все, включая пропуски. */
export function History({ plays }: { plays: PlayRecord[] }) {
  return (
    <section className="panel history" aria-label="История">
      <header className="panel-head">
        <h2>История</h2>
      </header>
      {plays.length === 0 ? (
        <div className="empty muted">Прослушивания появятся здесь.</div>
      ) : (
        <ol className="plays">
          {plays.map((p) => (
            <li key={p.id} className="play">
              <div className="ellipsis title">{p.track.title}</div>
              <div className="ellipsis muted">{artists(p.track.artists)}</div>
              <div className="meta">
                <span className="muted">{playedAt(p.started_at_ms)}</span>
                <span className={`tag outcome-${p.outcome}`}>{OUTCOME[p.outcome]}</span>
                {p.outcome !== "playing" && <span className="muted">{duration(p.played_ms)}</span>}
                {p.scrobbled && (
                  <span className="tag scrobbled" title="Засчитано по правилу скроббла">
                    засчитано
                  </span>
                )}
              </div>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

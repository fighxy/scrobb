import { useApp } from "@/app-context";
import { Cover } from "@/components/common";
import { Eyebrow, Page, PageTitle } from "@/components/Page";
import type { PlayRecord } from "@/core";
import { artists, duration, OUTCOME, playedAt } from "@/format";
import { cn } from "@/lib/utils";

/** Последние прослушивания из локальной истории ядра — все, включая пропуски. */
export function HistoryView({ plays }: { plays: PlayRecord[] }) {
  const app = useApp();
  const byId = new Map(app.tracks.map((t) => [t.id, t]));
  return (
    <Page
      header={
        <>
          <Eyebrow>Прослушивания</Eyebrow>
          <PageTitle text="История" />
          <div className="text-sm text-foreground/80">Всё, что звучало на этом компьютере, — и засчитанное для Scrobb, и пропущенное</div>
        </>
      }
    >
      {plays.length === 0 ? (
        <p className="px-6 py-8 text-muted-foreground">Прослушивания появятся здесь.</p>
      ) : (
        <ol className="flex flex-col px-2">
          {plays.map((p) => {
            const track = p.track_id !== null ? byId.get(p.track_id) : undefined;
            return (
              <li key={p.id} className="flex items-center gap-3 rounded-md px-4 py-2 hover:bg-surface-hover">
                <Cover src={track ? app.cover(track) : "data:,"} className="size-10 rounded" />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium">{p.track.title}</div>
                  <div className="truncate text-[13px] text-muted-foreground">{artists(p.track.artists)}</div>
                </div>
                <div className="flex shrink-0 flex-wrap items-center justify-end gap-2 text-xs">
                  {p.scrobbled && (
                    <span className="rounded-full bg-primary/15 px-2 py-0.5 font-medium text-primary" title="Засчитано по правилу скроббла">
                      засчитано
                    </span>
                  )}
                  <span
                    className={cn(
                      "rounded-full bg-surface-raised px-2 py-0.5 text-muted-foreground",
                      p.outcome === "playing" && "text-success",
                      (p.outcome === "error" || p.outcome === "interrupted") && "text-destructive",
                    )}
                  >
                    {OUTCOME[p.outcome]}
                  </span>
                  <span className="w-28 text-right text-muted-foreground tabular-nums">
                    {playedAt(p.started_at_ms)}
                    {p.outcome !== "playing" && ` · ${duration(p.played_ms)}`}
                  </span>
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </Page>
  );
}

import { FolderPlus, ListMusic, Plus } from "lucide-react";
import { useApp } from "@/app-context";
import { PlayButton } from "@/components/common";
import { Eyebrow, Page, PageTitle } from "@/components/Page";
import { TrackTable } from "@/components/TrackTable";
import { Button } from "@/components/ui/button";
import type { Scanning } from "@/core";
import { count, folderName, longDuration, TRACKS } from "@/format";

/** Акцент, приглушённый до тёмного фона шапки. */
const LIBRARY_TINT = "color-mix(in oklab, var(--accent) 42%, black)";

interface LibraryActions {
  scanning: Scanning | null;
  onAddFolder: () => void;
  onAddFiles: () => void;
}

/** Все треки библиотеки. */
export function TracksView({ scanning, onAddFolder, onAddFiles }: LibraryActions) {
  const app = useApp();
  const tracks = app.tracks;
  const active = app.isActive(tracks);
  const total = tracks.reduce((sum, t) => sum + (t.duration_ms ?? 0), 0);

  return (
    <Page
      tint={LIBRARY_TINT}
      header={
        <div className="flex items-end gap-6">
          <div className="hidden size-40 shrink-0 place-items-center rounded-lg bg-gradient-to-br from-primary to-[color-mix(in_oklab,var(--accent)_35%,black)] shadow-2xl shadow-black/40 @xl:grid @3xl:size-48">
            <ListMusic className="size-16 text-white" strokeWidth={1.5} />
          </div>
          <div className="min-w-0">
            <Eyebrow>Библиотека</Eyebrow>
            <PageTitle text="Все треки" />
            <div className="text-sm text-foreground/80">
              {tracks.length ? `${count(tracks.length, TRACKS)} · ${longDuration(total)}` : "Пока пусто"}
            </div>
          </div>
        </div>
      }
    >
      {tracks.length === 0 ? (
        <EmptyLibrary scanning={scanning} onAddFolder={onAddFolder} onAddFiles={onAddFiles} />
      ) : (
        <>
          <div className="flex items-center gap-3 px-6 py-5">
            <PlayButton
              playing={active && app.state.status === "playing"}
              onClick={() => (active ? app.toggle() : app.play(tracks, 0))}
            />
            <Button variant="outline" size="sm" className="ml-2 rounded-full bg-transparent" onClick={onAddFolder}>
              <FolderPlus />
              Добавить папку
            </Button>
            <Button variant="ghost" size="sm" className="hidden rounded-full @lg:inline-flex" onClick={onAddFiles}>
              <Plus />
              Файлы
            </Button>
          </div>
          <TrackTable tracks={tracks} />
        </>
      )}
    </Page>
  );
}

/** Пустая библиотека: одно главное действие (§10.4) и ход сканирования, если оно идёт. */
function EmptyLibrary({ scanning, onAddFolder, onAddFiles }: LibraryActions) {
  if (scanning) {
    const share = scanning.found ? scanning.read / scanning.found : 0;
    return (
      <div className="mx-auto max-w-md px-6 py-16 text-center" role="status">
        <p className="text-lg font-semibold">Ищу музыку в папке «{folderName(scanning.folder)}»…</p>
        <div className="mt-4 h-1.5 overflow-hidden rounded-full bg-foreground/10">
          <div
            className="h-full rounded-full bg-primary transition-[width] duration-300"
            style={{ width: scanning.read ? `${Math.max(4, share * 100)}%` : "8%" }}
          />
        </div>
        <p className="mt-2 text-sm text-muted-foreground tabular-nums">
          {scanning.read ? `Прочитано ${scanning.read} из ${scanning.found}` : `Найдено ${scanning.found}`}
        </p>
      </div>
    );
  }
  return (
    <div className="mx-auto flex max-w-md flex-col items-center px-6 py-16 text-center">
      <div className="mb-5 grid size-20 place-items-center rounded-full bg-surface-raised text-primary">
        <FolderPlus className="size-9" strokeWidth={1.5} />
      </div>
      <p className="text-xl font-bold">Здесь будет ваша музыка</p>
      <p className="mt-2 text-muted-foreground">
        Добавьте папку — плеер найдёт в ней все треки (FLAC, MP3, AAC, ALAC, Ogg, WAV) вместе с обложками и запомнит её.
      </p>
      <div className="mt-6 flex flex-wrap justify-center gap-2">
        <Button className="rounded-full px-6" onClick={onAddFolder}>
          Добавить папку
        </Button>
        <Button variant="ghost" className="rounded-full" onClick={onAddFiles}>
          или отдельные файлы
        </Button>
      </div>
    </div>
  );
}

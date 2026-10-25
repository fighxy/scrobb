import { Download, MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useApp } from "@/app-context";
import { IconButton, PlayButton } from "@/components/common";
import { Eyebrow, Page, PageTitle } from "@/components/Page";
import { Mosaic } from "@/components/playlist-ui";
import { TrackTable } from "@/components/TrackTable";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { describeError, EXPORT_FORMATS, type PlaylistEntry, type PlaylistView as View, type Track } from "@/core";
import { count, longDuration, TRACKS } from "@/format";
import { useCoverColor } from "@/usePlayer";

/** Строка для трека, которого нет в библиотеке: из снимка, с отрицательным id. */
function placeholder(entry: PlaylistEntry, index: number): Track {
  const t = entry.track;
  return {
    id: -(index + 1),
    uri: entry.location ?? "",
    title: t.title,
    artists: t.artists,
    album: t.album,
    album_artist: t.album_artist,
    duration_ms: t.duration_ms,
    track_no: null,
    disc_no: null,
    recording_mbid: t.recording_mbid,
    isrc: t.ext_ids.isrc ?? null,
    replay_gain: { track_gain_db: null, track_peak: null, album_gain_db: null, album_peak: null },
  };
}

const ORIGIN: Record<string, string> = {
  "import:m3u": "из M3U",
  "import:pls": "из PLS",
  "import:xspf": "из XSPF",
  "import:jspf": "из JSPF",
  "import:csv": "из CSV",
  "import:txt": "из списка",
};

/** Плейлист: шапка с мозаикой, треки (недостающие — бледно), правка и экспорт. */
export function PlaylistView({ id }: { id: string }) {
  const app = useApp();
  const { playlists, backend } = app;
  const [view, setView] = useState<View | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    let cancelled = false;
    backend.playlist(id).then(
      (v) => !cancelled && (setView(v), setError(null)),
      (e) => !cancelled && setError(describeError(e)),
    );
    return () => {
      cancelled = true;
    };
  }, [backend, id, playlists.version]);

  const byId = useMemo(() => new Map(app.tracks.map((t) => [t.id, t])), [app.tracks]);
  const rows = useMemo(
    () => (view?.items ?? []).map((e, i) => (e.track_id !== null && byId.get(e.track_id)) || placeholder(e, i)),
    [view, byId],
  );
  const missing = useMemo(() => new Set(rows.flatMap((t, i) => (t.id < 0 ? [i] : []))), [rows]);
  const card = playlists.playlists.find((p) => p.playlist.id === id);
  const tint = useCoverColor(backend, card?.covers[0] ?? null);

  if (error && !view) {
    return (
      <Page header={<PageTitle text="Плейлиста больше нет" />}>
        <p className="px-6 text-muted-foreground">{error}</p>
      </Page>
    );
  }
  if (!view) return <Page header={<PageTitle text=" " />} />;

  const playlist = view.playlist;
  const items = view.items;
  const playable = rows.filter((_, i) => !missing.has(i));
  const active = app.isActive(playable);
  const total = items.reduce((sum, e) => sum + (e.track.duration_ms ?? 0), 0);
  const origin = playlist.origin ? ORIGIN[playlist.origin] : null;

  // Правки видны сразу, ядро догоняет: перетаскивание не должно мигать.
  const move = (from: number, to: number) => {
    const next = [...items];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    setView({ ...view, items: next });
    void playlists.move(id, moved.id, to);
  };
  const remove = (index: number) => {
    setView({ ...view, items: items.filter((_, i) => i !== index) });
    void playlists.remove(id, [items[index].id]);
  };

  return (
    <Page
      tint={tint}
      header={
        <div className="flex flex-col gap-6 @xl:flex-row @xl:items-end">
          <Mosaic covers={card?.covers ?? []} className="size-44 shadow-2xl shadow-black/50 @3xl:size-56" />
          <div className="min-w-0">
            <Eyebrow>Плейлист{origin ? ` · импорт ${origin}` : ""}</Eyebrow>
            <PageTitle text={playlist.title} />
            {playlist.description && <p className="mb-2 line-clamp-2 text-sm text-foreground/80">{playlist.description}</p>}
            <div className="text-sm text-foreground/80">
              {items.length ? `${count(items.length, TRACKS)} · ${longDuration(total)}` : "Пока пусто"}
              {missing.size > 0 && (
                <span title="Файлов этих треков нет на этом компьютере. Добавьте их в библиотеку — и они найдутся сами.">
                  {" "}
                  · {missing.size} нет в библиотеке
                </span>
              )}
            </div>
          </div>
        </div>
      }
    >
      <div className="flex items-center gap-3 px-6 py-5">
        <PlayButton
          playing={active && app.state.status === "playing"}
          onClick={() => (active ? app.toggle() : playable.length && app.play(playable, 0))}
        />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm" className="ml-2 rounded-full bg-transparent">
              <Download />
              Экспорт
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-80">
            <DropdownMenuLabel className="font-normal text-muted-foreground">Сохранить файлом — для плеера или другого сервиса</DropdownMenuLabel>
            {EXPORT_FORMATS.map((f) => (
              <DropdownMenuItem key={f.format} onSelect={() => void playlists.export(playlist, f.format)} className="flex-col items-start gap-0">
                <span className="font-medium">{f.label}</span>
                <span className="text-xs text-muted-foreground">{f.hint}</span>
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <IconButton label="Ещё">
              <MoreHorizontal />
            </IconButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem onSelect={() => setRenaming(true)}>
              <Pencil />
              Название и описание
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" onSelect={() => setDeleting(true)}>
              <Trash2 />
              Удалить плейлист
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {items.length === 0 ? (
        <p className="px-6 py-6 text-muted-foreground">
          Перетащите сюда треки из библиотеки на название плейлиста слева или добавьте их через меню трека «Добавить в плейлист».
        </p>
      ) : (
        <TrackTable
          tracks={rows}
          unavailable={(i) => missing.has(i)}
          playlist={{ keys: items.map((e) => e.id), onRemove: remove, onMove: move }}
        />
      )}
      <RenameDialog
        open={renaming}
        onOpenChange={setRenaming}
        title={playlist.title}
        description={playlist.description}
        onSave={(title, description) => void playlists.update(id, title, description)}
      />
      <Dialog open={deleting} onOpenChange={setDeleting}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Удалить «{playlist.title}»?</DialogTitle>
            <DialogDescription>Удалится только плейлист — файлы музыки останутся на месте.</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setDeleting(false)}>
              Отмена
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setDeleting(false);
                void playlists.destroy(id).then(() => app.go({ kind: "tracks" }));
              }}
            >
              Удалить
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}

/** Название и описание плейлиста. */
export function RenameDialog({
  open,
  onOpenChange,
  title,
  description,
  heading = "Название и описание",
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string | null;
  heading?: string;
  onSave: (title: string, description: string | null) => void;
}) {
  const [name, setName] = useState(title);
  const [text, setText] = useState(description ?? "");
  useEffect(() => {
    if (open) {
      setName(title);
      setText(description ?? "");
    }
  }, [open, title, description]);
  const field = "w-full rounded-md border bg-surface px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring select-text";
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim()) return;
            onSave(name.trim(), text.trim() || null);
            onOpenChange(false);
          }}
        >
          <DialogHeader>
            <DialogTitle>{heading}</DialogTitle>
          </DialogHeader>
          <label className="grid gap-1.5 text-sm">
            <span className="text-muted-foreground">Название</span>
            <input autoFocus className={field} value={name} onChange={(e) => setName(e.target.value)} maxLength={200} />
          </label>
          <label className="grid gap-1.5 text-sm">
            <span className="text-muted-foreground">Описание</span>
            <textarea className={`${field} min-h-20 resize-none`} value={text} onChange={(e) => setText(e.target.value)} maxLength={1000} />
          </label>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Отмена
            </Button>
            <Button type="submit" disabled={!name.trim()}>
              Сохранить
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

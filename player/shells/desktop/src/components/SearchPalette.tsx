import { useMemo, useState } from "react";
import { useApp } from "@/app-context";
import { Cover } from "@/components/common";
import { Mosaic } from "@/components/playlist-ui";
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { artists } from "@/format";
import { normalize, search } from "@/lib/library";

/** Палитра поиска (⌘K / Ctrl+K): треки, альбомы, артисты. */
export function SearchPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const app = useApp();
  const [query, setQuery] = useState("");
  // Фильтруем сами: cmdk на десятках тысяч строк медленный, а выдача всё равно ограничена.
  const found = useMemo(() => search(app.tracks, app.lib, query), [app.tracks, app.lib, query]);
  const lists = useMemo(() => {
    const q = normalize(query);
    return q ? app.playlists.playlists.filter((p) => normalize(p.playlist.title).includes(q)).slice(0, 5) : [];
  }, [app.playlists.playlists, query]);
  const close = () => {
    onOpenChange(false);
    setQuery("");
  };
  return (
    <CommandDialog
      open={open}
      onOpenChange={(next) => (next ? onOpenChange(true) : close())}
      title="Поиск"
      description="Треки, альбомы и артисты библиотеки"
      showCloseButton={false}
      shouldFilter={false}
      className="top-[18%] translate-y-0 sm:max-w-xl"
    >
      <CommandInput value={query} onValueChange={setQuery} placeholder="Что послушаем?" />
      <CommandList className="max-h-[min(60vh,480px)]">
        {query.trim() && !lists.length && <CommandEmpty>Ничего не нашлось.</CommandEmpty>}
        {found.tracks.length > 0 && (
          <CommandGroup heading="Треки">
            {found.tracks.map((t) => (
              <CommandItem
                key={`t${t.id}`}
                value={`t${t.id}`}
                onSelect={() => {
                  // Трек играет в контексте своего альбома, если он есть.
                  const album = app.lib.albumOf.get(t.id);
                  const list = album?.tracks ?? [t];
                  app.play(list, Math.max(0, list.findIndex((x) => x.id === t.id)));
                  close();
                }}
              >
                <Cover src={app.cover(t)} className="size-9 rounded" />
                <span className="min-w-0">
                  <span className="block truncate font-medium">{t.title}</span>
                  <span className="block truncate text-xs text-muted-foreground">
                    {artists(t.artists)}
                    {t.album ? ` · ${t.album}` : ""}
                  </span>
                </span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {found.albums.length > 0 && (
          <CommandGroup heading="Альбомы">
            {found.albums.map((a) => (
              <CommandItem
                key={`a${a.key}`}
                value={`a${a.key}`}
                onSelect={() => {
                  app.go({ kind: "album", key: a.key });
                  close();
                }}
              >
                <Cover src={app.backend.coverUrl(a.cover, "s")} className="size-9 rounded" />
                <span className="min-w-0">
                  <span className="block truncate font-medium">{a.title}</span>
                  <span className="block truncate text-xs text-muted-foreground">Альбом · {a.artist}</span>
                </span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {lists.length > 0 && (
          <CommandGroup heading="Плейлисты">
            {lists.map((p) => (
              <CommandItem
                key={`p${p.playlist.id}`}
                value={`p${p.playlist.id}`}
                onSelect={() => {
                  app.go({ kind: "playlist", key: p.playlist.id });
                  close();
                }}
              >
                <Mosaic covers={p.covers} className="size-9 rounded" />
                <span className="min-w-0">
                  <span className="block truncate font-medium">{p.playlist.title}</span>
                  <span className="block truncate text-xs text-muted-foreground">Плейлист</span>
                </span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {found.artists.length > 0 && (
          <CommandGroup heading="Артисты">
            {found.artists.map((a) => (
              <CommandItem
                key={`r${a.key}`}
                value={`r${a.key}`}
                onSelect={() => {
                  app.go({ kind: "artist", key: a.key });
                  close();
                }}
              >
                <Cover round src={app.backend.coverUrl(a.cover, "s")} className="size-9" />
                <span className="truncate font-medium">{a.name}</span>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
      </CommandList>
    </CommandDialog>
  );
}

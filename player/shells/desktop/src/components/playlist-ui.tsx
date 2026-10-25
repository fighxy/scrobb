// Общие части плейлистов: мозаика обложек и подменю «Добавить в плейлист»
// (одно и то же в меню по правой кнопке и в выпадающем меню).

import { ListMusic, ListPlus, Plus } from "lucide-react";
import { useApp } from "@/app-context";
import { Cover } from "@/components/common";
import {
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from "@/components/ui/dropdown-menu";
import type { TrackId } from "@/core";
import { cn } from "@/lib/utils";

/** Обложка плейлиста: одна картинка, мозаика 2×2 из разных альбомов или заглушка. */
export function Mosaic({ covers, className }: { covers: TrackId[]; className?: string }) {
  const app = useApp();
  const src = (id: TrackId) => app.backend.coverUrl(app.lib.albumOf.get(id)?.cover ?? id, covers.length >= 4 ? "s" : "l");
  if (covers.length === 0) {
    return (
      <div className={cn("grid shrink-0 place-items-center rounded-md bg-gradient-to-br from-surface-hover to-surface-raised text-muted-foreground", className)}>
        <ListMusic className="size-[40%]" strokeWidth={1.5} />
      </div>
    );
  }
  if (covers.length < 4) {
    return <Cover src={src(covers[0])} className={className} />;
  }
  return (
    <div className={cn("grid shrink-0 grid-cols-2 overflow-hidden rounded-md", className)}>
      {covers.slice(0, 4).map((id) => (
        <Cover key={id} src={src(id)} className="size-full rounded-none" />
      ))}
    </div>
  );
}

/** Подменю «Добавить в плейлист»: все плейлисты и «Новый плейлист». */
export function AddToPlaylist({ tracks, kind }: { tracks: TrackId[]; kind: "context" | "dropdown" }) {
  const { playlists } = useApp();
  const [Sub, Trigger, Content, Item, Separator] =
    kind === "context"
      ? [ContextMenuSub, ContextMenuSubTrigger, ContextMenuSubContent, ContextMenuItem, ContextMenuSeparator]
      : [DropdownMenuSub, DropdownMenuSubTrigger, DropdownMenuSubContent, DropdownMenuItem, DropdownMenuSeparator];
  return (
    <Sub>
      <Trigger disabled={tracks.length === 0}>
        <ListPlus />
        Добавить в плейлист
      </Trigger>
      <Content className="max-h-80 w-56 overflow-y-auto">
        <Item onSelect={() => void playlists.create(tracks)}>
          <Plus />
          Новый плейлист
        </Item>
        {playlists.playlists.length > 0 && <Separator />}
        {playlists.playlists.map((p) => (
          <Item key={p.playlist.id} onSelect={() => void playlists.add(p.playlist.id, tracks)}>
            <span className="truncate">{p.playlist.title}</span>
          </Item>
        ))}
      </Content>
    </Sub>
  );
}

// Общие части плейлистов: мозаика обложек, подменю «Добавить в плейлист»
// (одно и то же в меню по правой кнопке и в выпадающем меню) и значок
// синхронизации с другими устройствами.

import { Cloud, CloudAlert, CloudCheck, CloudOff, ListMusic, ListPlus, Plus, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { useApp } from "@/app-context";
import { Cover, IconButton } from "@/components/common";
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
import type { PlaylistSyncState, TrackId } from "@/core";
import { syncLabel } from "@/format";
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

const SYNC_ICON: Record<PlaylistSyncState, typeof Cloud> = {
  not_linked: Cloud,
  syncing: RefreshCw,
  synced: CloudCheck,
  retrying: CloudOff,
  unauthorized: CloudAlert,
  upgrade_required: CloudAlert,
  unsupported: CloudOff,
};

/** Значок синхронизации плейлистов: состояние в подсказке, щелчок — синхронизировать
 *  сейчас (или привязать устройство, если оно не привязано). */
export function SyncButton({ className }: { className?: string }) {
  const { sync } = useApp();
  // «2 мин назад» должно стареть, пока окно открыто.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);
  if (!sync.available) return null;
  const state = sync.status?.state ?? "not_linked";
  const Icon = SYNC_ICON[state];
  const needsUser = state === "unauthorized" || state === "not_linked";
  return (
    <IconButton
      label={syncLabel(sync.status, now)}
      data-sync={state}
      className={cn(
        className,
        state === "synced" && "text-success hover:text-success",
        (state === "retrying" || state === "unauthorized" || state === "upgrade_required") && "text-warning hover:text-warning",
      )}
      disabled={state === "upgrade_required"}
      onClick={() => (needsUser ? sync.link() : sync.now())}
    >
      <Icon className={cn(state === "syncing" && "animate-spin motion-reduce:animate-none")} />
    </IconButton>
  );
}

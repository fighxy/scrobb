import { CircleAlert, ExternalLink, Unlink } from "lucide-react";
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
import type { Command, PlaylistSyncStatus, ScrobbleStatus } from "@/core";
import { deviceName } from "@/core";
import { syncLabel } from "@/format";
import { cn } from "@/lib/utils";
import type { LinkCode } from "@/usePlayer";

interface Props {
  status: ScrobbleStatus | null;
  available: boolean;
  code: LinkCode | null;
  run: (command: Command) => void;
  openUrl: (url: string) => void;
  /** Синхронизация плейлистов, если она есть на этой сборке. */
  playlistSync?: PlaylistSyncStatus | null;
}

const chip = "inline-flex h-8 items-center gap-2 rounded-full bg-background/55 px-3 text-[13px] font-semibold text-foreground hover:bg-background/75";

/** Связь с аккаунтом Scrobb: привязка по коду (RFC 8628) и состояние отправки. */
export function ScrobbLink({ status, available, code, run, openUrl, playlistSync }: Props) {
  if (!available || !status) return null;
  const link = () => run({ type: "link_scrobb_account", data: { device_name: deviceName() } });
  const queued = status.pending > 0 ? `в очереди ${status.pending}` : null;

  switch (status.state) {
    case "not_linked":
      return (
        <Button size="sm" className="rounded-full px-4 font-semibold" onClick={link}>
          Подключить Scrobb
        </Button>
      );
    case "linking":
      return (
        <>
          <span className={chip}>Привязка…</span>
          <Dialog open onOpenChange={(open) => !open && run({ type: "cancel_scrobb_link" })}>
            <DialogContent className="sm:max-w-sm">
              <DialogHeader>
                <DialogTitle>Привязка к Scrobb</DialogTitle>
                <DialogDescription>
                  Страница привязки открыта в браузере. Убедитесь, что код совпадает, и нажмите «Подключить».
                </DialogDescription>
              </DialogHeader>
              <div className="rounded-lg bg-surface-hover py-4 text-center font-mono text-3xl font-bold tracking-[0.12em] text-primary select-text">
                {code?.user_code ?? "…"}
              </div>
              <DialogFooter>
                <Button variant="ghost" onClick={() => run({ type: "cancel_scrobb_link" })}>
                  Отмена
                </Button>
                {code && (
                  <Button onClick={() => openUrl(code.verification_uri_complete)}>
                    <ExternalLink />
                    Открыть сайт
                  </Button>
                )}
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </>
      );
    case "ready":
    case "retrying":
      return (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" className={chip} title={status.state === "retrying" ? "Нет связи — повторим позже" : "Прослушивания уходят в Scrobb"}>
              <span className={cn("size-2 rounded-full", status.state === "ready" ? "bg-success" : "bg-warning")} />
              {status.state === "ready" ? "Scrobb" : "Scrobb · нет связи"}
              {queued && <span className="font-normal text-muted-foreground">· {queued}</span>}
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-60">
            <DropdownMenuLabel className="font-normal text-muted-foreground">
              {status.state === "ready" ? "Прослушивания уходят в Scrobb" : "Нет связи с Scrobb — отправим позже"}
              {queued ? `, ${queued}` : ""}
            </DropdownMenuLabel>
            {playlistSync && (
              <DropdownMenuItem onSelect={() => run({ type: "sync_playlists" })} className="text-muted-foreground">
                {syncLabel(playlistSync)}
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => run({ type: "unlink_scrobb_account" })}>
              <Unlink />
              Отключить
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      );
    case "unauthorized":
      return (
        <button type="button" className={cn(chip, "text-warning")} onClick={link}>
          <CircleAlert className="size-4" />
          Scrobb: войдите заново{queued ? ` · ${queued}` : ""}
        </button>
      );
    case "upgrade_required":
      return (
        <span className={cn(chip, "text-warning")}>
          <CircleAlert className="size-4" />
          Обновите плеер, чтобы скробблить
        </span>
      );
  }
}

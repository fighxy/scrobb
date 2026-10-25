import {
  Disc3,
  Folder,
  FolderPlus,
  History,
  ListMusic,
  Monitor,
  Moon,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Sun,
  UsersRound,
  X,
} from "lucide-react";
import type { ComponentType } from "react";
import { IconButton } from "@/components/common";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Scanning } from "@/core";
import { folderName } from "@/format";
import type { View } from "@/lib/nav";
import type { Theme } from "@/lib/theme";
import { cn } from "@/lib/utils";

interface Props {
  view: View;
  go: (view: View) => void;
  folders: string[];
  scanning: Scanning | null;
  theme: Theme;
  setTheme: (theme: Theme) => void;
  onAddFolder: () => void;
  onAddFiles: () => void;
  onRemoveFolder: (path: string) => void;
  onRescan: () => void;
  /** Узкая полоса значков вместо полной панели. */
  compact: boolean;
}

const SECTIONS: Array<{ kind: "tracks" | "albums" | "artists" | "history"; label: string; icon: ComponentType<{ className?: string }> }> = [
  { kind: "tracks", label: "Треки", icon: ListMusic },
  { kind: "albums", label: "Альбомы", icon: Disc3 },
  { kind: "artists", label: "Артисты", icon: UsersRound },
  { kind: "history", label: "История", icon: History },
];

/** К какому разделу относится экран: альбом — к «Альбомам», артист — к «Артистам». */
function section(view: View): string {
  return view.kind === "album" ? "albums" : view.kind === "artist" ? "artists" : view.kind;
}

/** Левая панель: разделы библиотеки, папки-источники, тема. */
export function Sidebar(props: Props) {
  const { view, go, compact } = props;
  const active = section(view);
  return (
    <nav aria-label="Разделы" className="flex h-full min-h-0 flex-col gap-2">
      <div className={cn("rounded-lg bg-surface p-2", compact && "flex flex-col items-center")}>
        <div className={cn("flex h-10 items-center gap-2 px-3 font-extrabold tracking-tight", compact && "justify-center px-0")}>
          <span className="grid size-7 place-items-center rounded-md bg-primary text-primary-foreground" aria-hidden>
            <Disc3 className="size-4" />
          </span>
          {!compact && <span className="text-[15px]">Scrobb Player</span>}
        </div>
        <ul className="mt-1 flex flex-col gap-0.5">
          {SECTIONS.map(({ kind, label, icon: Icon }) => {
            const item = (
              <button
                type="button"
                aria-current={active === kind ? "page" : undefined}
                aria-label={compact ? label : undefined}
                onClick={() => go({ kind })}
                className={cn(
                  "flex h-10 w-full items-center gap-3 rounded-md px-3 font-semibold text-muted-foreground transition-colors hover:text-foreground",
                  active === kind && "bg-surface-raised text-foreground",
                  compact && "w-10 justify-center px-0",
                )}
              >
                <Icon className="size-5 shrink-0" />
                {!compact && label}
              </button>
            );
            return (
              <li key={kind}>
                {compact ? (
                  <Tooltip>
                    <TooltipTrigger asChild>{item}</TooltipTrigger>
                    <TooltipContent side="right">{label}</TooltipContent>
                  </Tooltip>
                ) : (
                  item
                )}
              </li>
            );
          })}
        </ul>
      </div>
      <div className="flex min-h-0 flex-1 flex-col rounded-lg bg-surface p-2">
        {compact ? <CompactFolders {...props} /> : <Folders {...props} />}
        <div className={cn("mt-auto flex items-center pt-2", compact ? "justify-center" : "justify-between px-1")}>
          {!compact && (
            <button type="button" onClick={props.onAddFiles} className="flex items-center gap-2 rounded-md px-2 py-1 text-[13px] text-muted-foreground hover:text-foreground">
              <Plus className="size-4" />
              Добавить файлы
            </button>
          )}
          <ThemeMenu theme={props.theme} setTheme={props.setTheme} />
        </div>
      </div>
    </nav>
  );
}

function Folders({ folders, scanning, onAddFolder, onRemoveFolder, onRescan }: Props) {
  return (
    <>
      <div className="flex h-10 items-center gap-1 pl-3">
        <h2 className="flex-1 font-semibold text-muted-foreground">Папки</h2>
        <IconButton label="Найти новые и удалённые файлы" onClick={onRescan} disabled={!folders.length || !!scanning}>
          <RefreshCw className={cn(scanning && "animate-spin")} />
        </IconButton>
        <IconButton label="Добавить папку" onClick={onAddFolder}>
          <FolderPlus />
        </IconButton>
      </div>
      {scanning && <ScanProgress scanning={scanning} />}
      <ul className="-mr-1 flex min-h-0 flex-col gap-0.5 overflow-y-auto pr-1" aria-label="Папки библиотеки">
        {folders.map((f) => (
          <li key={f} className="group flex h-12 items-center gap-3 rounded-md px-2 hover:bg-surface-hover" title={f}>
            <span className="grid size-9 shrink-0 place-items-center rounded-md bg-surface-raised text-muted-foreground">
              <Folder className="size-4" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate font-medium">{folderName(f)}</span>
              <span className="block truncate text-xs text-muted-foreground">{f}</span>
            </span>
            <IconButton
              label={`Убрать папку «${folderName(f)}» из библиотеки`}
              className="opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
              onClick={() => onRemoveFolder(f)}
            >
              <X />
            </IconButton>
          </li>
        ))}
        {folders.length === 0 && !scanning && (
          <li className="rounded-md bg-surface-raised p-4 text-[13px]">
            <p className="font-semibold">Добавьте папку с музыкой</p>
            <p className="mt-1 text-muted-foreground">Плеер запомнит её и будет находить новые треки.</p>
            <button type="button" onClick={onAddFolder} className="mt-3 rounded-full bg-foreground px-4 py-1.5 text-[13px] font-semibold text-background hover:scale-[1.03]">
              Выбрать папку
            </button>
          </li>
        )}
      </ul>
    </>
  );
}

function ScanProgress({ scanning }: { scanning: Scanning }) {
  const share = scanning.found ? scanning.read / scanning.found : 0;
  return (
    <div className="mx-2 mb-2 rounded-md bg-surface-raised p-3 text-[13px]" role="status">
      <div className="truncate">
        Сканирую «{folderName(scanning.folder)}»
      </div>
      <div className="mt-2 h-1 overflow-hidden rounded-full bg-foreground/10">
        <div className="h-full rounded-full bg-primary transition-[width] duration-300" style={{ width: scanning.read ? `${Math.max(4, share * 100)}%` : "8%" }} />
      </div>
      <div className="mt-1 text-xs text-muted-foreground tabular-nums">
        {scanning.read ? `прочитано ${scanning.read} из ${scanning.found}` : `найдено ${scanning.found}`}
      </div>
    </div>
  );
}

/** В узкой полосе папки — в меню. */
function CompactFolders({ folders, scanning, onAddFolder, onAddFiles, onRemoveFolder, onRescan }: Props) {
  return (
    <div className="flex flex-col items-center gap-1">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <IconButton label="Папки" className="size-10">
            {scanning ? <RefreshCw className="animate-spin" /> : <Folder />}
          </IconButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="right" align="start" className="w-64">
          <DropdownMenuLabel>{scanning ? `Сканирую «${folderName(scanning.folder)}»…` : "Папки"}</DropdownMenuLabel>
          {folders.map((f) => (
            <DropdownMenuItem key={f} onSelect={() => onRemoveFolder(f)} title={f}>
              <X />
              <span className="truncate">Убрать «{folderName(f)}»</span>
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={onAddFolder}>
            <FolderPlus />
            Добавить папку
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onAddFiles}>
            <Plus />
            Добавить файлы
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onRescan} disabled={!folders.length || !!scanning}>
            <RefreshCw />
            Обновить
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

const THEMES: Array<{ value: Theme; label: string; icon: ComponentType<{ className?: string }> }> = [
  { value: "dark", label: "Тёмная", icon: Moon },
  { value: "light", label: "Светлая", icon: Sun },
  { value: "system", label: "Как в системе", icon: Monitor },
];

function ThemeMenu({ theme, setTheme }: { theme: Theme; setTheme: (t: Theme) => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <IconButton label="Оформление">
          <MoreHorizontal />
        </IconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="end" className="w-48">
        <DropdownMenuLabel>Тема</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={theme} onValueChange={(v) => setTheme(v as Theme)}>
          {THEMES.map(({ value, label, icon: Icon }) => (
            <DropdownMenuRadioItem key={value} value={value}>
              <Icon className="size-4 text-muted-foreground" />
              {label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

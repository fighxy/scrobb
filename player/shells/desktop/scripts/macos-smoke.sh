#!/usr/bin/env bash
# Дымовая проверка DMG на macOS (её запускает
# .github/workflows/player-installers.yml, можно и вручную):
# в образе — программа и ярлык «Applications» для перетаскивания, подпись
# ad hoc цела; копия программы запускается с плейлистом и импортирует его.
#
#   scripts/macos-smoke.sh <образ.dmg> [снимок.png]
#
# В /Applications ничего не ставит. Плеер пишет данные в
# ~/Library/Application Support/ru.scrobb.player. Нужен Python 3 (читает user.db).
set -euo pipefail

dmg=$(realpath "$1")
screenshot=${2:-}

check() {
  local what=$1
  shift
  if "$@"; then echo "ok: $what"; else echo "Не прошло: $what" >&2; exit 1; fi
}

wait_until() {
  local seconds=$1
  shift
  for _ in $(seq $((seconds * 2))); do
    "$@" && return 0
    sleep 0.5
  done
  "$@"
}

has_playlist() {
  python3 - "$1" "$2" <<'PY' 2>/dev/null
import sqlite3, sys
titles = [r[0] for r in sqlite3.connect(sys.argv[1]).execute('select title from playlists')]
sys.exit(0 if sys.argv[2] in titles else 1)
PY
}

work=$(mktemp -d)
mnt="$work/mnt"
mkdir "$mnt"
trap 'hdiutil detach "$mnt" -quiet 2>/dev/null || true; rm -rf "$work"' EXIT

# 1. Образ.
hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$mnt" >/dev/null
app="$mnt/Scrobb Player.app"
exe="Contents/MacOS/scrobb-player"
check 'в образе Scrobb Player.app' test -x "$app/$exe"
check 'ярлык Applications для перетаскивания' test -L "$mnt/Applications"
check 'подпись цела (ad hoc)' codesign --verify --deep --strict "$app"
echo "архитектуры: $(lipo -archs "$app/$exe")"
ditto "$app" "$work/Scrobb Player.app"
hdiutil detach "$mnt" -quiet

# 2. Запуск с плейлистом.
title='Дымовой тест'
playlist="$work/$title.m3u8"
printf '#EXTM3U\n#EXTINF:215,Кино - Кукушка\nКино - Кукушка.flac\n' > "$playlist"
db="$HOME/Library/Application Support/ru.scrobb.player/user.db"
"$work/Scrobb Player.app/$exe" "$playlist" >"$work/player.log" 2>&1 &
pid=$!
check "плейлист «$title» импортирован" wait_until 60 has_playlist "$db" "$title"
sleep 3
check 'плеер работает' kill -0 "$pid"
if [ -n "$screenshot" ]; then
  screencapture -x "$screenshot" && echo "снимок экрана: $screenshot" || true
fi
kill "$pid"
wait "$pid" 2>/dev/null || true

echo 'Дымовая проверка пройдена.'

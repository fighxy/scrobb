#!/usr/bin/env bash
# Дымовая проверка пакетов Linux (её запускает
# .github/workflows/player-installers.yml, можно и вручную):
# deb ставится через apt со всеми зависимостями, плеер запускается и
# импортирует плейлист из аргумента, пакет удаляется; AppImage — то же
# без установки.
#
#   scripts/linux-smoke.sh <пакет.deb> <файл.AppImage> [снимок.png]
#
# Внимание: ставит и удаляет пакет scrobb-player (через sudo, если не root).
# Нужны Xvfb и Python 3 (читает user.db); снимок — через ImageMagick, если есть.
set -euo pipefail

deb=$(realpath "$1")
appimage=$(realpath "$2")
screenshot=${3:-}
sudo=$([ "$(id -u)" = 0 ] || echo sudo)

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
trap 'kill "$xvfb" 2>/dev/null || true; rm -rf "$work"' EXIT
Xvfb :97 -screen 0 1280x800x24 >/dev/null 2>&1 &
xvfb=$!
export DISPLAY=:97

title='Дымовой тест'
playlist="$work/$title.m3u8"
printf '#EXTM3U\n#EXTINF:215,Кино - Кукушка\nКино - Кукушка.flac\n' > "$playlist"

# Запустить плеер с плейлистом и отдельными данными; дождаться импорта.
run_player() {
  local name=$1 data=$2
  shift 2
  XDG_DATA_HOME="$data" "$@" "$playlist" >"$work/$name.log" 2>&1 &
  local pid=$!
  local db="$data/ru.scrobb.player/user.db"
  check "$name: плейлист «$title» импортирован" wait_until 60 has_playlist "$db" "$title"
  sleep 3
  check "$name: плеер работает" kill -0 "$pid"
  if [ -n "$screenshot" ] && command -v import >/dev/null; then
    import -window root "${screenshot%.png}-$name.png" && echo "снимок экрана: ${screenshot%.png}-$name.png" || true
  fi
  kill "$pid"
  wait "$pid" 2>/dev/null || true
  # AppImage запускает плеер дочерним процессом.
  pkill -x scrobb-player 2>/dev/null || true
}

# 1. deb: зависимости из Depends ставит apt.
$sudo apt-get install -y --no-install-recommends "$deb" >"$work/apt.log" 2>&1 \
  || { cat "$work/apt.log" >&2; exit 1; }
check 'deb установлен' dpkg-query -W -f='' scrobb-player
check 'программа в /usr/bin' test -x /usr/bin/scrobb-player
check 'ярлык в меню приложений' test -f '/usr/share/applications/Scrobb Player.desktop'
check 'значок' test -f /usr/share/icons/hicolor/128x128/apps/scrobb-player.png
run_player deb "$work/deb-data" /usr/bin/scrobb-player
$sudo apt-get remove -y scrobb-player >"$work/apt.log" 2>&1 || { cat "$work/apt.log" >&2; exit 1; }
check 'deb удалён' test ! -e /usr/bin/scrobb-player

# 2. AppImage: без FUSE — распаковка во временную папку.
chmod +x "$appimage"
run_player appimage "$work/appimage-data" env APPIMAGE_EXTRACT_AND_RUN=1 "$appimage"

echo 'Дымовая проверка пройдена.'

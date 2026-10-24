#!/usr/bin/env bash
# Тестовые файлы движка. Нужен ffmpeg с libmp3lame.
# Непрерывный стереосигнал (440 Гц слева, 660 Гц справа), 0,6 с при 44,1 кГц = 26460 кадров,
# разрезан на кадре 11113 — не на границе пакета ни FLAC, ни MP3.
set -euo pipefail
cd "$(dirname "$0")"
ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }

SIGNAL="aevalsrc=0.5*sin(2*PI*440*t)|0.5*sin(2*PI*660*t)"
ff -f lavfi -i "$SIGNAL:s=44100:d=0.6" -c:a pcm_s16le -bitexact -map_metadata -1 full.wav
for codec in flac mp3; do
  args=(-c:a flac)
  [ "$codec" = mp3 ] && args=(-c:a libmp3lame -b:a 192k)
  ff -i full.wav -af atrim=end_sample=11113 "${args[@]}" -bitexact -map_metadata -1 "a.$codec"
  ff -i full.wav -af atrim=start_sample=11113 "${args[@]}" -bitexact -map_metadata -1 "b.$codec"
done
ff -i full.wav -c:a flac -bitexact -map_metadata -1 full.flac
# Эталон для MP3: склейка a.mp3 + b.mp3, раскодированная ffmpeg с учётом gapless.
ff -i a.mp3 -i b.mp3 -filter_complex "[0:a][1:a]concat=n=2:v=0:a=1" \
  -c:a pcm_s16le -bitexact -map_metadata -1 mp3_ffmpeg.wav
# Другая частота: проверка переоткрытия вывода.
ff -f lavfi -i "$SIGNAL:s=48000:d=0.2" -c:a flac -bitexact -map_metadata -1 c48k.flac
# Низкая частота и моно: буфер движка меньше обычного порога записи.
ff -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t):s=8000:d=0.5" -c:a flac -bitexact -map_metadata -1 mono8k.flac

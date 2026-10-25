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
# Теги для чтения библиотекой: тот же a.flac без перекодирования, с комментариями Vorbis.
ff -i a.flac -c copy -map_metadata -1 \
  -metadata TITLE="Кукушка" -metadata ARTIST="Кино" -metadata ALBUM="Чёрный альбом" \
  -metadata ALBUMARTIST="Кино" -metadata TRACKNUMBER="4/12" -metadata DISCNUMBER="1" \
  -metadata MUSICBRAINZ_TRACKID="6f1c2f5b-1c5c-4d6b-9a3e-0a1b2c3d4e5f" \
  -metadata REPLAYGAIN_TRACK_GAIN="-6.50 dB" -metadata REPLAYGAIN_TRACK_PEAK="0.988" \
  -metadata REPLAYGAIN_ALBUM_GAIN="-7.25 dB" -metadata REPLAYGAIN_ALBUM_PEAK="0.999" \
  tagged.flac
# Встроенная обложка (картинка 64×64 цвета 0xc0392b, тип «Cover (front)»): тот же a.flac.
# Картинка — сначала в файл: `-frames:v 1` при прикреплении оборвал бы и звук.
ff -f lavfi -i "color=c=0xc0392b:s=64x64" -frames:v 1 cover.png
ff -i a.flac -i cover.png -map 0:a -map 1:v -c:a copy -c:v png -disposition:v attached_pic \
  -metadata:s:v comment="Cover (front)" -map_metadata -1 -metadata ALBUM="Звезда по имени Солнце" \
  -bitexact covered.flac
rm cover.png

#!/usr/bin/env bash
# Проверки ядра: формат, линтер, тесты и сборка веб-совместимых крейтов под wasm32.
# То же самое будет запускать CI (docs/ci-cd.md: тесты ядра — на Linux на каждом PR).
# На Linux для адаптера вывода (cpal) нужны заголовки ALSA: apt install libasound2-dev.
set -euo pipefail
cd "$(dirname "$0")/.."

# Крейты, которые обязаны собираться под WebAssembly: их использует веб-приложение
# Scrobb и расширение через bindings/wasm (README.md, «Ярусы»).
WEB_SAFE=(scrobb-api scrobb-rules scrobb-client scrobb-sync scrobb-wasm player-domain player-playback player-history)

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

if rustup target list --installed | grep -qx wasm32-unknown-unknown; then
  cargo check --target wasm32-unknown-unknown "${WEB_SAFE[@]/#/-p=}"
else
  echo "Пропущено: нет цели wasm32-unknown-unknown (rustup target add wasm32-unknown-unknown)" >&2
fi

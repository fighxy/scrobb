#!/usr/bin/env bash
# Проверки десктоп-оболочки: интерфейс (типы, тесты, сборка) и Rust-часть
# (формат, clippy, тесты). На Linux нужны libwebkit2gtk-4.1-dev, libgtk-3-dev,
# libsoup-3.0-dev и libasound2-dev; ядро проверяется своим player/core/scripts/check.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
pnpm build

cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

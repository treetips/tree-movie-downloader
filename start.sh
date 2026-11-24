#!/bin/sh
#
# start.sh — アプリをビルドして起動する。
#
# 使い方:
#   ./start.sh             # デバッグビルドで起動
#   ./start.sh --release   # リリースビルドで起動
#
# GPUI はネイティブのデスクトップアプリなので、ビルドしたバイナリをそのまま起動する。
# アプリのウィンドウを閉じるとこのスクリプトも終了する。
set -eu

cd "$(dirname "$0")"

PROFILE="debug"
CARGO_ARGS=""
if [ "${1:-}" = "--release" ]; then
    PROFILE="release"
    CARGO_ARGS="--release"
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo が見つかりません。Rust をインストールしてください。" >&2
    exit 1
fi

echo "==> Building (${PROFILE})..."
# shellcheck disable=SC2086
cargo build ${CARGO_ARGS}

echo "==> Launching..."
exec "./target/${PROFILE}/tree-movie-downloader"

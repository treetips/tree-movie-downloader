#!/usr/bin/env bash
# ビルド済みの yt-dlp / ffmpeg 配布物を GitHub Releases へ公開する。
#
# 使い方:
#   tools/publish-tools-release.sh <output-dir>
#
# 環境変数:
#   GH_TOKEN  GitHub 認証トークン
#   TAG       配布先のリリースタグ（既定: tools-latest）
set -euo pipefail

OUT_DIR="${1:?成果物ディレクトリを指定してください}"
TAG="${TAG:-tools-latest}"
GH_TOKEN="${GH_TOKEN:?GitHub 認証トークンを指定してください}"

[[ "$TAG" == tools-* ]] || {
  echo "::error::TAG は tools- で始まる名前を指定してください: $TAG" >&2
  exit 1
}

[[ -d "$OUT_DIR" ]] || {
  echo "::error::成果物ディレクトリがありません: $OUT_DIR" >&2
  exit 1
}
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
[[ -f "$OUT_DIR/tools-info.json" ]] || {
  echo "::error::tools-info.json がありません: $OUT_DIR" >&2
  exit 1
}

binary_assets=()
metadata_assets=()
for file in "$OUT_DIR"/*; do
  [[ -f "$file" ]] || continue
  case "${file##*/}" in
    tools-info.json | SHA256SUMS.txt) metadata_assets+=("$file") ;;
    *) binary_assets+=("$file") ;;
  esac
done
[[ "${#binary_assets[@]}" -gt 0 ]] || {
  echo "::error::アップロードするツール成果物がありません: $OUT_DIR" >&2
  exit 1
}

if gh release view "$TAG" >/dev/null 2>&1; then
  echo "Release $TAG already exists; replacing assets."
else
  gh release create "$TAG" \
    --title "Tools" \
    --notes "yt-dlp と ffmpeg の配布物です。アプリが起動時に取得します。" \
    --prerelease
fi
# ツール配布用リリースが GitHub の「Latest」になり、アプリ本体の
# 更新確認（releases/latest）を隠さないように prerelease を維持する。
gh release edit "$TAG" --prerelease

keep_assets=" "
for file in "${binary_assets[@]}" "${metadata_assets[@]}"; do
  keep_assets+=" ${file##*/} "
done
while IFS= read -r name; do
  [[ -n "$name" ]] || continue
  if [[ "$keep_assets" != *" $name "* ]]; then
    gh release delete-asset "$TAG" "$name" --yes
  fi
done < <(gh release view "$TAG" --json assets --jq '.assets[].name')

gh release upload "$TAG" "${binary_assets[@]}" --clobber
if [[ "${#metadata_assets[@]}" -gt 0 ]]; then
  gh release upload "$TAG" "${metadata_assets[@]}" --clobber
fi

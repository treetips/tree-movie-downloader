#!/usr/bin/env bash
# yt-dlp と ffmpeg の配布物を GitHub Releases へアップロードできる形にまとめる。
#
# 使い方:
#   tools/build-release-assets.sh <output-dir> [yt-dlp-version] [ffmpeg-static-tag]
#
# 環境変数:
#   REPO_URL  GitHub リポジトリの URL（既定: https://github.com/treetips/tree-movie-downloader）
#   TAG       配布先のリリースタグ（既定: tools-latest）
set -euo pipefail

OUT_DIR="${1:?出力先ディレクトリを指定してください}"
YT_DLP_VERSION="${2:-${YT_DLP_VERSION:-}}"
FFMPEG_STATIC_TAG="${3:-${FFMPEG_STATIC_TAG:-}}"
REPO_URL="${REPO_URL:-https://github.com/treetips/tree-movie-downloader}"
TAG="${TAG:-tools-latest}"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

download() {
  local url="$1"
  local dest="$2"
  echo "download: $url -> $dest"
  curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 30 -o "$dest" "$url"
  test -s "$dest"
}

if [[ -z "$YT_DLP_VERSION" ]]; then
  YT_DLP_VERSION="$(curl -fsSL https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])')"
fi
if [[ -z "$FFMPEG_STATIC_TAG" ]]; then
  FFMPEG_STATIC_TAG="$(curl -fsSL https://api.github.com/repos/eugeneware/ffmpeg-static/releases/latest \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])')"
fi

echo "yt-dlp: $YT_DLP_VERSION"
echo "ffmpeg-static: $FFMPEG_STATIC_TAG"

YT_DLP_BASE="https://github.com/yt-dlp/yt-dlp/releases/download/${YT_DLP_VERSION}"
FFMPEG_BASE="https://github.com/eugeneware/ffmpeg-static/releases/download/${FFMPEG_STATIC_TAG}"

# yt-dlp は macOS 向けだけ universal2 の単一バイナリなので、両アーキテクチャで同じものを使う。
YT_DLP_STAGE="$WORK_DIR/yt-dlp"
mkdir -p "$YT_DLP_STAGE"
download "${YT_DLP_BASE}/yt-dlp_macos" "$YT_DLP_STAGE/yt-dlp"
download "https://raw.githubusercontent.com/yt-dlp/yt-dlp/${YT_DLP_VERSION}/LICENSE" "$YT_DLP_STAGE/LICENSE"
chmod +x "$YT_DLP_STAGE/yt-dlp"
for platform in macos-aarch64 macos-x86_64; do
  (cd "$YT_DLP_STAGE" && zip -j -q "$OUT_DIR/yt-dlp-${platform}-${YT_DLP_VERSION}.zip" yt-dlp LICENSE)
done

YT_DLP_WINDOWS="$WORK_DIR/yt-dlp-windows"
mkdir -p "$YT_DLP_WINDOWS"
download "${YT_DLP_BASE}/yt-dlp.exe" "$YT_DLP_WINDOWS/yt-dlp.exe"
cp "$YT_DLP_STAGE/LICENSE" "$YT_DLP_WINDOWS/LICENSE"
(cd "$YT_DLP_WINDOWS" && zip -j -q "$OUT_DIR/yt-dlp-windows-x86_64-${YT_DLP_VERSION}.zip" yt-dlp.exe LICENSE)

YT_DLP_LINUX="$WORK_DIR/yt-dlp-linux"
mkdir -p "$YT_DLP_LINUX"
download "${YT_DLP_BASE}/yt-dlp_linux" "$YT_DLP_LINUX/yt-dlp"
cp "$YT_DLP_STAGE/LICENSE" "$YT_DLP_LINUX/LICENSE"
chmod +x "$YT_DLP_LINUX/yt-dlp"
(cd "$YT_DLP_LINUX" && zip -j -q "$OUT_DIR/yt-dlp-linux-x86_64-${YT_DLP_VERSION}.zip" yt-dlp LICENSE)

YT_DLP_LINUX_ARM="$WORK_DIR/yt-dlp-linux-arm"
mkdir -p "$YT_DLP_LINUX_ARM"
download "${YT_DLP_BASE}/yt-dlp_linux_aarch64" "$YT_DLP_LINUX_ARM/yt-dlp"
cp "$YT_DLP_STAGE/LICENSE" "$YT_DLP_LINUX_ARM/LICENSE"
chmod +x "$YT_DLP_LINUX_ARM/yt-dlp"
(cd "$YT_DLP_LINUX_ARM" && zip -j -q "$OUT_DIR/yt-dlp-linux-aarch64-${YT_DLP_VERSION}.zip" yt-dlp LICENSE)

download "${FFMPEG_BASE}/darwin-arm64.LICENSE" "$WORK_DIR/ffmpeg-LICENSE"

package_ffmpeg() {
  local platform="$1"
  local ffmpeg_asset="$2"
  local ffprobe_asset="$3"
  local ffmpeg_name="$4"
  local ffprobe_name="$5"
  local stage="$WORK_DIR/ffmpeg-$platform"

  mkdir -p "$stage"
  download "${FFMPEG_BASE}/${ffmpeg_asset}" "$stage/$ffmpeg_name"
  download "${FFMPEG_BASE}/${ffprobe_asset}" "$stage/$ffprobe_name"
  cp "$WORK_DIR/ffmpeg-LICENSE" "$stage/LICENSE"
  chmod +x "$stage/$ffmpeg_name" "$stage/$ffprobe_name"
  (cd "$stage" && zip -j -q "$OUT_DIR/ffmpeg-${platform}-${FFMPEG_STATIC_TAG}.zip" "$ffmpeg_name" "$ffprobe_name" LICENSE)
}

package_ffmpeg "macos-aarch64" "ffmpeg-darwin-arm64" "ffprobe-darwin-arm64" "ffmpeg" "ffprobe"
package_ffmpeg "macos-x86_64" "ffmpeg-darwin-x64" "ffprobe-darwin-x64" "ffmpeg" "ffprobe"
package_ffmpeg "windows-x86_64" "ffmpeg-win32-x64" "ffprobe-win32-x64" "ffmpeg.exe" "ffprobe.exe"
package_ffmpeg "linux-x86_64" "ffmpeg-linux-x64" "ffprobe-linux-x64" "ffmpeg" "ffprobe"
package_ffmpeg "linux-aarch64" "ffmpeg-linux-arm64" "ffprobe-linux-arm64" "ffmpeg" "ffprobe"

python3 - "$OUT_DIR" "$REPO_URL" "$TAG" "$YT_DLP_VERSION" "$FFMPEG_STATIC_TAG" <<'PY'
import hashlib
import json
import pathlib
import sys

out = pathlib.Path(sys.argv[1])
repo_url = sys.argv[2].rstrip("/")
tag = sys.argv[3]
yt_dlp_version = sys.argv[4]
ffmpeg_version = sys.argv[5]
platforms = [
    "macos-aarch64",
    "macos-x86_64",
    "windows-x86_64",
    "linux-x86_64",
    "linux-aarch64",
]

entries = []
for platform in platforms:
    for name, version, filename in (
        ("yt-dlp", yt_dlp_version, f"yt-dlp-{platform}-{yt_dlp_version}.zip"),
        ("ffmpeg", ffmpeg_version, f"ffmpeg-{platform}-{ffmpeg_version}.zip"),
    ):
        asset = out / filename
        if not asset.is_file():
            raise SystemExit(f"missing asset: {asset}")
        entries.append(
            {
                "name": name,
                "version": version,
                "platform": platform,
                "url": f"{repo_url}/releases/download/{tag}/{filename}",
                "sha256": hashlib.sha256(asset.read_bytes()).hexdigest(),
                "required": True,
            }
        )

manifest = out / "tools-info.json"
manifest.write_text(
    json.dumps({"schemaVersion": 1, "tools": entries}, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
print(f"created: {manifest}")
PY

(
  cd "$OUT_DIR"
  for file in *; do
    if [[ "$file" == "SHA256SUMS.txt" ]]; then
      continue
    fi
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum "$file"
    else
      shasum -a 256 "$file"
    fi
  done
) > "$OUT_DIR/SHA256SUMS.txt"

echo "created assets:"
ls -lh "$OUT_DIR"

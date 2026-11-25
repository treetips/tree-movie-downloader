#!/usr/bin/env bash
# 上流ツールのバージョンを解決し、既存の配布情報と比較する。
#
# 使い方:
#   tools/preflight-tools-release.sh
#
# 環境変数:
#   REPOSITORY         GitHub リポジトリ（owner/name）
#   TAG                配布先のリリースタグ（既定: tools-latest）
#   YT_DLP_VERSION     yt-dlp のバージョン（空なら API から解決）
#   FFMPEG_STATIC_TAG  ffmpeg-static のタグ（空なら API から解決）
#   FORCE              true なら既存配布情報に関係なく更新ありとして扱う
#   GH_TOKEN           GitHub API の認証トークン（任意）
#   GITHUB_OUTPUT      changed / yt_dlp_version / ffmpeg_static_tag の出力先
set -euo pipefail

TAG="${TAG:-tools-latest}"
REPOSITORY="${REPOSITORY:-treetips/tree-movie-downloader}"
YT_DLP_VERSION="${YT_DLP_VERSION:-}"
FFMPEG_STATIC_TAG="${FFMPEG_STATIC_TAG:-}"
FORCE="${FORCE:-false}"
OUTPUT_FILE="${GITHUB_OUTPUT:-/dev/stdout}"

fail() {
  echo "::error::$*" >&2
  exit 1
}

[[ "$FORCE" == "true" || "$FORCE" == "false" ]] || fail "FORCE は true または false を指定してください。"

python3 - "$TAG" "$REPOSITORY" <<'PY'
import sys

tag, repository = sys.argv[1:]
if not tag.startswith("tools-"):
    raise SystemExit("TAG は tools- で始まる名前を指定してください")
parts = repository.split("/")
if len(parts) != 2 or not all(parts) or any(char.isspace() for part in parts for char in part):
    raise SystemExit("REPOSITORY は owner/name 形式で指定してください")
PY

if [[ "$TAG" != "tools-latest" ]]; then
  echo "::warning::アプリは tools-latest のみを参照します: $TAG"
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

api_curl() {
  local -a args=(
    -sS -L
    --retry 3
    --retry-delay 2
    --connect-timeout 30
    -H 'Accept: application/vnd.github+json'
    -H 'X-GitHub-Api-Version: 2022-11-28'
    -H 'User-Agent: tree-movie-downloader-tools-release'
  )
  if [[ -n "${GH_TOKEN:-}" ]]; then
    args+=(-H "Authorization: Bearer $GH_TOKEN")
  fi
  curl "${args[@]}" "$@"
}

resolve_latest() {
  local repository="$1"
  local response="$2"
  local url="https://api.github.com/repos/${repository}/releases/latest"

  api_curl -fsSL "$url" -o "$response" || return 1
  python3 - "$response" "$repository" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
try:
    data = json.loads(path.read_text(encoding="utf-8"))
except (OSError, UnicodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"{sys.argv[2]} の最新リリース情報を読み込めません: {error}")
if not isinstance(data, dict):
    raise SystemExit(f"{sys.argv[2]} の最新リリース情報の形式が不正です")
version = data.get("tag_name")
if not isinstance(version, str) or not version or any(char.isspace() for char in version):
    raise SystemExit(f"{sys.argv[2]} の最新リリースに tag_name がありません")
print(version)
PY
}

if [[ -z "$YT_DLP_VERSION" ]]; then
  if ! YT_DLP_VERSION="$(resolve_latest 'yt-dlp/yt-dlp' "$WORK_DIR/yt-dlp-latest.json")"; then
    fail "yt-dlp の最新バージョンを解決できませんでした。"
  fi
fi
if [[ -z "$FFMPEG_STATIC_TAG" ]]; then
  if ! FFMPEG_STATIC_TAG="$(resolve_latest 'eugeneware/ffmpeg-static' "$WORK_DIR/ffmpeg-latest.json")"; then
    fail "ffmpeg-static の最新タグを解決できませんでした。"
  fi
fi

python3 - "$YT_DLP_VERSION" "$FFMPEG_STATIC_TAG" <<'PY'
import re
import sys

for label, value in zip(("yt-dlp", "ffmpeg-static"), sys.argv[1:]):
    if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]*", value) is None:
        raise SystemExit(f"{label} のバージョンに不正な文字が含まれています")
PY

changed=true
if [[ "$FORCE" == "true" ]]; then
  echo "FORCE が指定されたため、既存配布情報に関係なく再作成します。"
else
  encoded_tag="$(python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$TAG")"
  release_api="https://api.github.com/repos/${REPOSITORY}/releases/tags/${encoded_tag}"
  release_response="$WORK_DIR/release.json"

  if ! status="$(api_curl -o "$release_response" -w '%{http_code}' "$release_api")"; then
    fail "配布先リリース $TAG の API 問い合わせに失敗しました。"
  fi

  case "$status" in
    200)
      if ! manifest_url="$(python3 - "$release_response" "$TAG" <<'PY'
import json
import pathlib
import sys
from urllib.parse import urlparse

path = pathlib.Path(sys.argv[1])
try:
    data = json.loads(path.read_text(encoding="utf-8"))
except (OSError, UnicodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"リリース {sys.argv[2]} の情報を読み込めません: {error}")
if not isinstance(data, dict) or not isinstance(data.get("assets"), list):
    raise SystemExit(f"リリース {sys.argv[2]} の情報の形式が不正です")
assets = [asset for asset in data["assets"] if isinstance(asset, dict) and asset.get("name") == "tools-info.json"]
if len(assets) != 1:
    raise SystemExit(f"リリース {sys.argv[2]} に tools-info.json がありません")
url = assets[0].get("browser_download_url")
parsed = urlparse(url) if isinstance(url, str) else None
if parsed is None or parsed.scheme != "https" or parsed.netloc != "github.com":
    raise SystemExit(f"リリース {sys.argv[2]} の tools-info.json URL が不正です")
print(url)
PY
      )"; then
        fail "配布先リリース $TAG のマニフェスト情報を確認できませんでした。"
      fi

      manifest="$WORK_DIR/tools-info.json"
      if ! api_curl -fsSL "$manifest_url" -o "$manifest"; then
        fail "配布先リリース $TAG の tools-info.json を取得できませんでした。"
      fi
      if ! changed="$(python3 - "$manifest" "$release_response" "$YT_DLP_VERSION" "$FFMPEG_STATIC_TAG" <<'PY'
import json
import pathlib
import posixpath
import re
import sys
from urllib.parse import unquote, urlparse

path = pathlib.Path(sys.argv[1])
release_path = pathlib.Path(sys.argv[2])
resolved = {"yt-dlp": sys.argv[3], "ffmpeg": sys.argv[4]}
platforms = {
    "macos-aarch64",
    "macos-x86_64",
    "windows-x86_64",
    "linux-x86_64",
    "linux-aarch64",
}
expected = {(name, platform) for name in resolved for platform in platforms}

try:
    data = json.loads(path.read_text(encoding="utf-8"))
except (OSError, UnicodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"tools-info.json を読み込めません: {error}")
if not isinstance(data, dict) or type(data.get("schemaVersion")) is not int or data["schemaVersion"] != 1:
    raise SystemExit("tools-info.json の schemaVersion が不正です")
entries = data.get("tools")
if not isinstance(entries, list):
    raise SystemExit("tools-info.json の tools が不正です")

seen = set()
entry_files = set()
versions = {name: set() for name in resolved}
for entry in entries:
    if not isinstance(entry, dict):
        raise SystemExit("tools-info.json の entry が不正です")
    name = entry.get("name")
    platform = entry.get("platform")
    version = entry.get("version")
    url = entry.get("url")
    checksum = entry.get("sha256")
    if name not in resolved or platform not in platforms:
        raise SystemExit("tools-info.json に未知のツールまたはプラットフォームがあります")
    if not isinstance(version, str) or not version:
        raise SystemExit(f"tools-info.json の {name} に version がありません")
    if entry.get("required") is not True:
        raise SystemExit(f"tools-info.json の {name}/{platform} が必須配布物ではありません")
    parsed = urlparse(url) if isinstance(url, str) else None
    if parsed is None or parsed.scheme != "https":
        raise SystemExit(f"tools-info.json の {name}/{platform} の URL が不正です")
    if not isinstance(checksum, str) or re.fullmatch(r"[0-9a-fA-F]{64}", checksum) is None:
        raise SystemExit(f"tools-info.json の {name}/{platform} の checksum が不正です")
    key = (name, platform)
    if key in seen:
        raise SystemExit(f"tools-info.json の {name}/{platform} が重複しています")
    seen.add(key)
    entry_files.add(unquote(posixpath.basename(parsed.path)))
    versions[name].add(version)

if seen != expected:
    raise SystemExit("tools-info.json の必須 entry が不足しています")
if any(len(values) != 1 for values in versions.values()):
    raise SystemExit("tools-info.json のツール間で version が一致していません")

try:
    release = json.loads(release_path.read_text(encoding="utf-8"))
except (OSError, UnicodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"リリース情報を読み込めません: {error}")
asset_names = {
    asset.get("name")
    for asset in release.get("assets", [])
    if isinstance(asset, dict) and isinstance(asset.get("name"), str)
}
missing_assets = (entry_files | {"SHA256SUMS.txt"}) - asset_names
current = {name: next(iter(values)) for name, values in versions.items()}
# ZIP は再生成で checksum が変わり得るため、同一 version/tag の差し替えは検知しない。
version_changed = any(current[name] != resolved[name] for name in resolved)
print("true" if version_changed or missing_assets else "false")
PY
      )"; then
        fail "配布先リリース $TAG の tools-info.json が不正です。"
      fi
      ;;
    404)
      echo "配布先リリース $TAG はまだ存在しません。"
      ;;
    *)
      fail "配布先リリース $TAG の API 問い合わせが HTTP $status で失敗しました。"
      ;;
  esac
fi

{
  printf 'changed=%s\n' "$changed"
  printf 'yt_dlp_version=%s\n' "$YT_DLP_VERSION"
  printf 'ffmpeg_static_tag=%s\n' "$FFMPEG_STATIC_TAG"
} >> "$OUTPUT_FILE"

if [[ "$changed" == "true" ]]; then
  echo "更新あり: yt-dlp=$YT_DLP_VERSION, ffmpeg-static=$FFMPEG_STATIC_TAG"
else
  echo "更新なし: yt-dlp=$YT_DLP_VERSION, ffmpeg-static=$FFMPEG_STATIC_TAG"
fi

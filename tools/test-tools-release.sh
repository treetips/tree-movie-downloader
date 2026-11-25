#!/usr/bin/env bash
# Release Tools のネットワーク不要テスト。curl と gh を fake コマンドに置き換える。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

PREFLIGHT="$ROOT/tools/preflight-tools-release.sh"
BUILD="$ROOT/tools/build-release-assets.sh"
PUBLISH="$ROOT/tools/publish-tools-release.sh"
YT_LATEST_URL='https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest'
FFMPEG_LATEST_URL='https://api.github.com/repos/eugeneware/ffmpeg-static/releases/latest'
RELEASE_API_URL='https://api.github.com/repos/example/repo/releases/tags/tools-latest'
MANIFEST_URL='https://github.com/example/repo/releases/download/tools-latest/tools-info.json'

mkdir -p "$TEST_DIR/bin"
export PATH="$TEST_DIR/bin:$PATH"
export FAKE_CURL_LOG="$TEST_DIR/curl.log"
export FAKE_GH_LOG="$TEST_DIR/gh.log"
export FAKE_YT_DLP_RESPONSE="$TEST_DIR/yt-dlp-latest.json"
export FAKE_FFMPEG_RESPONSE="$TEST_DIR/ffmpeg-latest.json"
export FAKE_RELEASE_RESPONSE="$TEST_DIR/release.json"
export FAKE_MANIFEST="$TEST_DIR/tools-info.json"
export GITHUB_OUTPUT="$TEST_DIR/github-output"

cat > "$TEST_DIR/bin/curl" <<'PY'
#!/usr/bin/env python3
import os
import pathlib
import sys

args = sys.argv[1:]
destination = None
write_out = None
url = None
fail_on_http_error = False
value_options = {
    "-o",
    "--output",
    "-w",
    "--write-out",
    "-H",
    "--header",
    "--retry",
    "--retry-delay",
    "--connect-timeout",
}
index = 0
while index < len(args):
    arg = args[index]
    if arg in value_options:
        value = args[index + 1]
        if arg in ("-o", "--output"):
            destination = value
        elif arg in ("-w", "--write-out"):
            write_out = value
        index += 2
        continue
    if arg == "--fail":
        fail_on_http_error = True
    elif arg.startswith("-") and not arg.startswith("--") and "f" in arg[1:]:
        fail_on_http_error = True
    elif not arg.startswith("-"):
        url = arg
    index += 1

if url is None:
    raise SystemExit("fake curl: URL がありません")
with open(os.environ["FAKE_CURL_LOG"], "a", encoding="utf-8") as log:
    log.write(url + "\n")

failure = os.environ.get("FAKE_CURL_FAIL_SUBSTRING", "")
if failure and failure in url:
    raise SystemExit(22)

status = 200
if url == "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest":
    status = int(os.environ.get("FAKE_YT_DLP_STATUS", "200"))
    payload_path = os.environ["FAKE_YT_DLP_RESPONSE"]
elif url == "https://api.github.com/repos/eugeneware/ffmpeg-static/releases/latest":
    status = int(os.environ.get("FAKE_FFMPEG_STATUS", "200"))
    payload_path = os.environ["FAKE_FFMPEG_RESPONSE"]
elif url.startswith("https://api.github.com/repos/example/repo/releases/tags/"):
    status = int(os.environ.get("FAKE_RELEASE_STATUS", "200"))
    payload_path = os.environ["FAKE_RELEASE_RESPONSE"]
elif url.endswith("/tools-info.json"):
    status = int(os.environ.get("FAKE_MANIFEST_STATUS", "200"))
    payload_path = os.environ["FAKE_MANIFEST"]
else:
    if destination is None:
        raise SystemExit("fake curl: ダウンロード先がありません")
    payload = f"fake asset for {url}\n"
    pathlib.Path(destination).write_text(payload, encoding="utf-8")
    sys.exit(0)

payload = pathlib.Path(payload_path).read_text(encoding="utf-8")
if destination is not None:
    pathlib.Path(destination).write_text(payload, encoding="utf-8")
else:
    sys.stdout.write(payload)
if write_out is not None:
    sys.stdout.write(str(status))
if status >= 400 and fail_on_http_error:
    raise SystemExit(22)
PY

cat > "$TEST_DIR/bin/gh" <<'PY'
#!/usr/bin/env python3
import os
import shlex
import sys

args = sys.argv[1:]
with open(os.environ["FAKE_GH_LOG"], "a", encoding="utf-8") as log:
    log.write(" ".join(shlex.quote(arg) for arg in args) + "\n")
if args[:2] == ["release", "view"]:
    raise SystemExit(0 if os.environ.get("FAKE_RELEASE_EXISTS", "true") == "true" else 1)
failure = os.environ.get("FAKE_GH_FAIL_SUBSTRING", "")
if failure and failure in " ".join(args):
    raise SystemExit(1)
PY

chmod +x "$TEST_DIR/bin/curl" "$TEST_DIR/bin/gh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_equal() {
  [[ "$1" == "$2" ]] || fail "$3: expected=$1 actual=$2"
}

assert_empty() {
  [[ ! -s "$1" ]] || fail "$2"
}

assert_contains() {
  grep -Fq "$2" "$1" || fail "$3"
}

assert_not_contains() {
  if grep -Fq "$2" "$1"; then
    fail "$3"
  fi
}

count_matches() {
  grep -Fc "$2" "$1" || true
}

assert_count() {
  local count
  count="$(count_matches "$1" "$2")"
  assert_equal "$3" "$count" "$4"
}

output_value() {
  local key
  local value
  while IFS='=' read -r key value; do
    if [[ "$key" == "$1" ]]; then
      printf '%s\n' "$value"
      return 0
    fi
  done < "$GITHUB_OUTPUT"
  return 1
}

write_manifest() {
  python3 - "$1" "$2" "$3" <<'PY'
import hashlib
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
yt_dlp_version, ffmpeg_tag = sys.argv[2:]
platforms = [
    "macos-aarch64",
    "macos-x86_64",
    "windows-x86_64",
    "linux-x86_64",
    "linux-aarch64",
]
entries = []
for platform in platforms:
    for name, version in (("yt-dlp", yt_dlp_version), ("ffmpeg", ffmpeg_tag)):
        entries.append(
            {
                "name": name,
                "version": version,
                "platform": platform,
                "url": f"https://github.com/example/repo/releases/download/tools-latest/{name}-{platform}-{version}.zip",
                "sha256": hashlib.sha256(f"{name}/{platform}".encode()).hexdigest(),
                "required": True,
            }
        )
path.write_text(json.dumps({"schemaVersion": 1, "tools": entries}) + "\n", encoding="utf-8")
PY
}

write_release_response() {
  python3 - "$1" "$2" <<'PY'
import json
import os
import pathlib
import sys
from urllib.parse import unquote, urlparse

assets = []
if sys.argv[2] == "true":
    names = {"tools-info.json", "SHA256SUMS.txt"}
    manifest_path = pathlib.Path(os.environ["FAKE_MANIFEST"])
    if manifest_path.exists():
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        for entry in manifest["tools"]:
            names.add(unquote(pathlib.PurePosixPath(urlparse(entry["url"]).path).name))
    assets = [
        {
            "name": name,
            "browser_download_url": f"https://github.com/example/repo/releases/download/tools-latest/{name}",
        }
        for name in sorted(names)
    ]
pathlib.Path(sys.argv[1]).write_text(json.dumps({"assets": assets}) + "\n", encoding="utf-8")
PY
}

reset_case() {
  : > "$FAKE_CURL_LOG"
  : > "$FAKE_GH_LOG"
  : > "$GITHUB_OUTPUT"
  unset FAKE_CURL_FAIL_SUBSTRING FAKE_GH_FAIL_SUBSTRING
  export FAKE_YT_DLP_STATUS=200
  export FAKE_FFMPEG_STATUS=200
  export FAKE_RELEASE_STATUS=200
  export FAKE_MANIFEST_STATUS=200
  export FAKE_RELEASE_EXISTS=true
}

set_versions() {
  local current_yt="$1"
  local current_ffmpeg="$2"
  local latest_yt="$3"
  local latest_ffmpeg="$4"

  printf '{"tag_name":"%s"}\n' "$latest_yt" > "$FAKE_YT_DLP_RESPONSE"
  printf '{"tag_name":"%s"}\n' "$latest_ffmpeg" > "$FAKE_FFMPEG_RESPONSE"
  write_manifest "$FAKE_MANIFEST" "$current_yt" "$current_ffmpeg"
  write_release_response "$FAKE_RELEASE_RESPONSE" true
}

run_preflight() {
  local yt_input="${1:-}"
  local ffmpeg_input="${2:-}"
  local force="${3:-false}"
  local tag="${4:-tools-latest}"

  : > "$GITHUB_OUTPUT"
  env TAG="$tag" REPOSITORY=example/repo \
    YT_DLP_VERSION="$yt_input" FFMPEG_STATIC_TAG="$ffmpeg_input" FORCE="$force" \
    bash "$PREFLIGHT"
}

run_build_and_publish() {
  local case_name="$1"
  local changed
  local yt_dlp_version
  local ffmpeg_static_tag
  local out_dir="$TEST_DIR/dist-$case_name"

  changed="$(output_value changed)"
  if [[ "$changed" != "true" ]]; then
    return 0
  fi
  yt_dlp_version="$(output_value yt_dlp_version)"
  ffmpeg_static_tag="$(output_value ffmpeg_static_tag)"

  if ! env TAG=tools-latest REPO_URL=https://github.com/example/repo \
    YT_DLP_VERSION="$yt_dlp_version" FFMPEG_STATIC_TAG="$ffmpeg_static_tag" \
    bash "$BUILD" "$out_dir" > "$TEST_DIR/build-$case_name.log" 2>&1; then
    cat "$TEST_DIR/build-$case_name.log" >&2
    fail "$case_name のビルドに失敗しました"
  fi
  if ! env TAG=tools-latest GH_TOKEN=test-token bash "$PUBLISH" "$out_dir" \
    > "$TEST_DIR/publish-$case_name.log" 2>&1; then
    cat "$TEST_DIR/publish-$case_name.log" >&2
    fail "$case_name の公開に失敗しました"
  fi
}

expect_preflight_failure() {
  if run_preflight "$@" > "$TEST_DIR/preflight-error.log" 2>&1; then
    fail "preflight が失敗すべきケースで成功しました"
  fi
  assert_empty "$FAKE_GH_LOG" "失敗時に gh が呼び出されました"
  assert_empty "$GITHUB_OUTPUT" "失敗時に更新判定が出力されました"
}

verify_built_manifest() {
  python3 - "$1" "$2" "$3" <<'PY'
import json
import pathlib
import sys

out = pathlib.Path(sys.argv[1])
expected = {"yt-dlp": sys.argv[2], "ffmpeg": sys.argv[3]}
data = json.loads((out / "tools-info.json").read_text(encoding="utf-8"))
assert data["schemaVersion"] == 1
assert len(data["tools"]) == 10
assert {entry["name"]: entry["version"] for entry in data["tools"]} == expected
for name, version in expected.items():
    for platform in (
        "macos-aarch64",
        "macos-x86_64",
        "windows-x86_64",
        "linux-x86_64",
        "linux-aarch64",
    ):
        assert (out / f"{name}-{platform}-{version}.zip").is_file()
assert (out / "SHA256SUMS.txt").is_file()
PY
}

test_unchanged() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  # ZIP の再生成で checksum が変わっても、version/tag が同じなら unchanged。
  python3 - "$FAKE_MANIFEST" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text(encoding="utf-8"))
for entry in data["tools"]:
    entry["sha256"] = "f" * 64
path.write_text(json.dumps(data) + "\n", encoding="utf-8")
PY

  run_preflight
  assert_equal false "$(output_value changed)" 'unchanged の changed'
  assert_equal '2026.11.26' "$(output_value yt_dlp_version)" 'unchanged の yt-dlp'
  assert_equal 'b6.1' "$(output_value ffmpeg_static_tag)" 'unchanged の ffmpeg-static'
  run_build_and_publish unchanged

  assert_empty "$FAKE_GH_LOG" 'unchanged で gh が呼び出されました'
  [[ ! -d "$TEST_DIR/dist-unchanged" ]] || fail 'unchanged でビルドされました'
  assert_count "$FAKE_CURL_LOG" "$YT_LATEST_URL" 1 'yt-dlp の解決回数'
  assert_count "$FAKE_CURL_LOG" "$FFMPEG_LATEST_URL" 1 'ffmpeg-static の解決回数'
  echo 'PASS: unchanged はビルド・gh を呼び出さない'
}

test_one_changed() {
  local case_name="$1"
  local current_yt="$2"
  local current_ffmpeg="$3"
  local latest_yt="$4"
  local latest_ffmpeg="$5"

  reset_case
  set_versions "$current_yt" "$current_ffmpeg" "$latest_yt" "$latest_ffmpeg"
  run_preflight
  assert_equal true "$(output_value changed)" "$case_name の changed"
  assert_equal "$latest_yt" "$(output_value yt_dlp_version)" "$case_name の yt-dlp"
  assert_equal "$latest_ffmpeg" "$(output_value ffmpeg_static_tag)" "$case_name の ffmpeg-static"
  run_build_and_publish "$case_name"

  assert_contains "$FAKE_GH_LOG" 'release view ' "$case_name で release view がありません"
  assert_contains "$FAKE_GH_LOG" 'release edit ' "$case_name で release edit がありません"
  assert_contains "$FAKE_GH_LOG" 'release upload ' "$case_name で release upload がありません"
  assert_not_contains "$FAKE_GH_LOG" 'release create ' "$case_name で既存リリースを再作成しました"
  assert_count "$FAKE_CURL_LOG" "$YT_LATEST_URL" 1 "$case_name で yt-dlp を再解決しました"
  assert_count "$FAKE_CURL_LOG" "$FFMPEG_LATEST_URL" 1 "$case_name で ffmpeg-static を再解決しました"
  assert_contains "$FAKE_CURL_LOG" "/releases/download/${latest_yt}/yt-dlp_macos" "$case_name の yt-dlp 版が build に渡りませんでした"
  assert_contains "$FAKE_CURL_LOG" "/releases/download/${latest_ffmpeg}/ffmpeg-darwin-arm64" "$case_name の ffmpeg-static 版が build に渡りませんでした"
  verify_built_manifest "$TEST_DIR/dist-$case_name" "$latest_yt" "$latest_ffmpeg"
  echo "PASS: $case_name は解決済み版でビルド・公開する"
}

test_missing_manifest() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  write_release_response "$FAKE_RELEASE_RESPONSE" false

  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" 'tools-info.json がありません' '欠落マニフェストのエラーがありません'
  echo 'PASS: manifest 欠落は失敗する'
}

test_invalid_manifest() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  printf '{invalid json\n' > "$FAKE_MANIFEST"

  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" 'tools-info.json を読み込めません' '不正マニフェストのエラーがありません'
  echo 'PASS: manifest 不正は失敗する'
}

test_force() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  printf '{invalid json\n' > "$FAKE_MANIFEST"

  run_preflight '' '' true
  assert_equal true "$(output_value changed)" 'force の changed'
  assert_count "$FAKE_CURL_LOG" "$RELEASE_API_URL" 0 'force が既存リリース API を問い合わせました'
  assert_count "$FAKE_CURL_LOG" "$MANIFEST_URL" 0 'force が既存マニフェストを取得しました'
  run_build_and_publish force

  assert_contains "$FAKE_GH_LOG" 'release edit ' 'force で release edit がありません'
  assert_contains "$FAKE_GH_LOG" 'release upload ' 'force で release upload がありません'
  assert_count "$FAKE_CURL_LOG" "$YT_LATEST_URL" 1 'force の yt-dlp 解決回数'
  assert_count "$FAKE_CURL_LOG" "$FFMPEG_LATEST_URL" 1 'force の ffmpeg-static 解決回数'
  verify_built_manifest "$TEST_DIR/dist-force" '2026.11.26' 'b6.1'
  echo 'PASS: force は同一版でも修復再公開する'
}

test_api_failure() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  export FAKE_YT_DLP_STATUS=500

  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" 'yt-dlp の最新バージョンを解決できませんでした' '上流 API 失敗のエラーがありません'

  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  export FAKE_RELEASE_STATUS=500

  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" 'HTTP 500' '配布先 API 失敗のエラーがありません'

  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  export FAKE_MANIFEST_STATUS=404

  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" 'tools-info.json を取得できませんでした' 'マニフェスト取得失敗のエラーがありません'
  echo 'PASS: API 失敗は unchanged にしない'
}

test_initial_release() {
  reset_case
  set_versions '2026.11.25' 'b6.0' '2026.11.26' 'b6.1'
  export FAKE_RELEASE_STATUS=404
  export FAKE_RELEASE_EXISTS=false

  run_preflight
  assert_equal true "$(output_value changed)" '初回作成の changed'
  run_build_and_publish initial

  assert_contains "$FAKE_GH_LOG" 'release create ' '初回作成で release create がありません'
  assert_contains "$FAKE_GH_LOG" 'release edit ' '初回作成で release edit がありません'
  assert_contains "$FAKE_GH_LOG" 'release upload ' '初回作成で release upload がありません'
  echo 'PASS: 既存リリースがなければ初回作成する'
}

test_explicit_versions() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'

  run_preflight '2026.11.26' 'b6.1'

  assert_equal false "$(output_value changed)" '明示 version の changed'
  assert_count "$FAKE_CURL_LOG" "$YT_LATEST_URL" 0 '明示 yt-dlp version で上流 API を呼びました'
  assert_count "$FAKE_CURL_LOG" "$FFMPEG_LATEST_URL" 0 '明示 ffmpeg tag で上流 API を呼びました'
  echo 'PASS: 明示 version は上流 API を呼ばない'
}

test_invalid_inputs() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  expect_preflight_failure '' '' false 'v1.2.3'
  assert_contains "$TEST_DIR/preflight-error.log" 'TAG は tools- ' 'アプリ release tag を拒否しませんでした'

  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  expect_preflight_failure '../bad' 'b6.1'
  assert_contains "$TEST_DIR/preflight-error.log" '不正な文字' '不正 version を拒否しませんでした'
  echo 'PASS: 不正 tag・version は失敗する'
}

test_manifest_validation() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  python3 - "$FAKE_MANIFEST" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text(encoding="utf-8"))
data["schemaVersion"] = 2
path.write_text(json.dumps(data) + "\n", encoding="utf-8")
PY
  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" 'schemaVersion' 'schema 不一致を検出できませんでした'

  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  python3 - "$FAKE_MANIFEST" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text(encoding="utf-8"))
data["tools"].pop()
path.write_text(json.dumps(data) + "\n", encoding="utf-8")
PY
  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" '必須 entry が不足' 'entry 欠落を検出できませんでした'

  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  python3 - "$FAKE_MANIFEST" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text(encoding="utf-8"))
data["tools"][0]["version"] = "2026.11.25"
path.write_text(json.dumps(data) + "\n", encoding="utf-8")
PY
  expect_preflight_failure '' ''
  assert_contains "$TEST_DIR/preflight-error.log" 'ツール間で version が一致していません' 'tool 間 version 不一致を検出できませんでした'
  echo 'PASS: manifest schema・entry・version を検証する'
}

test_missing_asset_triggers_rebuild() {
  reset_case
  set_versions '2026.11.26' 'b6.1' '2026.11.26' 'b6.1'
  python3 - "$FAKE_RELEASE_RESPONSE" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text(encoding="utf-8"))
zip_asset = next(asset for asset in data["assets"] if asset["name"].endswith(".zip"))
data["assets"].remove(zip_asset)
path.write_text(json.dumps(data) + "\n", encoding="utf-8")
PY

  run_preflight

  assert_equal true "$(output_value changed)" '欠落 asset の changed'
  echo 'PASS: 欠落 asset は再作成する'
}

test_built_manifest_roundtrip() {
  reset_case
  set_versions '2026.11.25' 'b6.0' '2026.11.26' 'b6.1'
  run_preflight
  run_build_and_publish roundtrip
  cp "$TEST_DIR/dist-roundtrip/tools-info.json" "$FAKE_MANIFEST"
  write_release_response "$FAKE_RELEASE_RESPONSE" true
  reset_case

  run_preflight

  assert_equal false "$(output_value changed)" 'build 製 manifest の changed'
  echo 'PASS: build 製 manifest は preflight と互換'
}

test_unchanged
test_explicit_versions
test_invalid_inputs
test_manifest_validation
test_missing_asset_triggers_rebuild
test_built_manifest_roundtrip
test_one_changed yt-dlp-changed '2026.11.25' 'b6.1' '2026.11.26' 'b6.1'
test_one_changed ffmpeg-changed '2026.11.26' 'b6.0' '2026.11.26' 'b6.1'
test_missing_manifest
test_invalid_manifest
test_force
test_api_failure
test_initial_release

echo 'All tools release tests passed.'

# アプリケーションのリリースフロー

## 目的

ビルド済みのアプリ本体と、アプリが起動時に取得する外部ツールを、それぞれ GitHub Releases で配布する。
アプリ本体の更新とツールの更新を分離し、ツールだけを差し替えられる状態にする。

## CI

`.github/workflows/ci.yml` が `main` へのpushとPull Requestで次を確認する。

- `rust-toolchain.toml` に固定したRustの導入
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --locked -- -D warnings`
- `cargo test --locked`
- `cargo build --locked`

対象はRustソース・アセット・`Cargo.toml`・`Cargo.lock`・`rust-toolchain.toml`・CIワークフローに限定する。

## アプリ本体のリリースフロー

アプリ本体は `.github/workflows/release.yml` でリリースする。

1. GitHub の **Actions** タブで **Release** を選ぶ。
2. **Run workflow** を押し、`bump`（`major` / `minor` / `patch`）を選ぶ。
   - 初回リリース（`v*` タグが無い場合）は `Cargo.toml` のバージョンをそのまま使う。
3. 最新の `v*` タグと `bump` から次のバージョンを算出し、同じタグが無いことを確認する。
4. `tools-latest` の `tools-info.json` が取得できることを確認する。
   - 取得できない場合はリリースを停止する。先に **Release Tools** を実行する。
5. `cargo test --locked` と `cargo build --release --locked` を実行する。
   - 表示バージョンは `TMD_APP_VERSION` としてビルド時に注入する。
6. `.app` を組み立てる。
   - `Contents/MacOS/tree-movie-downloader`
   - `Contents/Info.plist`
   - `Contents/Resources/AppIcon.icns`
   - `Contents/Resources/assets`（サウンド・背景画像）
   - `Contents/Resources/LICENSE`
   - `Contents/Resources/THIRD_PARTY_NOTICES.md`
7. ad-hoc署名して `.app` をZIP化し、SHA-256を計算する。
8. `update-info.json` を生成し、ZIP・チェックサム・ライセンスと一緒に添付する。
9. Gitタグ（`vX.Y.Z`）を作成・pushし、GitHub Releaseを作成する。

アプリは `.app` として起動した場合、同梱アセットを `Contents/Resources/assets` から読む。
`cargo run` で起動した場合はリポジトリ直下の `assets` を読む。

### アプリ本体の配布物

- `Tree-Movie-Downloader-v<version>-macos.zip`（`.app` を格納）
- `update-info.json`（`version` / `url` / `sha256`）
- `SHA256SUMS.txt`
- `LICENSE`
- `THIRD_PARTY_NOTICES.md`

## ツール配布フロー

外部ツールはアプリ本体とは別の `tools-latest` リリースで配布する。
アプリ本体のリリース前に、こちらを先に実行しておく。

1. GitHub の **Actions** タブで **Release Tools** を選ぶ。
2. **Run workflow** を押す。
   - `tag`: 配布先のリリースタグ（既定は `tools-latest`）
   - `yt_dlp_version`: yt-dlp のバージョン（空なら最新）
   - `ffmpeg_static_tag`: `eugeneware/ffmpeg-static` のタグ（空なら最新）
3. ワークフローが上流の Releases からバイナリを取得し、プラットフォーム別の ZIP と
   `tools-info.json` を生成する。
4. `tools-latest` リリースを作成または再利用し、成果物を上書きアップロードする。
   - このリリースは **pre-release** として扱い、GitHub の「Latest」がアプリ本体の
     リリースを指したままになるようにする。

アプリは次の URL から配布情報を取得する。

- `https://github.com/treetips/tree-movie-downloader/releases/download/tools-latest/tools-info.json`

### ツール配布物

- ツール情報: `tools-info.json`
- yt-dlp: `yt-dlp-<platform>-<version>.zip`
- ffmpeg: `ffmpeg-<platform>-<version>.zip`（`ffmpeg` と `ffprobe` を含む）
- チェックサム: `SHA256SUMS.txt`

対象プラットフォームは次のとおり。

- `macos-aarch64`
- `macos-x86_64`
- `windows-x86_64`
- `linux-x86_64`
- `linux-aarch64`

## 上流の配布元

- yt-dlp: [yt-dlp/yt-dlp の Releases](https://github.com/yt-dlp/yt-dlp/releases)
- ffmpeg: [eugeneware/ffmpeg-static の Releases](https://github.com/eugeneware/ffmpeg-static/releases)

## 失敗時の対応

- アセットの生成やアップロードに失敗した場合、`tools-latest` の既存アセットは置き換わらない。
  原因を修正して **Release Tools** を再実行する。
- アプリ本体のリリースでタグ作成後に失敗した場合、同名タグが残るため、タグと
  Release を確認してから再実行する。
- 公開済みのアセットを手作業で差し替えず、ワークフローから再生成する。
- アプリ側で配布情報の取得に失敗した場合は、起動時のダウンロード画面に理由と `再試行` を表示する。

## 注意事項

- 配布バイナリは `macos-latest` のネイティブアーキテクチャのみで、Intel向けUniversal Binaryは未対応。
- Apple Developer Program未加入のためad-hoc署名で配布する。初回起動時にGatekeeperの警告が出る。
- アプリ本体のバージョンと `tools-latest` のバージョンは独立して管理する。
- GitHub Release のアセットは上書きできるが、履歴は残らない。配布したバージョンと
  `tools-info.json` の内容はリリースノートやログで確認する。

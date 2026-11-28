# アプリケーションのリリースフロー

## 目的

ビルド済みのアプリ本体と、アプリが取得する外部ツール（yt-dlp、ffmpeg/ffprobe、Deno）を、それぞれ GitHub Releases で配布する。アプリ本体の更新とライブラリの更新を分離し、ツールだけを差し替えられる状態にする。

## CI

`.github/workflows/ci.yml` が `main` へのpushとPull Requestで次を確認する。

- `rust-toolchain.toml` に固定したRustの導入
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --locked -- -D warnings`
- `cargo test --locked`
- `cargo build --locked`
- Release Tools 用シェルの構文チェックとネットワーク不要テスト

対象はRustソース・アセット・`Cargo.toml`・`Cargo.lock`・`rust-toolchain.toml`・`tools/` 配下のスクリプト・CIとRelease Toolsのワークフローです。

## アプリ本体のリリースフロー

アプリ本体は `.github/workflows/release.yml` でリリースする。

1. GitHub の **Actions** タブで **Release** を選ぶ。
2. **Run workflow** を押し、`bump`（`major` / `minor` / `patch`）を選ぶ。
   - 初回リリース（`v*` タグが無い場合）は `Cargo.toml` のバージョンをそのまま使う。
3. 最新の `v*` タグと `bump` から次のバージョンを算出し、同じタグが無いことを確認する。
4. `tools-latest` の `tools-info.json` を取得し、macOS の両アーキテクチャ向けに yt-dlp・ffmpeg・Deno が必須ツールとして含まれることを確認する。
   - 取得できない場合や Deno が欠けている場合はリリースを停止する。先に **Release Tools** を実行する。
5. `cargo test --locked` と `cargo build --release --locked` を実行する。
   - 表示バージョンは `TMD_APP_VERSION` としてビルド時に注入する。
6. `TreeMovieDownloader.app` を組み立てる。
   - `Contents/MacOS/tree-movie-downloader`
   - `Contents/Info.plist`
   - `Contents/Resources/AppIcon.icns`
   - `Contents/Resources/assets`（サウンド・背景画像）
   - `Contents/Resources/LICENSE`
   - `Contents/Resources/THIRD_PARTY_NOTICES.md`
7. ad-hoc署名して `.app` をZIP化し、SHA-256を計算する。
8. `update-info.json` を生成し、ZIP・チェックサム・ライセンスと一緒に添付する。
9. Gitタグ（`vX.Y.Z`）を作成・pushし、GitHub Releaseを作成する。

アプリは `.app` として起動した場合、同梱アセットを `Contents/Resources/assets` から読む。`cargo run` で起動した場合はリポジトリ直下の `assets` を読む。アプリ更新では配置先のバンドル名を `TreeMovieDownloader.app` に変更するため、旧バンドル名のアプリも移行する。新しいアプリを配置後に旧バンドルを退避して起動し、起動失敗時は旧バンドルを復元する。退避した旧バンドルは次回以降の起動時に60秒以上経過したものだけ削除する。設定ディレクトリとバンドル識別子は変更しないため、既存設定は引き継ぐ。

### アプリ本体の配布物

- `Tree-Movie-Downloader-v<version>-macos.zip`（`.app` を格納）
- `update-info.json`（`version` / `url` / `sha256`）
- `SHA256SUMS.txt`
- `LICENSE`
- `THIRD_PARTY_NOTICES.md`

## ツール配布フロー

外部ツールはアプリ本体とは別の `tools-latest` リリースで配布する。アプリ本体のバージョンやリリースタイミングには依存しない。アプリ本体のリリース前に、こちらを先に実行しておく。Deno を必須にするアプリを公開する際は、Deno を含む配布情報を先に公開する。

1. GitHub の **Actions** タブで **Release Tools** を選ぶ。
2. **Run workflow** を押す。
   - `tag`: 配布先のリリースタグ（既定は `tools-latest`）
   - `yt_dlp_version`: yt-dlp のバージョン（空なら最新）
   - `ffmpeg_static_tag`: `eugeneware/ffmpeg-static` のタグ（空なら最新）
   - `deno_version`: Deno のバージョン（空なら最新）
   - `force`: 上流の更新がなくても配布物を再作成するか（既定は `false`）
3. preflight が上流の version / tag を1回だけ解決し、配布先の既存 `tools-info.json` と比較する。
   - 入力が空でない場合は指定値を使い、空の場合だけ上流 API から最新値を解決する。
   - 既存 manifest の全プラットフォーム・全ツールの version が解決値と一致すれば、build / publish をスキップして正常終了する。
   - `force=true` の場合は既存 manifest と比較せず、再作成・再公開する。
   - manifest の欠落・不正、上流 API や配布先 API の失敗は更新なしとせず、ワークフローを失敗させる。
4. 更新がある場合だけ、解決済みの version / tag を使ってバイナリを取得し、プラットフォーム別の ZIP と `tools-info.json` を生成する。
5. `tools-latest` リリースを作成または再利用し、成果物を上書きアップロードする。
   - このリリースは **pre-release** として扱い、GitHub の「Latest」がアプリ本体のリリースを指したままになるようにする。
   - 公開は全件 atomic ではないため、アップロード途中の失敗で一部のアセットだけが差し替わる可能性がある。

アプリは次の URL から配布情報を取得する。

- `https://github.com/treetips/tree-movie-downloader/releases/download/tools-latest/tools-info.json`

### ツール配布物

- ツール情報: `tools-info.json`
- yt-dlp: `yt-dlp-<platform>-<version>.zip`
- ffmpeg: `ffmpeg-<platform>-<version>.zip`（`ffmpeg` と `ffprobe` を含む）
- Deno: `deno-<platform>-<version>.zip`（`deno` または `deno.exe` とライセンスを含む）
- チェックサム: `SHA256SUMS.txt`

対象プラットフォームは次のとおり。

- `macos-aarch64`
- `macos-x86_64`
- `windows-x86_64`
- `linux-x86_64`
- `linux-aarch64`

`tools-info.json` とアプリ側のローカル記録 `.installed.json` の schema は変更しない。ライブラリ更新では、この2つの情報とローカルバイナリの完全性を比較し、更新対象だけを SHA-256 検証して置換する。

## 上流の配布元

- yt-dlp: [yt-dlp/yt-dlp の Releases](https://github.com/yt-dlp/yt-dlp/releases)
- ffmpeg: [eugeneware/ffmpeg-static の Releases](https://github.com/eugeneware/ffmpeg-static/releases)
- Deno: [denoland/deno の Releases](https://github.com/denoland/deno/releases)

## 失敗時の対応

- preflight が失敗した場合は、更新なしとして終了せずワークフローを失敗させる。API 障害や manifest の欠落・不正を修正してから **Release Tools** を再実行する。既存 manifest がない場合は `force=true` を指定して再公開する。
- アセット生成に失敗した場合は公開を開始しない。原因を修正して **Release Tools** を再実行する。
- アップロード途中で失敗した場合は、既存アセットの一部だけが新しくなっている可能性がある。原因を修正して `force=true` で再実行し、全 ZIP・`tools-info.json`・`SHA256SUMS.txt` が同じバージョンでそろったことを確認する。
- アプリ本体のリリースでタグ作成後に失敗した場合、同名タグが残るため、タグと Release を確認してから再実行する。
- 公開済みのアセットを手作業で差し替えず、ワークフローから再生成する。
- アプリ側のツール準備で配布情報の取得に失敗した場合は、ツール準備画面に理由と `再試行` を表示する。日常の更新確認失敗は自動インストールせず、成功した側の結果を保持する。

## 注意事項

- 配布バイナリは `macos-latest` のネイティブアーキテクチャのみで、Intel向けUniversal Binaryは未対応。
- Apple Developer Program未加入のためad-hoc署名で配布する。初回起動時にGatekeeperの警告が出る。
- アプリ本体のバージョンと `tools-latest` のバージョンは独立して管理する。
- GitHub Release のアセットは上書きできるが、履歴は残らない。配布したバージョンと `tools-info.json` の内容はリリースノートやログで確認する。

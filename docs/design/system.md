# システム

## アプリケーション形態

- デスクトップアプリ。
- macOSのみ対応。

## アーキテクチャ

- 画面はGPUIのビュー層で構成し、状態を保持するモデルと描画処理を分離する。
- 画面の状態はModelが保持し、Viewは状態を読んで描画する。
- ダウンロードの進行はUseCaseが調整し、yt-dlpの実行やファイル操作はServiceへ分離する。
- 設定・ログなど横断的な処理はCoreに置く。

## Linter・Formatter

- Linterは [Clippy](https://github.com/rust-lang/rust-clippy) を使う。
- Formatterは [rustfmt](https://github.com/rust-lang/rustfmt) を使う。
- 実行手順（AGENTS.md「ビルド・テスト・検証方法」の順序に従う）:
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo fmt`
  - `cargo build`
- MarkdownのLinter・Formatterは [rumdl](https://rumdl.dev/) を使う。設定はリポジトリ直下の `.rumdl.toml` で管理する。
  - Markdownを修正した場合は `rumdl fmt` でフォーマットし、`rumdl check` でlintチェックして違反があれば修正する。
  - GitHub Actions（`.github/workflows/markdown.yml`）でも `rumdl check` と `rumdl fmt --check` を実行し、違反があるとCIに失敗する（ワークフローは今後整備）。対象は `.opencode`（同梱スキル文書）と `tools`（同梱ツールの付属文書）を除くすべてのMarkdown。

## 外部ツール

- yt-dlp と ffmpeg（ffprobe を含む）はアプリ本体へ同梱せず、GitHub Releases から取得する。
- 配布物は `tools-latest` タグの `tools-info.json` で管理し、実行中の OS と CPU に合う yt-dlp と ffmpeg の ZIP を選ぶ。
- 取得した ZIP は SHA-256 を検証してから `~/.config/tree-movie-downloader/tools/` へ配置する。
- 起動時のツール準備では、管理下の yt-dlp・ffmpeg・ffprobe が実行可能なファイルとして存在するかを確認する。1つでも欠けていれば、更新対象だけを取得してツール準備画面を表示し、完了するまでメイン画面の操作をブロックする。すべて揃っていれば、この準備処理では通信せずメイン画面を表示する。
- ツール準備と日常の更新確認は分離する。ツールがそろっている場合も、準備完了後にライブラリの更新を1回バックグラウンドで確認する。
- ダウンロード中は受信バイト数と Content-Length から進捗率を求め、配置中は不確定表示にする。失敗時は理由と再試行ボタンを表示する。
- yt-dlp の実行時は管理下のバイナリを使い、`--ffmpeg-location` で管理下の ffmpeg を渡す。
- 配布物は `.github/workflows/tools-release.yml` と `tools/` 配下の生成・事前確認・公開スクリプトで管理する。上流の yt-dlp 公式 Releases と `eugeneware/ffmpeg-static` を使用する。

## アップデート

- アプリ本体とライブラリ（yt-dlp、ffmpeg/ffprobe）を独立した2種類の更新として管理する。更新の自動インストールは行わない。
- 起動時はアプリ本体をバックグラウンドで確認し、ツール準備完了後にライブラリも1回だけバックグラウンドで確認する。どちらもダイアログは表示せず、更新が見つかった場合だけヘッダー下の右上の更新ステータスピルへ反映する。
- メニューバーの `Tree Movie Downloader` → `アップデートを確認` と About 画面の `アップデートを確認` は、アプリとライブラリの両方を確認し、結果を集約して1つのダイアログで表示する。片方の確認失敗で、もう片方の成功結果・更新情報を消さない。
- ヘッダー下の右上の更新ステータスピルにある `インストール…` は、利用可能な更新をまとめてインストールするための確認ダイアログを開く。両方ある場合はライブラリ→アプリの順に更新し、アプリ更新の成功時に再起動する。ライブラリのみの場合は再起動しない。ライブラリ更新の失敗時はアプリ更新を開始しない。
- ライブラリは `tools-info.json` の `version` / `platform` と `.installed.json` のローカル記録・バイナリの完全性を比較し、更新対象だけを SHA-256 検証して置換する。manifest schema と installed schema は変更しない。
- About 画面はアプリ更新とライブラリ更新を分け、yt-dlp と ffmpeg のバージョンを表示する。詳細は [アプリとライブラリの更新](./feature/update-application.md) を参照する。

## サウンドの再生

- 音声ファイルは同梱します。
- 再生対象のフォルダは以下の通りです。
  - アプリバンドル内の `assets/sounds/success` 。最初から同梱されている `成功音声` のフォルダです。このフォルダには必ず音声ファイルが配置されています。
  - アプリバンドル内の `assets/sounds/error` 。最初から同梱されている `失敗音声` のフォルダです。このフォルダには必ず音声ファイルが配置されています。
  - `~/.config/tree-movie-downloader/sounds/success` 。ユーザーが自分で用意した `成功音声` のフォルダです。ユーザーが任意に配置するので、フォルダ・ファイルが存在しない場合があります。
  - `~/.config/tree-movie-downloader/sounds/error` 。ユーザーが自分で用意した `失敗音声` のフォルダです。ユーザーが任意に配置するので、フォルダ・ファイルが存在しない場合があります。

## 通知

- 起動時に許可を求め、アプリ前面表示中もバナーを出します。

## 国際化

- 日本語・英語に対応します。

## アプリケーション

- アプリケーションアイコンは `assets/images/icon-tree-01.png` を使用します。
- About画面の表示アイコンは `assets/images/icon-tree-01.png` を使用します。

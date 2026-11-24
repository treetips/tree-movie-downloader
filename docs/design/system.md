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

- yt-dlp と ffmpeg はアプリ本体へ同梱せず、GitHub Releases から取得する。
- 配布物は `tools-latest` タグの `tools-info.json` で管理し、実行中の OS と CPU に合う
  `yt-dlp` と `ffmpeg`（`ffprobe` を含む）の ZIP を選ぶ。
- 取得した ZIP は SHA-256 を検証してから `~/.config/tree-movie-downloader/tools/` へ配置する。
- 起動時に必須バイナリが揃っていなければダウンロード画面を表示し、完了するまでメイン画面の
  操作をブロックする。揃っていれば通信せずにメイン画面を表示する。
- ダウンロード中は受信バイト数と Content-Length から進捗率を求め、配置中は不確定表示にする。
  失敗時は理由と再試行ボタンを表示する。
- yt-dlp の実行時は管理下のバイナリを使い、`--ffmpeg-location` で管理下の ffmpeg を渡す。
- 配布物は `.github/workflows/tools-release.yml` と `tools/build-release-assets.sh` で生成する。
  上流の yt-dlp 公式 Releases と `eugeneware/ffmpeg-static` を使用する。

## アップデート

- 起動時にバックグラウンドで GitHub Releases の `update-info.json` を確認し、ダイアログは表示せず
  更新が見つかった場合だけタイトルバー右端の更新エリアへ反映する。
- メニューバーの `Tree Movie Downloader` → `アップデートを確認` とAbout画面の
  `アップデートを確認` から手動で確認できる。手動確認の結果はダイアログと更新エリアへ反映する
  （確認失敗時は更新エリアを変更しない）。
- 更新がある場合は確認ダイアログからインストールする。ダウンロードの進捗、SHA-256検証、
  ZIP展開、`.app` の置き換え、再起動を行う。詳細は
  [アプリケーションの更新](./feature/update-application.md) を参照する。

## サウンドの再生

- 音声ファイルは同梱します。
- 再生対象のフォルダは以下の通りです。
  - アプリバンドル内の `assets/sounds/success` 。最初から同梱されている `成功音声` のフォルダです。このフォルダには必ず音声ファイルが配置されています。
  - アプリバンドル内の `assets/sounds/error` 。最初から同梱されている `失敗音声` のフォルダです。このフォルダには必ず音声ファイルが配置されています。
  - `~/.config/tree-movie-downloader/sounds/success` 。ユーザーが自分で用意した `成功音声` のフォルダです。このフォルダはユーザーが任意に配置するので、フォルダ・ファイルが存在しない場合があります。
  - `~/.config/tree-movie-downloader/sounds/error` 。ユーザーが自分で用意した `失敗音声` のフォルダです。このフォルダはユーザーが任意に配置するので、フォルダ・ファイルが存在しない場合があります。

## 通知

- 起動時に許可を求め、アプリ前面表示中もバナーを出します。

## 国際化

- 日本語・英語に対応します。

## アプリケーション

- アプリケーションアイコンは `assets/images/icon-tree-01.png` を使用します。
- About画面の表示アイコンは `assets/images/icon-tree-01.png` を使用します。

# 外部ツールを GitHub Releases から起動時にダウンロードする

## ステータス

- 承認済み (Accepted)

## 背景・課題

これまで yt-dlp と ffmpeg は、利用者の PATH 上または Homebrew のインストール先から見つけて実行していた。
そのため、アプリを起動しても次の問題があった。

- 利用者が事前に yt-dlp と ffmpeg をインストールしていないと、ダウンロードを開始できない。
- PATH 上のバージョンが環境ごとに異なり、動作確認した組み合わせを保証できない。
- アプリ本体へ同梱すると、ffmpeg を含む配布物が数百MB規模になり、ツールだけを更新できない。
- ツールの不具合修正を配るためだけに、アプリ本体の再リリースが必要になる。

[tree-image-optimizer](https://github.com/treetips/tree-image-optimizer) では、アプリ本体と外部ツールを分離し、ツール単位のアーカイブと `tools-info.json` を GitHub Releases で配布している。同じ方式を採用し、yt-dlp と ffmpeg を
初回起動時にだけ取得する。

## 決定

- アプリ本体に yt-dlp と ffmpeg を同梱しない。
- `yt-dlp` と `ffmpeg`（`ffprobe` を含む）をプラットフォーム別の ZIP にまとめ、
  `tools-latest` タグの GitHub Releases で配布する。
- 配布物の一覧は `tools-info.json` とし、アプリは起動時に次の URL から取得する。
  - `https://github.com/treetips/tree-movie-downloader/releases/download/tools-latest/tools-info.json`
- `tools-info.json` は次の形式とする。

```json
{
  "schemaVersion": 1,
  "tools": [
    {
      "name": "yt-dlp",
      "version": "2026.10.26",
      "platform": "macos-aarch64",
      "url": "https://github.com/treetips/tree-movie-downloader/releases/download/tools-latest/yt-dlp-macos-aarch64-2026.10.26.zip",
      "sha256": "<hex>",
      "required": true
    },
    {
      "name": "ffmpeg",
      "version": "b6.1.1",
      "platform": "macos-aarch64",
      "url": "https://github.com/treetips/tree-movie-downloader/releases/download/tools-latest/ffmpeg-macos-aarch64-b6.1.1.zip",
      "sha256": "<hex>",
      "required": true
    }
  ]
}
```

- 対応プラットフォームは `macos-aarch64` `macos-x86_64` `windows-x86_64`
  `linux-x86_64` `linux-aarch64` とする。
- 取得した ZIP は配置前に SHA-256 で検証する。不一致なら配置せず、エラーとして再試行を促す。
- 一時的なネットワークエラーは指数バックオフで最大3回まで再試行する。HTTP 404 などの恒久的な
  エラーとチェックサム不一致は再試行しない。
- ツールは `~/.config/tree-movie-downloader/tools/` 配下へ次の構成で配置する。
  - `yt-dlp/yt-dlp`
  - `ffmpeg/ffmpeg`
  - `ffmpeg/ffprobe`
- インストール済みバージョンは `tools/.installed.json` に記録する。現時点では起動時の
  欠損検知に使い、ツール単体の更新UIは別途追加する。
- アプリ起動時に必須バイナリが揃っていなければ、ダウンロード画面を表示してメイン画面の操作を
  ブロックする。揃っていれば通信せずにメイン画面を表示する。
- ダウンロード画面には、確認中・ダウンロード中（受信バイト数と Content-Length による進捗）・
  配置中・失敗（再試行ボタン）の状態を表示する。
- yt-dlp の実行時は管理下のバイナリを優先し、`--ffmpeg-location` に管理下の ffmpeg ディレクトリを
  渡す。`YT_DLP_PATH` は開発時の差し替え用として残す。
- 配布物は `.github/workflows/tools-release.yml` から生成する。上流の yt-dlp 公式 Releases と
  `eugeneware/ffmpeg-static` の Releases を取得し、プラットフォームごとの ZIP と
  `tools-info.json` を作り直して `tools-latest` リリースへ上書きアップロードする。
  `tools-latest` は pre-release として扱い、GitHub の「Latest」がアプリ本体のリリースを
  指したままになるようにする。

## 代替案

- yt-dlp と ffmpeg をアプリ本体へ同梱する
  - 却下: ffmpeg だけで数十MBから数百MBになり、ツールの修正のたびにアプリ全体を再配布することになる。
- 利用者の PATH 上の yt-dlp と ffmpeg を使い続ける
  - 却下: 事前インストールが必須で、バージョンの組み合わせも保証できない。
- 上流の Releases から直接ダウンロードする
  - 却下: 配布形式・アセット名・チェックサムの有無が配布元ごとに異なり、OS ごとの正規化と
    検証をアプリ側で吸収できなくなる。自前の Releases に正規化したアセットを置く。
- すべてのツールを1つの ZIP にまとめる
  - 却下: ffmpeg だけを更新する場合も無関係な yt-dlp を取得することになり、差分更新の余地がなくなる。

## 結果

- 初回起動時に yt-dlp と ffmpeg が自動で揃い、利用者は Homebrew などを別途導入しなくてよい。
- アプリ本体の配布物に大きなバイナリを含めずに済む。
- ツールの取得元とチェックサムが一元管理され、改ざん・破損したアーカイブを配置前に検出できる。
- ツール単体の更新通知は今回の範囲外とし、`tools/.installed.json` を次回の更新UIで利用する。

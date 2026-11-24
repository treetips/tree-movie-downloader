# シェルスクリプトからGPUIアプリへの移行

- 現在のプロジェクト `/Users/tester/work/youtube-downloader` はシェルスクリプトで実装している。
- `yt-dlp` を使っている。
- 720p等の画質変更や、ダウンロードする場所を `手動で` コメントアウトを切り替えて対応している。
- 実行ログもターミナルに流れていってしまい、閲覧しにくい。

## 達成したいこと

- [GPUI](https://gpui-ce.github.io/) と [GPUI Kit](https://gpui-kit.com/) でGUIアプリ化する。
- GitHub Releasesでリリースできるようにする。
- 画質・音質・コーデック等をGUIで選択できるようにする。
- プログレスバー等で進捗状況を把握しやすくする。
- YouTubeがエラーを返してくる事が結構多いので、自動リトライと、諦めて次のファイルを処理する機能。
- YouTube APIは使わない（依存したくない）。
- `/Users/tester/Sources/github.com/tree-image-optimizer` と同様に、表形式でキュー形式で順次DLする。
- YouTubeの制限が厳しいの1ファイルづつDLする。
- 同一のチャンネルのURLを一気に登録したい (/Users/tester/work/youtube-downloader/url.txt のように) 。

# PR補助ワークフローの自己修復と再試行

- ステータス: Accepted
- 日付: 2026-11-26

## 背景

[Label PR](https://github.com/treetips/tree-movie-downloader/actions/runs/45650058576)
は、リリースノートの分類に使う `feature` ラベルが存在しない状態でラベル付与に失敗した。
[Auto Assign PR Creator](https://github.com/treetips/tree-movie-downloader/actions/runs/45650058573)
は、Assignee APIが一時的な HTTP 500 を返したため失敗した。

ラベルはリポジトリの初期状態だけでは必要な種類がそろわず、手動で作成する運用は
再現性とメンテナンス性が低い。一方、GitHub APIの一時的な障害はワークフローの修正では
発生自体を防げない。

## 決定

- Label PR は `.github/release.yml` の分類に使うラベルを、不足している場合のみ作成する。
- ラベル作成には `issues: write` 権限を付与し、ラベル付与には `pull-requests: write`
  権限を使う。
- Auto Assign PR Creator は、GitHub APIへのリクエストを最大3回まで自動リトライする。
- HTTP 400、401、403、404、422 は恒久的な失敗としてリトライしない。

## 理由

必要なラベルをワークフロー内で整えることで、リポジトリのラベル設定に依存せず
PR作成時に安定して分類できる。既存ラベルの名前・説明・色は上書きしない。

Assignee APIの HTTP 500 は一時的な障害とみなせるため、リトライで復帰を待つ。
恒久的なクライアントエラーまで再試行すると、失敗の検出が遅れるため除外する。

## 結果

- PR補助ワークフローが手動のラベル設定やAPIの一時的な障害に強くなる。
- `GITHUB_TOKEN` の権限は各ワークフロー内に限定して追加する。
- リトライ回数を超える障害や権限不足は、引き続きワークフロー失敗として可視化する。

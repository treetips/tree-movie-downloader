# RustアプリのCIとmacOSリリース

## ステータス

- 承認済み (Accepted)

## 背景・課題

`.github/workflows/ci.yml` と `.github/workflows/release.yml` は、別プロジェクトのSwiftUIアプリ向けの内容が残っていた。
このプロジェクトはRustとGPUIで実装しており、Xcodeプロジェクトや `Info.plist` をビルド元にはしていない。
そのため、次の点をこのプロジェクトに合わせる必要があった。

- CIで `rust-toolchain.toml` のRustを使い、整形・lint・テスト・ビルドを実行する。
- リリースでは `cargo build --release` の成果物をmacOSの `.app` にまとめる。
- アプリは起動時に外部ツールを取得するため、ツール配布が無い状態でリリースしない。
- バージョンは `Cargo.toml` とGitタグ、macOSバンドルの表示を一致させる。

## 決定

### CI

- `macos-latest` で実行する。
- `rust-toolchain.toml` に固定したRustを導入し、`cargo fmt --check`、`cargo clippy --all-targets --locked -- -D warnings`、
  `cargo test --locked`、`cargo build --locked` を順に実行する。
- Cargoのキャッシュを使い、同じブランチの実行は打ち切る。
- 対象パスはRustソース・アセット・依存ファイル・Rustツールチェーン・CIワークフローに限定する。

### リリース

- `workflow_dispatch` で `major` / `minor` / `patch` を選ぶ。
- バージョンは最新の `v*` タグと指定したbumpから算出する。初回リリース（`v*` タグが無い場合）は
  `Cargo.toml` のバージョンをそのまま使う。
- リリースビルドでは `TMD_APP_VERSION` をビルド時に注入し、アプリのタイトルバーとAboutの表示に使う。
  `Cargo.toml` は開発用のバージョンのままにし、リリースのためだけにファイルを書き換えない。
- `cargo build --release --locked` のバイナリを `.app` の `Contents/MacOS/` へ配置する。
- `.app` には `Info.plist`、アプリアイコン、`LICENSE`、`THIRD_PARTY_NOTICES.md`、
  同梱アセット（サウンド・背景画像）を含める。
- アセットは `.app` の `Contents/Resources/assets` から読む。開発時はリポジトリ直下の `assets` を読む。
- 署名はApple Developer Program未加入のためad-hocで行う。Gatekeeperの警告は残る。
- 配布物は `.app` をZIP化したものと `update-info.json`、`SHA256SUMS.txt`、`LICENSE`、`THIRD_PARTY_NOTICES.md` とする。
- アプリは初回起動時に `tools-latest` から外部ツールを取得するため、リリース前に
  `tools-info.json` が取得できることを確認する。無ければリリースを失敗させる。
- 外部ツールの配布は `tools-release.yml` に分離したままにする（[ADR 0018](./0018-download-tools-from-releases.md)）。
- Gitタグはリリース元のコミットに付け、GitHub Releaseを作成する。

## 代替案

- SwiftUI版の `xcodebuild` を使う
  - 却下: このプロジェクトにXcodeプロジェクトは無く、Rustのビルド成果物を扱えない。
- バイナリだけを配布する
  - 却下: アプリケーションバンドルでないと、アイコン・バンドル識別子・リソースの解決ができず、
    ユーザーの起動体験も悪い。
- アプリ本体のリリースに外部ツールを同梱する
  - 却下: 数百MBのツールをアプリ更新のたびに配布することになり、ADR 0018の方針に反する。
- CIで `Cargo.toml` を書き換えてからタグを付ける
  - 却下: タグが指すコミットとビルド時のバージョンがずれ、作業ツリーも汚れる。
    今回はビルド時注入で表示バージョンだけを差し替える。
- 初回からUniversal Binaryを作る
  - 却下: 現時点では `macos-latest` のネイティブビルドで十分であり、Intel向けの検証が別途必要になる。
    必要になった時点でターゲット追加と `lipo` を検討する。

## 結果

- CIはRustの整形・lint・テスト・ビルドをmacOS上で確認できる。
- リリースは `.app` として配布でき、アイコン・同梱アセット・バージョン表示が揃う。
- 外部ツールの配布が無いままアプリをリリースする事故を防げる。
- 現状の配布バイナリは `macos-latest` のネイティブアーキテクチャのみで、Intel向けUniversal Binaryは未対応。

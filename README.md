# sdGalleryImporter

AI 画像 CG 集を「お手本」として取り込み、画像ごとの txt サイドカーと共有 SQLite
（ギャラリー＝下書きストーリー）を同時に作る Windows 向け単体 exe。

- 仕様書（正本）: [`docs/261127_plan-sd-gallery-importer.md`](docs/261127_plan-sd-gallery-importer.md)
- Phase 0 実測記録: [`docs/phase0/measurement.md`](docs/phase0/measurement.md)

## 現在の状態

| Phase | 内容 | 状態 |
|---|---|---|
| 0 | 実測とフィクスチャ | 完了 |
| 1 | txt ＋ ログ ＋ D&D（DB なし） | 完了（`docs/phase1/report.md`） |
| 2 | DB 取り込み（NovelAI） | 完了（`docs/phase2/report.md`） |
| 3 | 他形式の抽出（A1111 / SwarmUI / InvokeAI） | 完了（`docs/phase3/report.md`） |
| 4 | ComfyUI 辿り | 未着手 |

## 使い方（Phase 2 時点）

exe にフォルダをドラッグ＆ドロップするか、CLI でフォルダを渡します。

```powershell
# 複数フォルダを一括処理（txt サイドカーとログを出力）
.\sdGalleryImporter.exe "D:\CG\gallery-a" "D:\CG\gallery-b"

# サブフォルダも常に処理する
.\sdGalleryImporter.exe --recursive "D:\CG"

# 書き込み内容の予測だけをログに出す
.\sdGalleryImporter.exe --dry-run "D:\CG\gallery-a"

# 自ツール製 txt のみ上書きする
.\sdGalleryImporter.exe --overwrite "D:\CG\gallery-a"

# DB パスの上書き
.\sdGalleryImporter.exe --db "D:\data\story.db" "D:\CG\gallery-a"

# バージョン
.\sdGalleryImporter.exe --version
```

Phase 2 では exe 隣（または `--db` / 設定の `db_path`）の SQLite に、ギャラリー・ページ・
segment・行・下書きストーリー・配置を書き込みます。画像は移動・改変しません。

## 依存とビルド

- [fast_meta v2.1.0](https://github.com/wai55555/checkImageMetadata) をライブラリとして
  プロセス内で使う（子プロセス起動なし）。Cargo の git tag 依存で固定。
- fast_meta の既定 feature `zlib-ng` は CMake を要求するため、本リポジトリでは
  `default-features = false, features = ["portable-zlib"]` を使う。展開結果は同一で、
  txt のバイト列にも影響しない。
- SQLite は `rusqlite` の `bundled` を使う。

```powershell
cargo build --release --locked
cargo test --locked
```

生成物: `target\release\sdGalleryImporter.exe`

## 開発用ツール

```powershell
# Phase 0 フィクスチャを実ファイルとして書き出す（目視確認・CLI 比較用）
cargo run --example write_fixtures -- target\phase0-fixtures

# 実サンプルギャラリーの run 構成・欠落ページを実測
cargo run --release --example phase0_survey -- "E:\path\to\gallery"

# 処理速度の実測（--bulk で合成、--bulk-copy で実画像コピー）
cargo run --release --example phase0_bench -- "E:\path\to\gallery"
```

## 非目的

- 層分解（キャラ・画風・品質の分離）は行わない。列（`derived_from_line_id`）だけ用意する。
- 画像の移動・コピー・改変は行わない。書き込みは txt サイドカー・DB・ログのみ。
- seed は参照しない。試行回数・難易度の推定もしない。

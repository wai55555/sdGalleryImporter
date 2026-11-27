# Phase 1 完了報告 — txt ＋ ログ ＋ D&D（DB なし）

- 実施日: 2026-11-28
- ブランチ: `phase/1-txt-log-dnd`
- 対象仕様書: `docs/261127_plan-sd-gallery-importer.md` §5 / §8 / §10 Phase 1

## 1. 実装した範囲

| 仕様 | 実装 |
|---|---|
| §3.1 ギャラリー境界 | 直下に画像があればリーフ、無ければコンテナ、画像もサブフォルダも無ければ空フォルダとしてスキップ |
| §3.2 ページ順 | REQ-3.7 の自然順（`src/natural.rs`、単体テスト付き） |
| §3.2 列挙 | `.png/.jpg/.jpeg/.webp` を大小文字無視で直下のみ列挙（REQ-3.4, REQ-3.8） |
| §5 txt | 画像ごとのサイドカー。`extract_to_writer` の出力をそのまま書き、索引ファイルは作らない |
| §5.3 衝突 | 見出し行を除いた本文のバイト一致 → 自ツール製。`_metadata` 退避、既定スキップ、`--overwrite` は自ツール製のみ |
| §8.2 引数 | `--db` / `--overwrite` / `--recursive` / `--dry-run` / `--version` と位置引数。不明オプション・`--db` 欠落・重複は引数エラー |
| §8.2 設定 | exe 隣の `sdGalleryImporter.toml` の `db_path`。不正 TOML・非文字列は異常終了。DB は開かない |
| §8.3 ログ | exe 隣に `import_YYYYMMDD_HHMMSS.log`（dry-run は `import_dryrun_`）。衝突時 `_2`、作れなければカレント、それも駄目ならコンソール |
| §9.1 同一性 | `canonicalize` から `\\?\` 除去・区切り統一・末尾除去・小文字化したキーで引数重複と循環を判定 |
| §9.2 エラー | ページ単位の失敗・読めない画像・空フォルダ・サブフォルダ無視をログに残して続行。終了コードは全体異常のみ 1 |

Phase 1 では DB を開かない。`story.db` も作らない。segment 数・story_id・`import_runs`・
全体合計はログに出さない（REQ-8.8 の Phase 1 向け範囲）。

## 2. 実装上の判断（仕様の解釈）

1. **見出し行の比較は両側で行う**。REQ-5.3 は「出力の見出し行を除いた本文で比較」と
   書くが、既存 txt も自ツールが書いたものであれば見出し行を持つ。既存側も
   `=== <形式> File: <パス> ===` の形なら見出しを除いて比較する。これにより、
   フォルダ移動後にパスだけ変わった自ツール製 txt を正しくスキップできる
   （統合テストで確認）。
2. **`source = error` 相当の判定**は Phase 1 ではログの「読めなかったページ」として扱う。
   fast_meta が `Err` を返した場合と、`ScanOutcome.format = None`（内容が画像でない）場合。
3. **訪問済みフォルダは全体で1回**。親コンテナを処理した後に同じ子を引数で渡した場合も
   「訪問済み」としてスキップし、二重処理しない（ジャンクションの循環対策と同じ仕組み）。
4. **`--recursive` はリーフにも適用**。リーフ自身を処理したうえで子フォルダも処理する。
   コンテナは指定に関係なく常に子を処理する（REQ-3.2）。

## 3. テスト結果

```
cargo test
  unit (lib):            23 passed
  fixtures_smoke:        17 passed
  phase1_txt_log:         6 passed
  doc tests:              0
cargo clippy --all-targets: warning 0
```

Phase 1 の統合テスト（`tests/phase1_txt_log.rs`、ビルド済み exe を一時フォルダへコピーして実行）:

| テスト | 確認内容 |
|---|---|
| `multi_folder_run_writes_sidecars_and_log` | 複数フォルダ・空フォルダ・ファイル引数・重複引数（末尾スラッシュ／大小文字）を一括処理。txt が CLI と同一、メタデータなし・壊れた画像には txt なし、サブフォルダ無視ログ、画像不変、DB 未作成、`_gallery.txt` なし |
| `rerun_skips_falls_back_and_overwrites_only_own_files` | 再実行スキップ、`--overwrite`、他者 txt の `_metadata` 退避、両方他者の保護と警告、同 stem 異拡張子、フォルダ移動後の見出し行差の吸収、`_metadata` のみ上書き |
| `container_recursive_and_natural_order` | コンテナ再帰、`--recursive` のリーフ兼用、無視サブフォルダの記録、自然順（`1` < `01` < `001` < `2` < `10`、大文字小文字、全角数字）、ジャンクション循環と別名重複 |
| `dry_run_predicts_without_writing` | txt を書かない、`import_dryrun_` ログ、予測 status、DB 未作成、既存 txt は `skipped_exists` 予測 |
| `version_argument_errors_and_config` | `--version` はログなし終了 0、引数エラー優先、`--db` 欠落・重複、有効フォルダなし、設定不正（`--db` 併用時も異常）、既定 `story.db`、相対 `--db` |
| `one_failed_page_or_gallery_does_not_stop_the_run` | txt 書込失敗（`001.txt` がディレクトリ）と壊れた画像があっても、兄弟ギャラリーは処理される |

## 4. ログの例（抜粋）

```
sdGalleryImporter 0.1.0
実行日時: 2026-11-28T05:22:19.3565133+09:00
モード: 通常
DB: C:\...\target\debug\story.db (由来: 既定 / Phase 1 では使用しない)
オプション: overwrite=false recursive=false dry_run=false

=== ギャラリー: C:\...\g1 ===
種別: リーフ
ページ数: 19
  ページ seq=0000 file=a1111.jpg status=written records=1 format=jpeg
  ...
  ページ seq=0008 file=no_metadata.jpg status=none records=0 format=jpeg
集計: pages=19 written=15 overwritten=0 skipped_exists=0 fallback_metadata=2 failed=0 none=2
メタデータなし: 2
読めなかったページ: 0
未抽出ページ:
  seq=0008 file=no_metadata.jpg reason=メタデータなし
  seq=0009 file=no_metadata.png reason=メタデータなし
```

## 5. 実 DB・実画像への影響

Phase 1 の検証はすべて一時フォルダのフィクスチャで行った。実サンプルギャラリー
（`E:\pixiv_ai\...`）には txt を書いていない。実 DB も開いていない。

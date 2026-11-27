# Phase 2 完了報告 — DB 取り込み（NovelAI）

- 実施日: 2026-11-28
- ブランチ: `phase/2-db-novelai`
- 対象: 仕様書 §4 / §6.1（NovelAI）/ §6.5 / §7 / §9 / §10 Phase 2

## 1. 実装した範囲

| 仕様 | 実装 |
|---|---|
| §4.1–§4.3 スキーマ | 共有テーブルと追加テーブルを冪等に作成。追加列を存在チェック付きで ALTER |
| REQ-4.3 | 接続直後に `PRAGMA foreign_keys = ON`。スキーマ・列追加・バックフィル・索引を1トランザクションで実行 |
| REQ-4.1 ハッシュ | `SHA-256(prompt || \x00 || negative)` の小文字16進（`src/hash.rs`） |
| REQ-4.2 行照合 | `prompt_hash` で検索し、一致した既存行（最小 id）を再利用。新規行は `origin='cg_import'` |
| REQ-4.4 | リーフ1件を1トランザクション。コンテナ行は子の処理前に単独トランザクションで確定 |
| REQ-4.5 | galleries → stories → galleries.story_id → lines → segments → pages → placements の順で生成 |
| REQ-6.1 | NovelAI の `Description` / `Comment.prompt` と `Comment.uc` を抽出 |
| REQ-6.5 | PNG IHDR / JPEG SOF / WebP VP8X・VP8L・VP8 から実寸を取得 |
| §7 | pos+neg の完全一致した連続区間を segment 化。`repeat = run_length` |
| REQ-9.1 | `root_path` は既存の正規化キー（小文字・実パス） |
| REQ-9.2 | 取り込み済みリーフは DB 書き込みのみスキップし、txt は通常処理。コンテナは再利用して未取り込みの子だけ処理 |
| REQ-9.2 役割不一致 | コンテナ→リーフは変換して `import_run_id` / `imported_at` を更新。リーフ→コンテナはリーフのまま子を処理 |
| REQ-8.10 | `import_runs` を作成し、終了時に集計値と `finished_at` を更新。`log_path` は実際のログの絶対パス |
| REQ-8.5 dry-run | DB を読み取り専用で開き、スキーマ変更・書き込み・`import_runs` を作らない。segment 構成と合計は予測として出力 |

## 2. 実装上の判断

1. **スキーマ用意の失敗は全体異常**。1トランザクションでロールバックし、`import_runs` を作らない
   （REQ-8.10 と §11-10）。テストで「列追加・テーブル作成が残らない」ことを確認。
2. **バックフィル後の重複**は `prompt_hash` ごとに警告ログを出して続行し、照合は最小 id。
3. **スキップしたリーフのサマリ**は、segment 数・メタデータなし数・未抽出一覧を既存 DB から読む
   （REQ-8.8）。ページ単位の txt_status は今回の実行結果を出す。
4. **Phase 2 の抽出は NovelAI のみ**。A1111 / ComfyUI / SwarmUI / InvokeAI は Phase 3/4 で追加する。
   それらの画像は現時点では `source = unknown` の未抽出として記録される。
5. **dry-run で追加テーブルが無い DB**は、スキーマを作らず全フォルダを未取り込みとして扱う。
6. 画像1枚につき「レコード走査」「txt 生成」「寸法取得」で複数回ファイルを読む。Phase 0 の
   実測では余裕があるため、まず正しさを優先した（最適化は必要になった時点で行う）。

## 3. テスト結果

```
cargo test --locked
  単体:               29 passed
  fixtures_smoke:     17 passed
  phase1_txt_log:      6 passed
  phase2_db_novelai:   6 passed
cargo clippy --all-targets: warning 0
```

Phase 2 の統合テスト:

| テスト | 確認内容 |
|---|---|
| `novelai_gallery_builds_story_segments_and_placements` | galleries / stories / pages / segments / lines / placements の内容、`repeat = run_length`、行の再利用、pos のみ neg 空、寸法、`import_runs` の集計とログ一致、ISO 8601 |
| `reimport_skips_db_writes_and_reuses_db_stats` | 再実行で全テーブルが増えない、2回目の集計 0、DB の segment 数・未抽出をログに出す、txt 非上書き、`import_run_id` / `imported_at` 不変 |
| `container_children_and_role_mismatch` | コンテナ3行（親子）、新規子だけ取り込み、リーフ→コンテナ、コンテナ→リーフ変換と実行更新 |
| `legacy_db_backfill_reuses_min_id_and_failure_rolls_back` | 既存行のバックフィル、重複警告、最小 id 採用、スキーマ失敗の完全ロールバック |
| `dry_run_predicts_without_touching_db` | DB 未作成、txt 未作成、予測セグメント、import_runs / galleries 不変、未初期化 DB にスキーマを作らない |
| `db_open_failure_is_global` | 開けない DB は終了コード 1 でログに理由 |

## 4. 完了条件の確認

- サンプルギャラリー（pos 同一 neg 差分、連続 run、メタデータなし、pos のみ）を取り込み、
  §4・§7 どおりの行が作られ、`placements.repeat` が `segments.run_length` と一致することを確認。
- 同じフォルダの再実行で galleries / pages / segments / lines / stories / placements が増えないことを確認。

## 5. 実 DB・実画像への影響

検証はすべて一時フォルダのフィクスチャと検証用 DB で行った。実サンプルギャラリーや実 DB には
触れていない。

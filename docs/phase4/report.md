# Phase 4 完了報告 — ComfyUI 辿り

- 実施日: 2026-11-28
- ブランチ: `phase/4-comfyui`
- 対象: 仕様書 REQ-6.4 / §10 Phase 4

## 1. 実装した範囲

- 入力は `prompt` キー（PNG テキスト／Trailing）と、JPEG・WebP・PNG eXIf の
  UserComment が JSON オブジェクトである場合。
- `{"prompt": <グラフ>, "workflow": ...}` のラッパ形式は `prompt` メンバーをグラフとして使う。
- サンプラーノードは `class_type` が `KSampler` / `KSamplerAdvanced` で、
  `positive` / `negative` 入力を持つものだけ。`SamplerCustom` /
  `SamplerCustomAdvanced` は対象外。
- サンプラーがちょうど 1 個のときだけ辿る。複数なら未抽出。
- `positive` / `negative` の直接リンク先が `CLIPTextEncode` で、`inputs.text` が
  文字列のときだけ採用。リンク先が別ノードならその方向は未抽出。
- pos だけ辿れた場合は neg = 空（REQ-6.3）。
- 辿れない場合は未抽出として記録し、取り込みは止めない。raw は txt に残る。

優先順位は Phase 3 のまま: NovelAI → ComfyUI → A1111 → SwarmUI → InvokeAI。
ComfyUI が辿れる場合は A1111 より優先され、辿れない場合は A1111 が拾う。

## 2. テスト結果

```
cargo test --locked
  単体:               44 passed
  fixtures_smoke:     17 passed
  phase1_txt_log:      6 passed
  phase2_db_novelai:   6 passed
  phase3_formats:      2 passed
  phase4_comfyui:      1 passed
  final_acceptance:    2 passed
cargo clippy --all-targets: warning 0
```

Phase 4 のテスト:

| テスト | 確認内容 |
|---|---|
| `comfyui_traversal_and_fallbacks` | 実物の ComfyUI グラフ（PNG）と JPEG UserComment ラッパで同じ行が作られ 1 run になること、複数サンプラーで未抽出、SamplerCustom で未抽出、ComfyUI が辿れる場合は A1111 より優先、辿れない場合は A1111 が拾うこと、寸法 |
| 単体 `comfyui_*` | 直接リンク、ラッパ、KSamplerAdvanced、複数サンプラー、ConditioningCombine、`text` がリンク、SamplerCustom の除外 |

## 3. 最終受け入れ（§11）の確認

| §11 | 確認したテスト |
|---|---|
| 1 複数フォルダ・失敗継続・重複引数・空フォルダ | `multi_folder_run_writes_sidecars_and_log`, `one_failed_page_or_gallery_does_not_stop_the_run`, `leaf_db_failure_rolls_back_and_other_galleries_continue` |
| 2 txt 同一性・索引なし・衝突順 | `multi_folder_run_writes_sidecars_and_log`, `rerun_skips_falls_back_and_overwrites_only_own_files` |
| 3 ギャラリー→ページ→segment→配置→story | `novelai_gallery_builds_story_segments_and_placements`, `container_children_and_role_mismatch` |
| 4 pos 同・neg 異、連続 run、pos のみ | `novelai_gallery_builds_story_segments_and_placements` |
| 5 メタデータなしの分断 | `novelai_gallery_builds_story_segments_and_placements`, `comfyui_traversal_and_fallbacks` |
| 6 再取り込みで増えない・子の追加 | `reimport_skips_db_writes_and_reuses_db_stats`, `container_children_and_role_mismatch` |
| 7 seed・パラメータ差で分かれない／正規化しない | `parameters_and_normalization_do_not_change_segments` |
| 8 ログが残る | 各統合テスト（ログの status・未抽出・合計） |
| 9 画像が改変されない | `multi_folder_run_writes_sidecars_and_log`, `parameters_and_normalization_do_not_change_segments` |
| 10 バックフィル・重複警告・ロールバック | `legacy_db_backfill_reuses_min_id_and_failure_rolls_back` |
| 11 大小文字・末尾スラッシュ・ジャンクション | `multi_folder_run_writes_sidecars_and_log`, `container_recursive_and_natural_order` |

## 4. 残課題・注意

- 実サンプル CG 集での通し確認と、実 DB への取り込みは行っていない。仕様書の運用に従い、
  Phase 4 完了版から実 DB を使ってよい段階になった。
- 画像1枚につき複数回ファイルを読む実装のため、大規模実行では I/O が支配的になる。
  必要になった時点で、レコード走査と txt 生成の統合などの最適化を検討する。

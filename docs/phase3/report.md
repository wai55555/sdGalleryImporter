# Phase 3 完了報告 — A1111 / SwarmUI / InvokeAI 抽出

- 実施日: 2026-11-28
- ブランチ: `phase/3-other-formats`
- 対象: 仕様書 §6.1 / REQ-6.2 / REQ-6.3 / REQ-6.7 / §10 Phase 3

## 1. 実装した範囲

| 形式 | 取得元 | pos | neg |
|---|---|---|---|
| A1111 / Forge | PNG `parameters`、Trailing `parameters`、JPEG COM、EXIF UserComment、EXIF ImageDescription | `Negative prompt:` 行より前 | `Negative prompt:` 行からパラメータ行まで |
| SwarmUI | PNG `parameters` 内の `sui_image_params`、直接 `sui_image_params`、JPEG UserComment、EXIF Model | `prompt` | `negativeprompt` |
| InvokeAI（現行） | `invokeai_metadata` | `positive_prompt` | `negative_prompt` |
| InvokeAI（旧） | `sd-metadata` | `image.prompt`（文字列／配列） | 空（旧形式に負のプロンプトが無い） |
| 生成元不明 | PNG `Description` | `Description` | 空 |

- REQ-6.7 の振り分けを実装。XMP 由来（`*Xmp`）のレコードは候補から除外。
- 複数候補の優先順位は NovelAI → ComfyUI → A1111 → SwarmUI → InvokeAI。
  pos が取れた最初の候補を採用し、どれも取れない場合は最上位の「判定済み」形式を
  `source` に記録する（REQ-6.1 総括）。
- 旧 InvokeAI の配列は、決定 D0-1（C）どおり `(<prompt>:<weight>)` を `, ` で連結して
  weight を保持する。weight が無い要素は prompt のみ、空 prompt は飛ばす。
- ComfyUI は `prompt` キー／UserComment JSON の検出まで行う。辿りは Phase 4。

## 2. 実装上の判断

1. **A1111 の `Negative prompt:` 直後の空白 1 個**はラベルの区切りとして除去する。
   仕様の「接頭辞を取り除いた残り」と「先頭・末尾のその他の空白は保持」を、
   実データの `Negative prompt: <neg>` に合わせて解釈した。2個目以降の空白は保持する。
2. **`parameters` 形式の判定**（COM・UserComment・ImageDescription 経由）は、
   `Negative prompt:` 行があるか、末尾の行が `Steps:` で始まる場合とする。
   PNG の `parameters` キーワードは内容に関係なく A1111 候補とする。
3. **`parameters` の JSON が `sui_image_params` を持つ場合**は SwarmUI 候補とし、
   A1111 候補にはしない（実 SwarmUI の PNG 経路）。
4. **旧 InvokeAI の weight 表記**は serde_json の数値表記を使う（`1.0` は `1.0`、
   `0.80` は `0.8`）。数値でない weight は無視して prompt のみ。
5. **生成元不明の Description 採用**は、全候補が pos を取れなかった場合に限る。
   XMP の `description` は対象外（REQ-6.7）。

## 3. テスト結果

```
cargo test
  単体:               40 passed（A1111 分割・XMP 除外・優先順位・旧 InvokeAI 等）
  fixtures_smoke:     17 passed
  phase1_txt_log:      6 passed
  phase2_db_novelai:   6 passed
  phase3_formats:      2 passed
cargo clippy --all-targets: warning 0
```

Phase 3 統合テスト:

| テスト | 確認内容 |
|---|---|
| `extracts_a1111_swarmui_and_invokeai_and_merges_across_formats` | A1111 の 5 経路（PNG/JPEG UserComment/COM/Trailing/EXIF ImageDescription）、SwarmUI の 3 経路、InvokeAI 新旧、Description のみ、XMP のみ、pos 無し A1111、WebP UserComment。形式が違っても同一 pos+neg が 1 segment にまとまり、行が再利用されること。寸法（PNG/JPEG/WebP） |
| `priority_and_detection_without_pos` | NovelAI > A1111 > SwarmUI の優先順位、直接 `sui_image_params`、InvokeAI、pos が取れない場合も生成元を記録 |

A1111 分割の単体テスト: neg あり/なし、Steps あり/なし、CRLF、末尾空行、複数の
`Negative prompt:` 行、`Negative prompt:` の残りが空、末尾以外の `Steps:` 行、
`parameters` 形式の判定。

## 4. 完了条件の確認

各形式のフィクスチャで pos/neg が行化されること、形式をまたいでも pos+neg の
文字列完全一致で segment が一貫することをテストで確認した。

## 5. 残り

ComfyUI のノードグラフ辿りは Phase 4 で実装する。Phase 3 の時点でも生成元の検出は
行うため、辿れない ComfyUI 画像は `source = comfyui` の未抽出として記録される。

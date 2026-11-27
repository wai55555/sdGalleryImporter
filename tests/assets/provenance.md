# フィクスチャ用メタデータの採取元

`tests/assets/` には、実物の画像から取り出した**メタデータ本文だけ**を置いている。
画像そのものはコミットせず、テスト時に小さなコンテナ（PNG/JPEG/WebP）へ埋め込む。

| ファイル | 採取元 | 備考 |
|---|---|---|
| `novelai/description.txt` | `E:\pixiv_ai\[POKI (118145342)] 250930 Kanna Kamui(80p) (AI Generated)\01_20250930_184959.png` の PNG `Description` チャンク | 995 バイト。pos 全文 |
| `novelai/comment.json` | 同上の PNG `Comment` チャンク | 5441 バイト。NovelAI 設定 JSON（`prompt` / `uc` を含む） |
| `a1111/parameters.txt` | `E:\pixiv_ai\AI練習帖 (62025033) pixiv\130283596_p0 - レイチェル =アルカード　営み.png` の PNG `parameters` チャンク | 1168 バイト。`Negative prompt:` 行と `Steps:` 行を含む |
| `comfyui/prompt.json` | `E:\pixiv_ai\!退避260829\名無しの召喚士【リク停止中】 (124431838) pixiv\148268406_p0 - お化け屋敷の二人.png` の PNG `prompt` チャンク | 2348 バイト。KSampler 1 個、pos/neg が直接 CLIPTextEncode に繋がる（Phase 4 の辿れる例） |
| `comfyui/workflow.json` | 同上の PNG `workflow` チャンク | UI 形式。辿り対象外（REQ-6.4） |
| `comfyui/prompt_multi_sampler.json` | `E:\pixiv_ai\LOLICON (25066967) pixiv\128438901_p0 - 1.png` の PNG `prompt` チャンク | KSampler が 2 個（3 と 14）。REQ-6.4 により未抽出になる例 |
| `swarmui/image_params.json` | SwarmUI 公式ドキュメント `docs/Image Metadata Format.md` の Full Example | `parameters` チャンク／UserComment／EXIF Model の各経路に同じ JSON を使う。実物の EXIF Model 形式は未確認 |
| `invokeai/metadata.json` | `https://isgenai.com/examples/guide-lab/invokeai-record.png` の `invokeai_metadata` チャンク | 261 バイト。`positive_prompt` / `negative_prompt` を含む実物チャンク |
| `invokeai/sd_metadata.json` | InvokeAI issue #1129 のコメントに貼られた v2.1.2 実ファイルの `sd-metadata` 本文 | `image.prompt` が `[{prompt, weight}]` 配列の実例。負のプロンプトキーは無い |

## 補足

- 実サンプルのパスは測定環境のものであり、リポジトリには含まれない。再現できない場合は
  `tests/assets/` の本文だけでフィクスチャは成立する（コンテナはテストが生成する）。
- `Comment` や `UserComment` は fast_meta が JSON を整形して返すが、assets には**チャンク
  本来の本文**をそのまま保存している。テストは整形後の JSON をパースして検証する。
- `swarmui/image_params.json` は公式ドキュメントの例であり、実ファイルから採取したものでは
  ない。EXIF Model 形式の実物が入手できた時点で差し替える。

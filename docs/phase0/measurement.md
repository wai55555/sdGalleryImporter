# Phase 0 実測記録 — 実サンプル・run 分布・速度・フィクスチャ確定

- 実施日: 2026-11-27
- 対象仕様書: `docs/261127_plan-sd-gallery-importer.md`
- ブランチ: `phase/0-measurement-fixtures`

本書は仕様書 §10 Phase 0 の完了条件に対応する実測記録である。数値はすべて本書記載の
コマンドを再実行して得たものであり、推測値は「外挿」と明記した。

---

## 0. 結論サマリ

| 項目 | 結果 |
|---|---|
| NovelAI 実サンプル | 80 ページの CG 集 1 ギャラリーで実測（全ページ NovelAI 判定） |
| run 分布 | 1 ギャラリー目: 77 segment（74×1, 3×2）。大規模サンプル: 2443 segment、最長 13 連続 |
| メタデータなしページ | 大規模サンプル 3946 ページ中 121。run 分断 125 か所 |
| 速度（ライブラリ内処理） | 実画像 80 枚: 4889 枚/秒（ウォーム）。CLI 子プロセス方式の約 210 倍 |
| 3,000 枚規模 | 2000 枚 3.07GB を 0.406 秒（ウォーム）。10,000 枚は 2 秒（ウォーム）／I/O 依存で数秒〜100 秒 |
| InvokeAI 旧 `sd-metadata` | 実物キー構成を確認し §6 に追記。配列は weight 付きで復元（C）に確定 |
| SwarmUI EXIF Model 形式 | 実物はローカルに無し。経路は実装可能だが発火しない想定（仕様が許容） |
| フィクスチャ | 画像 20 種、フォルダ構成用ヘルパー、既存 DB 2 種を確定。17 テストで検証済み |

---

## 1. 環境

| 項目 | 内容 |
|---|---|
| OS | Windows 11 + WSL2（カーネル `6.18.33.2-microsoft-standard-WSL2`） |
| CPU | 13th Gen Intel Core i5-13600KF（20 スレッド） |
| RAM | 64 GiB（WSL2 からは 32 GiB が見える） |
| ビルド | Windows MSVC `cargo 1.91.1` / `rustc 1.91.1`（WSL から `cargo.exe` を呼ぶ） |
| fast_meta | v2.1.0（git tag）。`default-features = false, features = ["portable-zlib"]` |
| 計測ドライブ | 実サンプルは `E:`、合成・コピーコーパスは `C:`（NTFS） |

`portable-zlib` を使う理由: 既定の `zlib-ng` はビルドに CMake を要求し、本環境に CMake が
無い。`portable-zlib` は pure Rust の deflate 実装で、**展開結果は同一**であり、txt の
バイト列（REQ-5.2）には影響しない。依存バージョンは v2.1.0 のまま固定している。

---

## 2. 実サンプル

### 2.1 主サンプル（run 分布・速度）

- パス: `E:\pixiv_ai\[POKI (118145342)] 250930 Kanna Kamui(80p) (AI Generated)`
- 画像: 80 枚（PNG）、合計 122,951,522 バイト（約 117 MiB）
- 非画像: `info.txt` 1 件（REQ-3.8 によりページに数えない）
- 生成元: 80/80 が NovelAI。`Description` と `Comment` の両方を持つ。

### 2.2 大規模サンプル（run 分布・欠落ページ）

- パス: `E:\pixiv_ai\ryopp (12680620) pixiv`
- 画像: 3946 枚（PNG）
- 生成元: novelai 3821 / unknown 4 / none 121 / error 0

大規模サンプルは 1 ギャラリーとしては大きいが、run の出方とメタデータ欠落の実態を
確認するために使った。Phase 0 の最低要件（数十〜数百枚の NovelAI CG 集 1 ギャラリー）は
2.1 で満たしている。

---

## 3. fast_meta v2.1.0 の実物出力

`fast_meta.exe`（v2.1.0、`portable-zlib`）で主サンプルの 1 枚目を実行した結果の見出し:

```
=== PNG File: <絶対パス> ===
--- Software ---
NovelAI
--- Source ---
NovelAI Diffusion V4.5 4BDE2A90
--- NovelAI [Settings] ---
{ ... Comment JSON ... }
--- Title ---
<タイトル>
--- NovelAI [Prompt] ---
<Description 本文>
```

確定した対応:

| PNG キーワード | fast_meta のラベル | 実物の内容 |
|---|---|---|
| `Description` | `NovelAI [Prompt]` | pos 全文（995 バイト） |
| `Comment` | `NovelAI [Settings]` | NovelAI の設定 JSON（5441 バイト、整形済みで返る） |
| `Software` | `Software` | `NovelAI` |
| `Source` | `Source` | `NovelAI Diffusion V4.5 ...` |
| `Title` | `Title` | 作品タイトル |

`Comment` JSON の実物キー（主サンプル）:

```
prompt, steps, height, width, scale, uncond_scale, cfg_rescale, seed, n_samples,
noise_schedule, legacy_v3_extend, reference_*, v4_prompt, v4_negative_prompt,
director_reference_*, sampler, controlnet_*, dynamic_thresholding*, sm, sm_dyn,
skip_cfg_above_sigma, skip_cfg_below_sigma, lora_*, deliberate_euler_ancestral_bug,
prefer_brownian, cfg_sched_eligibility, explike_fine_detail, minimize_sigma_inf,
uncond_per_vibe, wonky_vibe_correlation, stream, version, uc, request_type, signed_hash
```

- `Description` と `Comment.prompt` は同一本文（995 バイト）だった。NovelAI の pos は
  `Description` 優先、無ければ `Comment.prompt`（REQ-6.1）で問題ない。
- `Comment.uc` は 633 バイトで空ではない。pos 同一・neg 差分のペアは実際に起こり得る。
- `Comment` は fast_meta が **JSON を整形して返す**。抽出側は JSON として再パースする。
- `Description` 単独の画像は主サンプルには無かった。大規模サンプルでは「レコードはあるが
  `prompt`/`uc` を持たない」ページが 4 枚あり、`source = unknown` の実例になる。

`fast_meta` は `scan_file`（レコード）と `extract_to_writer`（CLI と同一の整形出力）の
両方を公開している。txt は後者をそのまま使うため、CLI との出力同一性は関数の同一性で担保される
（渡すパスは常に絶対パス。REQ-5.2）。

---

## 4. run（segment）の実測

計測ツール: `examples/phase0_survey.rs`（fast_meta ライブラリ、`ScanOptions::default()`、
REQ-3.7 の自然順、pos+neg の完全一致で run を数える）。

### 4.1 主サンプル（80 ページ）

```
pages: 80
sources: novelai=80 ... none=0 error=0
segments: 77 (distinct pos+neg: 61)
run_length distribution: [(1, 74), (2, 3)]
longest run: 2
metadata-less pages: 0
pages that break runs (no pos): 0
```

- 80 枚中 61 種の pos+neg。同一本文が離れた位置で再登場する例があり（例: seq 0 と seq 6）、
  **行は再利用・配置は別**（REQ-7.1）の実例になる。
- 連続 run は最大 2。差分を連続させない構成の CG 集では run が短い。

### 4.2 大規模サンプル（3946 ページ）

```
sources: novelai=3821 comfyui=0 a1111=0 swarmui=0 invokeai=0 unknown=4 none=121 error=0
segments: 2443 (distinct pos+neg: 2156)
run_length distribution:
  [(1,1645), (2,475), (3,192), (4,70), (5,30), (6,16), (7,8), (8,3), (9,1), (11,2), (13,1)]
longest run: 13
metadata-less pages: 121 seq=[0, 91, 134, 148, 196, 277, 352, 431, 549, 694, 713, 732, 788, 818, 829, ...]
novelai/other records but no pos: 4 seq=[428, 851, 1820, 1886]
pages that break runs (no pos): 125
```

- run_length は 1〜13 に広く分布する。**repeat = run_length を直接セットする**設計
  （REQ-7.7）は、この実データで意味を持つ。
- メタデータなしページ 121 枚が run を分断する（REQ-7.6）。欠落は先頭だけでなく全体に散る。
- 「レコードはあるが pos が取れない」ページが 4 枚あり、`source` の記録規則
  （判定できなければ `unknown`、`prompt_hash` は NULL）の実例になる。

---

## 5. 処理速度

計測ツール: `examples/phase0_bench.rs`。`extract_to_writer` で txt 本文を生成する経路を
プロセス内で回す。ウォームアップ 1 回のあと 3 回計測し、中央値を採用。

### 5.1 実画像 80 枚（117 MiB）

```
warmup: 0.018s
run 1: 0.017s
run 2: 0.016s
run 3: 0.016s
median: 0.016s (4888.5 img/s, 7165.0 MiB/s)
extrapolation: 10000 images = 2.0s, 30000 images = 6.1s
```

### 5.2 実画像 2000 枚（3,073,788,050 バイト）を C: にコピーして計測

```
warmup: 0.429s
run 1: 0.406s
run 2: 1.320s   ← 背景 I/O の影響とみられる外れ値
run 3: 0.391s
median: 0.406s (4924.6 img/s, 7218.0 MiB/s)
```

### 5.3 合成メタデータ画像 2000 枚（8.28 MiB、1 枚あたり数 KiB）

```
median: 0.058s (34249.7 img/s)
```

### 5.4 CLI 子プロセス方式との比較（主サンプル 80 枚）

```
80 回の fast_meta.exe 起動: 3,368 ms（42 ms/枚）
ライブラリ内処理:              16 ms（0.2 ms/枚）
```

約 210 倍差。REQ-2.3（画像1枚ごとの子プロセス起動をしない）の根拠になる。

### 5.5 外挿の考え方

- ウォームキャッシュでは 3GB を 0.4 秒で読めており、これは RAM 速度であって
  ストレージ速度ではない。**コールドキャッシュではストレージ I/O が支配する**。
- 10,000 枚 × 1.5 MiB = 約 15 GB。NVMe 2 GB/s で約 8 秒、HDD 150 MB/s で約 100 秒。
  どちらも「数千〜数万枚を現実的な時間で処理できる」範囲であり、Phase 0 の目的を満たす。
- 1 枚あたりの解析コスト（合成 2000 枚で 34,000 枚/秒）は無視できる。ボトルネックは
  ファイル読み出しと（Phase 1 以降の）txt 書き込み・DB 書き込みになる。

---

## 6. InvokeAI 旧 `sd-metadata` 形式のキー構成（確定）

### 6.1 実物の確認方法

- InvokeAI v2.1.2 の実ファイルから利用者が `sd-metadata.py` で出力した JSON
  （InvokeAI issue #1129 のコメント）。`sd-metadata` チャンクの本文そのもの。
- InvokeAI v2.3.5 の生成側実装
  （`invokeai/backend/invoke_ai_web_server.py` の `parameters_to_generated_image_metadata`）と
  `ldm/invoke/pngwriter.py`（`info.add_text('sd-metadata', json.dumps(metadata))`）。

### 6.2 確定した構造

PNG の `tEXt` キーワード `sd-metadata` の本文は、次の形の JSON オブジェクトである
（外側に `{"sd-metadata": ...}` の入れ子は無い）。

```json
{
  "model": "stable diffusion",
  "model_id": "stable-diffusion-1.4",
  "model_hash": "<hex>",
  "app_id": "invoke-ai/InvokeAI",
  "app_version": "v2.1.2",
  "image": {
    "type": "txt2img",
    "prompt": [
      { "prompt": "an astronaut riding a horse", "weight": 1.0 }
    ],
    "seed": 1034856165,
    "steps": 50,
    "cfg_scale": 9,
    "sampler": "k_lms",
    "width": 512,
    "height": 512,
    "variations": [],
    "postprocessing": null,
    "facetool": "gfpgan",
    "facetool_strength": 0.0,
    "threshold": 0.0,
    "perlin": 0.0,
    "init_mask": null,
    "upscale": null
  }
}
```

- v2.3.5 の Web UI 経由では `image.prompt` は**文字列**（`parameters["prompt"]`）。
  v2.1.2 の CLI 経由では**配列** `[{prompt, weight}, ...]`。両方が実在する。
- 旧形式に**負のプロンプトのキーは無い**（v2.1.2 の実物にも、v2.3.5 の生成コードにも無い）。
  したがって neg は空文字列になる（REQ-6.3）。
- 旧 `Dream` キー（コマンドライン文字列）は §1.2 のとおり対象外。

### 6.3 仕様書 §6.1 の表への追記（確定分）

| 生成元 (source) | 格納場所 | pos | neg |
|---|---|---|---|
| invokeai（旧） | PNG `sd-metadata`（JSON） | `image.prompt`（文字列はそのまま／配列は `(<prompt>:<weight>)` を `, ` 連結） | なし（＝空文字列） |

### 6.4 復元規則（D0-1: C で確定）

`image.prompt` が配列 `[{prompt, weight}, ...]` のときは、weight を捨てずに次の形で復元する。

- 各要素を出現順に `, ` で連結する。
- 各要素は `(<prompt>:<weight>)` とする。例:
  `[{"prompt":"masterpiece, best quality","weight":1.0},{"prompt":"1girl, cat ears","weight":0.8}]`
  → `(masterpiece, best quality:1.0), (1girl, cat ears:0.8)`
- `weight` が無い／数値でない要素は `prompt` のみ。`prompt` が空の要素は飛ばす。
- 連結結果が空なら未抽出。`neg` は空文字列（旧形式に負のプロンプトが無い）。
- `weight` は JSON の数値を serde_json の標準表記で書く（`1.0` は `1.0`、`0.80` は `0.8`）。
- `image.prompt` が文字列でも配列でもない場合は未抽出（推測しない）。

この規則は仕様書 §6.1 に追記済み。Phase 3 でこのとおり実装する。

---

## 7. SwarmUI EXIF Model 形式の確認結果

- 現行 SwarmUI の公式ドキュメント（`docs/Image Metadata Format.md`）では、PNG は
  `parameters` tEXt（中身 `{"sui_image_params": {...}}`）、JPEG は EXIF `UserComment` を使う。
- 仕様書 REQ-6.7 が言う「EXIF Model（タグ 0x0110）に `sui_image_params` を含む JSON」の
  **実物ファイルはローカル（`C:`/`D:`/`E:` の AI 画像コレクション）に見つからなかった**。
- 仕様書は「未確認の間も候補として実装してよい。実物が存在しなければ、この経路は単に
  発火しない」と明記しているため、Phase 3 で候補として実装する。フィクスチャは
  公式ドキュメントの実例 JSON を EXIF Model に埋めて用意した（`tests/assets/swarmui/`）。
- 実物サンプルが入手できた場合は、Phase 3 で本節に追記する。

---

## 8. フィクスチャ一覧と §11 対応

### 8.1 画像フィクスチャ（`tests/common/mod.rs`）

実物サンプルから採取したメタデータ本文だけを `tests/assets/` に置き、コンテナは
テスト時にバイト列として生成する（大きな画像をコミットしない）。

| 生成関数 | 内容 | 主な検証対象 |
|---|---|---|
| `novelai_png()` | NovelAI 正常系（Description+Comment+Software+Source） | REQ-5.2, §6.1, §11-2 |
| `novelai_png_with(pos,neg)` | 同一 pos・異なる neg のペア生成 | REQ-4.1, REQ-7.3, §11-4 |
| `novelai_pos_only_png(pos)` | Comment に uc が無い | REQ-6.3, §11-4 |
| `novelai_comment_only_png(pos,neg)` | Description 無し・Comment から pos | REQ-6.1 代替規則 |
| `novelai_comment_without_prompt_png()` | Comment は JSON だが prompt/uc 無し | 判定不成立・`unknown` |
| `description_only_png(text)` | Description 単独 | `source=unknown` の pos 採用 |
| `a1111_png(parameters)` | PNG `parameters` チャンク | Phase 3 / REQ-6.2 |
| `a1111_jpeg(parameters)` | JPEG EXIF UserComment | REQ-6.7 |
| `a1111_jpeg_comment(parameters)` | JPEG COM | REQ-6.7 |
| `comfyui_png(graph)` | 実物 `prompt` グラフ | Phase 4 / REQ-6.4 |
| `comfyui_jpeg(graph)` | UserComment ラッパ `{prompt, workflow}` | REQ-6.4, REQ-6.7 |
| `swarmui_png(params)` | SwarmUI 実形式（`parameters` に `sui_image_params`） | Phase 3 |
| `swarmui_jpeg_user_comment(params)` | UserComment 入れ子形式 | REQ-6.7 |
| `swarmui_jpeg_model(params)` | EXIF Model 形式（実物未確認） | REQ-6.7 |
| `invokeai_png()` | `invokeai_metadata`（実物チャンク） | Phase 3 |
| `invokeai_sd_metadata_png()` | 旧 `sd-metadata`（実物 JSON） | Phase 3 |
| `png_exif_user_comment(json)` | PNG eXIf 由来 UserComment | REQ-6.7 |
| `png_trailing_parameters(parameters)` | IEND 後付加データ | REQ-6.7 |
| `xmp_only_png()` | XMP のみ（振り分け対象外） | REQ-6.7 |
| `stealth_only_png()` | Stealth のみ | §1.2 対象外・`unknown` |
| `no_metadata_png()` / `no_metadata_jpeg()` | メタデータなし | REQ-5.4, REQ-7.5 |
| `broken_image()` / `empty_file()` | 壊れた画像・0 バイト | REQ-9.4, `source=error` |
| `webp_with_exif_user_comment(text)` | WebP EXIF | REQ-6.7 |

実物メタデータの採取元は `tests/assets/provenance.md` に記録した。

### 8.2 フォルダ構成フィクスチャ（`tests/common/folders.rs`）

| ヘルパー | 用途 |
|---|---|
| `fastmeta_text(image)` | CLI と同一経路の txt 本文生成（REQ-5.2） |
| `body_without_header(text)` | 見出し行を除いた比較対象（REQ-5.3） |
| `own_sidecar_body(image)` | 自ツール製 txt の内容 |
| `FOREIGN_TXT` | 他者 txt の内容 |
| `create_junction` / `junction_supported` | ジャンクション経由・循環・表記ゆれ（REQ-9.1, §11-11） |

テスト側で `Sandbox` の `dir` / `write` を使って次の構成を組む（Phase 1 以降で使用）:

- リーフ（直下に画像）、コンテナ（直下に画像なし・サブフォルダあり）、空フォルダ
- 既存 txt（自ツール製＝`own_sidecar_body` と一致／他者＝`FOREIGN_TXT`）、`_metadata` 既存
- 同 stem・異拡張子（`001.png` と `001.jpg`）
- サブフォルダに同名ファイル、`--recursive` の混成
- パス表記ゆれ（大小文字・末尾スラッシュ）、ジャンクション経由、親へ戻る循環
- 役割不一致: (a) コンテナとして取り込んだフォルダへ後から直下画像を追加、
  (b) リーフとして取り込んだフォルダから画像を消してサブフォルダだけにする
  （いずれも Phase 2 の DB 状態を作った後にフォルダを変更して再現する）

### 8.3 既存 DB フィクスチャ（`tests/common/db.rs`）

| ビルダー | 内容 | 対応 |
|---|---|---|
| `write_legacy_db(path, rows, HashColumn::Absent)` | §4.1 のみ。`prompt_hash` 列が無い | REQ-4.3 の列追加＋バックフィル |
| `write_legacy_db(path, rows, HashColumn::Null)` | `prompt_hash` 列あり・全行 NULL | バックフィル |
| 同一 (prompt, negative) を 2 行入れる | 重複行 | 警告・続行・最小 id 採用（§11-10） |

### 8.4 §11 受け入れ基準との対応（テスト計画）

| §11 | 使用するフィクスチャ | 実装 Phase |
|---|---|---|
| 1 複数フォルダ・失敗継続・重複引数・空フォルダ | `Sandbox` の複数リーフ／コンテナ／空、`broken_image` | 1 |
| 2 txt 同一性・索引なし・衝突順 | `novelai_png`, `own_sidecar_body`, `FOREIGN_TXT`, 同 stem 異拡張子 | 1 |
| 3 ギャラリー→ページ→segment→配置→story | 画像フィクスチャ一式＋確認用 DB | 2 |
| 4 pos 同・neg 異、連続 run、pos のみ | `novelai_png_with`, `novelai_pos_only_png` | 2 |
| 5 メタデータなしの分断 | `no_metadata_*`, `broken_image` | 2 |
| 6 再取り込みで増えない | 同じサンドボックスで 2 回実行 | 2 |
| 7 seed 差では分かれない | `novelai_png_with` の seed 違い | 2 |
| 8 ログが残る | 全実行 | 1 |
| 9 画像が改変されない | 実行前後の SHA-256 比較 | 2 |
| 10 バックフィル・重複警告・ロールバック | `write_legacy_db` 2 種 | 2 |
| 11 大小文字・末尾スラッシュ・ジャンクション | `create_junction`, パス表記ゆれ | 2 |

---

## 9. 実装上の観測メモ

1. **fast_meta の `Comment` は整形済み JSON**。txt もその整形結果になる（CLI と同一）。
   抽出は文字列ではなく JSON として読む。
2. **Stealth のビット長は「ビット数」**（`payload.len() * 8`）で格納される。フィクスチャ生成で
   この点を合わせた。読み出し順は列優先（x ごとに y）である。
3. **Trailing の `parameters` レコード**は、本文が `{` で始まる JSON なら全体が 1 レコードに
   なるが、キーワード探索経路では最初の改行までが本文になる。Phase 3 では fast_meta が
   返した本文に REQ-6.2 を適用する（本文が途中で切れる場合は、切れた範囲で判定する）。
4. **`source = error` の判定**: fast_meta が `Err` を返した場合（0 バイト等）、または
   `ScanOutcome.format = None`（拡張子は画像だが内容が画像でない）場合。REQ-3.8 と REQ-9.4 に対応。
5. **XMP は PNG では iTXt** で書く必要がある（tEXt の `XML:com.adobe.xmp` は fast_meta が
   XMP として解釈しない）。フィクスチャは iTXt で生成している。
6. **`sui_image_params` の実 PNG は `parameters` チャンク**であり、キーワード自体が
   `sui_image_params` ではない。Phase 3 では `parameters` レコードの中身が JSON で
   `sui_image_params` を持つ場合も SwarmUI 候補として振り分ける必要がある。

---

## 10. 判断点の決着

| # | 内容 | 決定 |
|---|---|---|
| D0-1 | InvokeAI 旧 `sd-metadata` で `image.prompt` が配列のときの pos 復元規則 | **C**: `(<prompt>:<weight>)` を `, ` で連結し、weight を保持する（§6.4） |
| D0-2 | SwarmUI EXIF Model 形式の実物サンプルが無い件 | **A**: 仕様どおり候補として実装し、実物が無ければ発火しない |

D0-1 は weight を情報として残す方針。D0-2 は仕様書 REQ-6.7 が許容している。
いずれも Phase 3 の実装で反映する。

---

## 11. Phase 0 完了条件のチェック

- [x] 実サンプル（NovelAI CG 集、80 ページ）で抽出結果・run 分布・速度を実測し、本書に記録した
- [x] 数千枚規模への外挿が現実的であることを数値で示した（§5）
- [x] InvokeAI 旧 `sd-metadata` の実物キー構成を確認し、§6 に追記した（配列の復元規則は C で確定）
- [x] 画像フィクスチャ一式を確定し、17 テストで fast_meta から期待どおり読めることを検証した
- [x] フォルダ構成フィクスチャ用のヘルパーを確定した（ジャンクション含む）
- [x] 既存 DB フィクスチャ（hash 列なし／NULL／重複）を確定した
- [x] 判断点を本書 §10 に記録し、D0-1／D0-2 の決定を反映した

再現コマンド:

```powershell
cargo test
cargo run --release --example phase0_survey -- "E:\pixiv_ai\[POKI (118145342)] 250930 Kanna Kamui(80p) (AI Generated)"
cargo run --release --example phase0_bench  -- "E:\pixiv_ai\[POKI (118145342)] 250930 Kanna Kamui(80p) (AI Generated)"
cargo run --release --example phase0_bench  -- "C:\github\sdGalleryImporter\target\phase0-bulk-real" --bulk-copy "E:\pixiv_ai\[POKI (118145342)] 250930 Kanna Kamui(80p) (AI Generated)" 2000
```

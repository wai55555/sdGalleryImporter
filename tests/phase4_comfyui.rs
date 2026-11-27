//! Phase 4: ComfyUI の辿りと、辿れない場合の未抽出扱い。

mod common;

#[path = "common/harness.rs"]
mod harness;

use common::*;
use harness::{path_string, TestEnv};
use rusqlite::Connection;
use sd_gallery_importer::hash;
use std::fs;
use std::path::Path;

const A1111_SIMPLE: &str = "a1111 pos\nNegative prompt: a1111 neg\nSteps: 20, Sampler: Euler";

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    fs::create_dir_all(dir).expect("ディレクトリを作成できる");
    fs::write(dir.join(name), bytes).expect("ファイルを書き込める");
}

fn open(path: &Path) -> Connection {
    Connection::open(path).expect("DB を開ける")
}

fn page(
    connection: &Connection,
    file_name: &str,
) -> (
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
) {
    connection
        .query_row(
            "SELECT source, prompt_hash, segment_id, width, height FROM pages WHERE file_name = ?1",
            [file_name],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap()
}

#[test]
fn comfyui_traversal_and_fallbacks() {
    let env = TestEnv::new("p4-comfyui");
    let gallery = env.work("work/g1");
    write(&gallery, "001.png", &comfyui_png(COMFYUI_PROMPT));
    write(&gallery, "002.jpg", &comfyui_jpeg(COMFYUI_PROMPT));
    write(
        &gallery,
        "003.png",
        &comfyui_png(COMFYUI_PROMPT_MULTI_SAMPLER),
    );
    write(&gallery, "004.png", &comfyui_png(COMFYUI_PROMPT));

    // ComfyUI が辿れる場合は A1111 より優先
    let comfy_and_a1111 = png(&PngSpec::default()
        .size(832, 1216)
        .text("prompt", COMFYUI_PROMPT)
        .text("parameters", A1111_SIMPLE));
    write(&gallery, "005.png", &comfy_and_a1111);

    // ComfyUI が辿れない場合は A1111 が拾う
    let comfy_broken_and_a1111 = png(&PngSpec::default()
        .size(832, 1216)
        .text("prompt", COMFYUI_PROMPT_MULTI_SAMPLER)
        .text("parameters", A1111_SIMPLE));
    write(&gallery, "006.png", &comfy_broken_and_a1111);

    // 対象外の SamplerCustom だけ → 生成元は comfyui、pos は未抽出
    let custom_sampler = r#"{
      "3": {"class_type":"SamplerCustom","inputs":{"positive":["7",0],"negative":["6",0]}},
      "6": {"class_type":"CLIPTextEncode","inputs":{"text":"neg"}},
      "7": {"class_type":"CLIPTextEncode","inputs":{"text":"pos"}}
    }"#;
    write(&gallery, "007.png", &comfyui_png(custom_sampler));

    let result = env.run_strs(&[&path_string(&gallery)]);
    assert_eq!(
        result.code, 0,
        "stderr: {}\nlog:\n{}",
        result.stderr, result.log
    );
    assert!(
        result.log.contains("reason=pos 未抽出"),
        "辿れないページは未抽出として記録"
    );
    let connection = open(&env.exe_dir.join("story.db"));

    let (source, hash_value, segment, width, height) = page(&connection, "001.png");
    assert_eq!(source, "comfyui");
    assert!(hash_value.is_some());
    assert!(segment.is_some());
    assert_eq!((width, height), (Some(832), Some(1216)));

    let (source, wrapper_hash, wrapper_segment, _, _) = page(&connection, "002.jpg");
    assert_eq!(source, "comfyui");
    assert_eq!(wrapper_hash, hash_value, "JPEG ラッパ経由でも同じ pos/neg");
    assert_eq!(wrapper_segment, segment);
    assert_eq!(
        (
            page(&connection, "002.jpg").3,
            page(&connection, "002.jpg").4
        ),
        (Some(832), Some(1216))
    );

    let (source, broken_hash, broken_segment, _, _) = page(&connection, "003.png");
    assert_eq!(source, "comfyui");
    assert!(broken_hash.is_none(), "サンプラーが複数なら未抽出");
    assert!(broken_segment.is_none());

    let (source, later_hash, later_segment, _, _) = page(&connection, "004.png");
    assert_eq!(source, "comfyui");
    assert_eq!(later_hash, hash_value);
    assert!(later_segment.is_some());

    let (source, priority_hash, _, _, _) = page(&connection, "005.png");
    assert_eq!(source, "comfyui", "辿れる ComfyUI は A1111 より優先");
    assert_eq!(priority_hash, hash_value);

    let (source, a1111_hash, _, _, _) = page(&connection, "006.png");
    assert_eq!(source, "a1111", "ComfyUI が辿れなければ A1111 が拾う");
    assert_eq!(
        a1111_hash.as_deref(),
        Some(hash::pair_hash("a1111 pos", "a1111 neg").as_str())
    );

    let (source, custom_hash, custom_segment, _, _) = page(&connection, "007.png");
    assert_eq!(source, "comfyui");
    assert!(custom_hash.is_none());
    assert!(custom_segment.is_none());

    let segments: Vec<(i64, i64, i64, String, String)> = connection
        .prepare("SELECT seq, run_length, line_id, prompt_full, negative_full FROM segments ORDER BY seq")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].1, 2, "001+002 が同一 run");
    assert_eq!(segments[1].1, 2, "004+005 が同一 run");
    assert_eq!(segments[0].2, segments[1].2, "004/005 は行を再利用");
    assert!(segments[0].3.starts_with("masterpiece, best quality"));
    assert!(segments[0].3.contains("2girls in a haunted house"));
    assert_eq!(
        segments[0].4,
        "worst quality, low quality, score_1, score_2, score_3, blurry, jpeg artifacts, sepia"
    );
    assert_eq!(segments[2].3, "a1111 pos");
    assert_eq!(segments[2].4, "a1111 neg");
}

//! Phase 3: A1111 / SwarmUI / InvokeAI の抽出と形式をまたぐ segment 同一判定。

mod common;

#[path = "common/harness.rs"]
mod harness;

use common::*;
use harness::{path_string, TestEnv};
use rusqlite::Connection;
use sd_gallery_importer::hash;
use std::fs;
use std::path::Path;

const A1111_SIMPLE: &str =
    "a1111 pos, second tag\nNegative prompt: a1111 neg, second neg\nSteps: 20, Sampler: Euler";

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    fs::create_dir_all(dir).expect("ディレクトリを作成できる");
    fs::write(dir.join(name), bytes).expect("ファイルを書き込める");
}

fn open(path: &Path) -> Connection {
    Connection::open(path).expect("DB を開ける")
}

#[derive(Debug)]
struct PageRow {
    source: String,
    prompt_hash: Option<String>,
    segment_id: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
}

fn page(connection: &Connection, file_name: &str) -> PageRow {
    connection
        .query_row(
            "SELECT source, prompt_hash, segment_id, width, height FROM pages WHERE file_name = ?1",
            [file_name],
            |row| {
                Ok(PageRow {
                    source: row.get(0)?,
                    prompt_hash: row.get(1)?,
                    segment_id: row.get(2)?,
                    width: row.get(3)?,
                    height: row.get(4)?,
                })
            },
        )
        .unwrap()
}

fn segment(connection: &Connection, seq: i64) -> (i64, i64, String, String) {
    connection
        .query_row(
            "SELECT run_length, line_id, prompt_full, negative_full FROM segments WHERE seq = ?1",
            [seq],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

#[test]
fn extracts_a1111_swarmui_and_invokeai_and_merges_across_formats() {
    let env = TestEnv::new("p3-formats");
    let gallery = env.work("work/g1");
    write(&gallery, "001.png", &a1111_png(A1111_SIMPLE));
    write(&gallery, "002.jpg", &a1111_jpeg(A1111_SIMPLE));
    write(&gallery, "003.jpg", &a1111_jpeg_comment(A1111_SIMPLE));
    write(&gallery, "004.png", &png_trailing_parameters(A1111_SIMPLE));
    write(
        &gallery,
        "005.png",
        &png_exif_image_description(A1111_SIMPLE),
    );
    write(
        &gallery,
        "006.png",
        &swarmui_png_with("swarm pos", "swarm neg"),
    );
    write(
        &gallery,
        "007.jpg",
        &swarmui_jpeg_user_comment_with("swarm pos", "swarm neg"),
    );
    write(
        &gallery,
        "008.jpg",
        &swarmui_jpeg_model_with("swarm pos", "swarm neg"),
    );
    write(
        &gallery,
        "009.png",
        &invokeai_png_with("invoke pos", "invoke neg"),
    );
    write(&gallery, "010.png", &invokeai_sd_metadata_png());
    write(&gallery, "011.png", &description_only_png("unknown pos"));
    write(&gallery, "012.png", &xmp_only_png());
    write(
        &gallery,
        "013.png",
        &a1111_png("Negative prompt: only neg\nSteps: 20"),
    );
    write(
        &gallery,
        "014.webp",
        &webp_with_exif_user_comment(A1111_SIMPLE),
    );

    let result = env.run_strs(&[&path_string(&gallery)]);
    assert_eq!(
        result.code, 0,
        "stderr: {}\nlog:\n{}",
        result.stderr, result.log
    );
    let connection = open(&env.exe_dir.join("story.db"));

    for name in [
        "001.png", "002.jpg", "003.jpg", "004.png", "005.png", "014.webp",
    ] {
        let page = page(&connection, name);
        assert_eq!(page.source, "a1111", "{name}");
        assert_eq!(
            page.prompt_hash.as_deref(),
            Some(hash::pair_hash("a1111 pos, second tag", "a1111 neg, second neg").as_str()),
            "{name}"
        );
    }
    for name in ["006.png", "007.jpg", "008.jpg"] {
        let page = page(&connection, name);
        assert_eq!(page.source, "swarmui", "{name}");
        assert_eq!(
            page.prompt_hash.as_deref(),
            Some(hash::pair_hash("swarm pos", "swarm neg").as_str()),
            "{name}"
        );
    }
    let modern = page(&connection, "009.png");
    assert_eq!(modern.source, "invokeai");
    assert_eq!(
        modern.prompt_hash.as_deref(),
        Some(hash::pair_hash("invoke pos", "invoke neg").as_str())
    );
    let legacy = page(&connection, "010.png");
    assert_eq!(legacy.source, "invokeai");
    assert_eq!(
        legacy.prompt_hash.as_deref(),
        Some(hash::pair_hash("(an astronaut riding a horse:1.0)", "").as_str())
    );
    let unknown = page(&connection, "011.png");
    assert_eq!(unknown.source, "unknown");
    assert_eq!(
        unknown.prompt_hash.as_deref(),
        Some(hash::pair_hash("unknown pos", "").as_str())
    );
    let xmp = page(&connection, "012.png");
    assert_eq!(xmp.source, "unknown");
    assert!(xmp.prompt_hash.is_none());
    assert!(xmp.segment_id.is_none());
    let no_pos = page(&connection, "013.png");
    assert_eq!(no_pos.source, "a1111");
    assert!(no_pos.prompt_hash.is_none());

    // 寸法は形式ごとのヘッダから取る
    assert_eq!(
        (
            page(&connection, "001.png").width,
            page(&connection, "001.png").height
        ),
        (Some(512), Some(768))
    );
    assert_eq!(
        (
            page(&connection, "002.jpg").width,
            page(&connection, "002.jpg").height
        ),
        (Some(512), Some(768))
    );
    assert_eq!(
        (
            page(&connection, "006.png").width,
            page(&connection, "006.png").height
        ),
        (Some(1024), Some(1024))
    );
    assert_eq!(
        (
            page(&connection, "009.png").width,
            page(&connection, "009.png").height
        ),
        (Some(1024), Some(768))
    );
    assert_eq!(
        (
            page(&connection, "010.png").width,
            page(&connection, "010.png").height
        ),
        (Some(512), Some(512))
    );
    assert_eq!(
        (
            page(&connection, "014.webp").width,
            page(&connection, "014.webp").height
        ),
        (Some(512), Some(512))
    );

    // A1111 の 5 形式は連続する同一 pos+neg なので 1 segment（run_length=5）
    let (a1111_run, a1111_line, a1111_pos, a1111_neg) = segment(&connection, 0);
    assert_eq!(a1111_run, 5, "形式が違っても文字列が同じなら同じ run");
    assert_eq!(a1111_pos, "a1111 pos, second tag");
    assert_eq!(a1111_neg, "a1111 neg, second neg");
    let (swarm_run, swarm_line, swarm_pos, swarm_neg) = segment(&connection, 1);
    assert_eq!(swarm_run, 3);
    assert_eq!(
        (swarm_pos.as_str(), swarm_neg.as_str()),
        ("swarm pos", "swarm neg")
    );
    assert_ne!(swarm_line, a1111_line);
    assert_eq!(segment(&connection, 2).2, "invoke pos");
    assert_eq!(
        segment(&connection, 3).2,
        "(an astronaut riding a horse:1.0)"
    );
    assert_eq!(segment(&connection, 4).2, "unknown pos");
    // 014 はギャップ後の同一ペア → 別 segment・同一行
    let (last_run, last_line, last_pos, _) = segment(&connection, 5);
    assert_eq!(last_run, 1);
    assert_eq!(last_pos, "a1111 pos, second tag");
    assert_eq!(last_line, a1111_line, "run をまたいでも行は再利用");
    assert_eq!(count(&connection, "lines"), 5);
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn priority_and_detection_without_pos() {
    let env = TestEnv::new("p3-priority");
    let gallery = env.work("work/g1");

    let novelai_comment =
        serde_json::json!({"prompt": "novelai pos", "uc": "novelai neg"}).to_string();
    let novelai_and_a1111 = png(&PngSpec::default()
        .size(64, 48)
        .text("Description", "novelai pos")
        .text("Comment", novelai_comment)
        .text("parameters", A1111_SIMPLE));
    write(&gallery, "001.png", &novelai_and_a1111);

    let a1111_and_swarm = png(&PngSpec::default()
        .size(64, 48)
        .text("parameters", A1111_SIMPLE)
        .text(
            "sui_image_params",
            r#"{"prompt":"direct swarm","negativeprompt":"direct neg"}"#,
        ));
    write(&gallery, "002.png", &a1111_and_swarm);

    let direct_swarm = png(&PngSpec::default().size(64, 48).text(
        "sui_image_params",
        r#"{"prompt":"direct swarm","negativeprompt":"direct neg"}"#,
    ));
    write(&gallery, "003.png", &direct_swarm);

    write(
        &gallery,
        "004.png",
        &invokeai_png_with("invoke only", "invoke neg"),
    );
    write(
        &gallery,
        "005.png",
        &a1111_png("Negative prompt: only neg\nSteps: 20"),
    );

    let result = env.run_strs(&[&path_string(&gallery)]);
    assert_eq!(result.code, 0, "{}", result.log);
    let connection = open(&env.exe_dir.join("story.db"));

    let novelai = page(&connection, "001.png");
    assert_eq!(novelai.source, "novelai", "NovelAI が最優先");
    assert_eq!(
        novelai.prompt_hash.as_deref(),
        Some(hash::pair_hash("novelai pos", "novelai neg").as_str())
    );

    let a1111 = page(&connection, "002.png");
    assert_eq!(a1111.source, "a1111", "A1111 は SwarmUI より優先");
    assert_eq!(
        a1111.prompt_hash.as_deref(),
        Some(hash::pair_hash("a1111 pos, second tag", "a1111 neg, second neg").as_str())
    );

    let swarm = page(&connection, "003.png");
    assert_eq!(swarm.source, "swarmui");
    assert_eq!(
        swarm.prompt_hash.as_deref(),
        Some(hash::pair_hash("direct swarm", "direct neg").as_str())
    );

    let invoke = page(&connection, "004.png");
    assert_eq!(invoke.source, "invokeai");
    assert_eq!(
        invoke.prompt_hash.as_deref(),
        Some(hash::pair_hash("invoke only", "invoke neg").as_str())
    );

    let no_pos = page(&connection, "005.png");
    assert_eq!(no_pos.source, "a1111", "pos が取れなくても生成元は記録");
    assert!(no_pos.prompt_hash.is_none());
}

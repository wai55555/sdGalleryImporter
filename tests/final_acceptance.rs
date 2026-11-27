//! 最終受け入れ: パラメータ差で分かれないこと・正規化で誤合体しないこと。

mod common;

#[path = "common/harness.rs"]
mod harness;

use common::*;
use harness::{path_string, TestEnv};
use rusqlite::Connection;
use sd_gallery_importer::hash;
use std::fs;
use std::path::Path;

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    fs::create_dir_all(dir).expect("ディレクトリを作成できる");
    fs::write(dir.join(name), bytes).expect("ファイルを書き込める");
}

fn novelai_with_seed(pos: &str, neg: &str, seed: i64) -> Vec<u8> {
    let comment = serde_json::json!({
        "prompt": pos,
        "uc": neg,
        "seed": seed,
        "steps": 28,
        "sampler": "k_euler",
    })
    .to_string();
    png(&PngSpec::default()
        .size(64, 48)
        .text("Description", pos)
        .text("Comment", comment)
        .text("Software", "NovelAI"))
}

#[test]
fn parameters_and_normalization_do_not_change_segments() {
    let env = TestEnv::new("final-params");
    let gallery = env.work("work/g1");
    write(
        &gallery,
        "001.png",
        &novelai_with_seed("same pos", "same neg", 1),
    );
    write(
        &gallery,
        "002.png",
        &novelai_with_seed("same pos", "same neg", 2),
    );
    write(
        &gallery,
        "003.png",
        &novelai_with_seed("same  pos", "same neg", 3),
    );
    write(
        &gallery,
        "004.png",
        &a1111_png(
            "same pos\nNegative prompt: same neg\nSteps: 99, Sampler: Euler, Seed: 12345, CFG scale: 7",
        ),
    );

    let before: Vec<(std::path::PathBuf, Vec<u8>)> = fs::read_dir(&gallery)
        .unwrap()
        .flatten()
        .map(|entry| {
            let path = entry.path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();

    let result = env.run_strs(&[&path_string(&gallery)]);
    assert_eq!(result.code, 0, "{}", result.log);
    let connection = Connection::open(env.exe_dir.join("story.db")).unwrap();

    let segments: Vec<(i64, i64, i64, String, String)> = connection
        .prepare("SELECT seq, run_length, line_id, prompt_full, negative_full FROM segments ORDER BY seq")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(segments.len(), 3, "seed/steps の差では segment を分けない");
    assert_eq!(
        segments.iter().map(|row| row.1).collect::<Vec<_>>(),
        vec![2, 1, 1]
    );
    assert_eq!(
        segments[0].2, segments[2].2,
        "A1111 でも同一文字列は行を再利用"
    );
    assert_ne!(
        segments[0].2, segments[1].2,
        "空白差は別の行（正規化しない）"
    );

    let lines: Vec<(String, String)> = connection
        .prepare("SELECT prompt, negative FROM lines ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], ("same pos".to_string(), "same neg".to_string()));
    assert_eq!(lines[1], ("same  pos".to_string(), "same neg".to_string()));

    let hash_same = hash::pair_hash("same pos", "same neg");
    let hash_spaced = hash::pair_hash("same  pos", "same neg");
    let page_hashes: Vec<(String, Option<String>)> = connection
        .prepare("SELECT file_name, prompt_hash FROM pages ORDER BY file_name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(page_hashes[0].1.as_deref(), Some(hash_same.as_str()));
    assert_eq!(page_hashes[1].1.as_deref(), Some(hash_same.as_str()));
    assert_eq!(page_hashes[2].1.as_deref(), Some(hash_spaced.as_str()));
    assert_eq!(page_hashes[3].1.as_deref(), Some(hash_same.as_str()));

    let (gallery_count, page_count, segment_count, unextracted): (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT gallery_count, page_count, segment_count, unextracted_count FROM import_runs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (gallery_count, page_count, segment_count, unextracted),
        (1, 4, 3, 0)
    );

    for (path, bytes) in before {
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "画像は改変されない: {path:?}"
        );
    }
}

#[test]
fn leaf_db_failure_rolls_back_and_other_galleries_continue() {
    let env = TestEnv::new("final-leaf-rollback");
    let seed = env.work("work/seed");
    write(
        &seed,
        "001.png",
        &novelai_with_seed("seed pos", "seed neg", 1),
    );
    assert_eq!(env.run_strs(&[&path_string(&seed)]).code, 0);

    let db_path = env.exe_dir.join("story.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_bad_page BEFORE INSERT ON pages
             WHEN NEW.file_name = 'bad.png'
             BEGIN SELECT RAISE(ABORT, 'intentional failure'); END;",
        )
        .unwrap();

    let bad = env.work("work/bad");
    write(&bad, "001.png", &novelai_with_seed("bad pos", "bad neg", 1));
    write(
        &bad,
        "bad.png",
        &novelai_with_seed("bad pos 2", "bad neg", 2),
    );
    let ok = env.work("work/ok");
    write(&ok, "001.png", &novelai_with_seed("ok pos", "ok neg", 1));

    let result = env.run_strs(&[&path_string(&bad), &path_string(&ok)]);
    assert_eq!(result.code, 0, "リーフの DB 失敗は全体異常にしない");
    assert!(result.log.contains("[失敗] リーフを取り込めない"));
    assert!(ok.join("001.txt").is_file(), "兄弟ギャラリーは処理される");

    let names: Vec<String> = connection
        .prepare("SELECT name FROM galleries ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(names, vec!["ok".to_string(), "seed".to_string()]);
    let bad_pages: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pages WHERE file_name = 'bad.png'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bad_pages, 0, "失敗したリーフの行は残らない");
    let (gallery_count, page_count, segment_count, unextracted): (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT gallery_count, page_count, segment_count, unextracted_count
                 FROM import_runs ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (gallery_count, page_count, segment_count, unextracted),
        (1, 1, 1, 0),
        "失敗分は集計に入れない"
    );
}

#[test]
fn lines_are_shared_across_galleries_and_txt_is_restored_on_skip() {
    let env = TestEnv::new("final-cross-gallery");
    let g1 = env.work("work/g1");
    let g2 = env.work("work/g2");
    write(
        &g1,
        "001.png",
        &novelai_with_seed("shared pos", "shared neg", 1),
    );
    write(
        &g2,
        "001.png",
        &novelai_with_seed("shared pos", "shared neg", 2),
    );

    let first = env.run_strs(&[&path_string(&g1), &path_string(&g2)]);
    assert_eq!(first.code, 0, "{}", first.log);
    let db_path = env.exe_dir.join("story.db");
    let connection = Connection::open(&db_path).unwrap();
    let scalar = |sql: &str| -> i64 { connection.query_row(sql, [], |row| row.get(0)).unwrap() };
    assert_eq!(scalar("SELECT COUNT(*) FROM galleries"), 2);
    assert_eq!(scalar("SELECT COUNT(*) FROM stories"), 2);
    assert_eq!(
        scalar("SELECT COUNT(*) FROM lines"),
        1,
        "ギャラリーをまたいで行を共有"
    );
    assert_eq!(scalar("SELECT COUNT(*) FROM segments"), 2);
    assert_eq!(scalar("SELECT COUNT(*) FROM placements"), 2);
    let same_line: i64 = connection
        .query_row("SELECT COUNT(DISTINCT line_id) FROM segments", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(same_line, 1);

    fs::remove_file(g1.join("001.txt")).unwrap();
    let second = env.run_strs(&[&path_string(&g1)]);
    assert_eq!(second.code, 0);
    assert!(second
        .log
        .contains("取り込み済みのため DB 書き込みをスキップ"));
    assert!(
        g1.join("001.txt").is_file(),
        "取り込み済みでも欠落した txt は復活する"
    );
    assert_eq!(scalar("SELECT COUNT(*) FROM lines"), 1);
    assert_eq!(scalar("SELECT COUNT(*) FROM segments"), 2);
    let (gallery_count, page_count, segment_count): (i64, i64, i64) = connection
        .query_row(
            "SELECT gallery_count, page_count, segment_count FROM import_runs
             ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((gallery_count, page_count, segment_count), (0, 0, 0));
}

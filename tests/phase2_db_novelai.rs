//! Phase 2: DB 取り込み（NovelAI）・segment・再取り込み。

mod common;

#[path = "common/harness.rs"]
mod harness;

use common::db::{write_legacy_db, HashColumn};
use common::*;
use harness::{path_string, TestEnv};
use rusqlite::Connection;
use sd_gallery_importer::{hash, paths};
use std::fs;
use std::path::Path;

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    fs::create_dir_all(dir).expect("ディレクトリを作成できる");
    fs::write(dir.join(name), bytes).expect("ファイルを書き込める");
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn open(path: &Path) -> Connection {
    Connection::open(path).expect("DB を開ける")
}

#[derive(Debug)]
struct PageRow {
    seq: i64,
    file_name: String,
    source: String,
    prompt_hash: Option<String>,
    segment_id: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
}

fn pages(connection: &Connection) -> Vec<PageRow> {
    let mut statement = connection
        .prepare(
            "SELECT seq, file_name, source, prompt_hash, segment_id, width, height
             FROM pages ORDER BY seq",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok(PageRow {
                seq: row.get(0)?,
                file_name: row.get(1)?,
                source: row.get(2)?,
                prompt_hash: row.get(3)?,
                segment_id: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn novelai_gallery_builds_story_segments_and_placements() {
    let env = TestEnv::new("p2-import");
    let gallery = env.work("work/g1");
    write(&gallery, "001.png", &novelai_png_with("pos A", "neg X"));
    write(&gallery, "002.png", &novelai_png_with("pos A", "neg X"));
    write(&gallery, "003.png", &novelai_png_with("pos A", "neg Y"));
    write(&gallery, "004.png", &no_metadata_png());
    write(&gallery, "005.png", &novelai_png_with("pos A", "neg X"));
    write(&gallery, "006.png", &novelai_pos_only_png("pos B"));

    let result = env.run_strs(&[&path_string(&gallery)]);
    assert_eq!(
        result.code, 0,
        "stderr: {}\nlog:\n{}",
        result.stderr, result.log
    );

    let db_path = env.exe_dir.join("story.db");
    assert!(db_path.is_file(), "既定の story.db が作られる");
    let connection = open(&db_path);

    let (run_id, finished, log_path, gallery_count, page_count, segment_count, unextracted): (
        i64,
        Option<String>,
        Option<String>,
        i64,
        i64,
        i64,
        i64,
    ) = connection
        .query_row(
            "SELECT id, finished_at, log_path, gallery_count, page_count, segment_count,
             unextracted_count FROM import_runs",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .unwrap();
    let finished_value = finished.as_deref().expect("finished_at が入る");
    assert!(
        finished_value.contains('+') || finished_value.ends_with('Z'),
        "ISO 8601 のタイムゾーン付き: {finished_value}"
    );
    assert_eq!(
        log_path.as_deref(),
        result.log_path.as_ref().map(|path| path.to_str().unwrap())
    );
    assert_eq!(
        (gallery_count, page_count, segment_count, unextracted),
        (1, 6, 4, 1)
    );
    assert!(result.log.contains("story_id:"), "story_id がログに出る");
    assert!(result.log.contains("セグメント: 4"));
    assert!(result.log.contains("gallery_count: 1"));
    assert!(result.log.contains("page_count: 6"));
    assert!(result.log.contains("segment_count: 4"));
    assert!(result.log.contains("unextracted_count: 1"));

    let (gallery_id, root_path, name, story_id, import_run_id): (
        i64,
        String,
        String,
        Option<i64>,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT id, root_path, name, story_id, import_run_id FROM galleries",
            [],
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
        .unwrap();
    assert_eq!(root_path, paths::normalize_key(&gallery).unwrap());
    assert_eq!(name, "g1");
    assert_eq!(import_run_id, Some(run_id));
    let imported_at: String = connection
        .query_row("SELECT imported_at FROM galleries", [], |row| row.get(0))
        .unwrap();
    assert!(imported_at.contains('+') || imported_at.ends_with('Z'));
    let story_id = story_id.expect("リーフには story_id がある");

    let (title, quality, style, character, status, origin):
        (String, String, String, String, String, Option<i64>) = connection
        .query_row(
            "SELECT title, quality_prompt, style_prompt, character_prompt, status, origin_gallery_id
             FROM stories WHERE id = ?1",
            [story_id],
            |row| {
                Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(title, "g1");
    assert_eq!(
        (quality.as_str(), style.as_str(), character.as_str()),
        ("", "", "")
    );
    assert_eq!(status, "draft");
    assert_eq!(origin, Some(gallery_id));

    let pages = pages(&connection);
    assert_eq!(pages.len(), 6);
    assert_eq!(
        pages.iter().map(|page| page.seq).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        pages
            .iter()
            .map(|page| page.file_name.as_str())
            .collect::<Vec<_>>(),
        vec!["001.png", "002.png", "003.png", "004.png", "005.png", "006.png"]
    );
    assert_eq!(pages[0].source, "novelai");
    assert_eq!(pages[3].source, "none");
    assert!(pages[3].prompt_hash.is_none());
    assert!(pages[3].segment_id.is_none());
    assert_eq!(pages[0].width, Some(64));
    assert_eq!(pages[0].height, Some(48));
    assert_eq!(
        pages[0].prompt_hash.as_deref(),
        Some(hash::pair_hash("pos A", "neg X").as_str())
    );
    assert_eq!(pages[0].segment_id, pages[1].segment_id);
    assert_ne!(pages[0].segment_id, pages[2].segment_id);
    assert_ne!(pages[0].segment_id, pages[4].segment_id);
    assert_ne!(pages[2].segment_id, pages[4].segment_id);
    assert_ne!(pages[5].segment_id, pages[0].segment_id);
    assert_eq!(
        pages[5].prompt_hash.as_deref(),
        Some(hash::pair_hash("pos B", "").as_str()),
        "pos のみは neg=空で抽出済み"
    );

    let segments: Vec<(i64, i64, i64, i64, String, String)> = connection
        .prepare(
            "SELECT id, seq, run_length, line_id, prompt_full, negative_full
             FROM segments ORDER BY seq",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(segments.len(), 4);
    assert_eq!(
        segments.iter().map(|row| row.1).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        segments.iter().map(|row| row.2).collect::<Vec<_>>(),
        vec![2, 1, 1, 1]
    );
    assert_eq!(
        segments[0].3, segments[2].3,
        "run をまたいだ同一 pos+neg は行を再利用"
    );
    assert_ne!(segments[0].3, segments[1].3);
    assert_eq!(segments[0].4, "pos A");
    assert_eq!(segments[0].5, "neg X");
    assert_eq!(segments[2].4, "pos A");
    assert_eq!(segments[2].5, "neg X");
    assert_eq!(segments[3].4, "pos B");
    assert_eq!(segments[3].5, "");

    let lines: Vec<(String, String, Option<String>)> = connection
        .prepare("SELECT prompt, negative, prompt_hash FROM lines ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].2, Some(hash::pair_hash("pos A", "neg X")));
    assert_eq!(lines[1].2, Some(hash::pair_hash("pos A", "neg Y")));
    assert_eq!(lines[2].2, Some(hash::pair_hash("pos B", "")));

    let placements: Vec<(i64, i64, i64, Option<i64>)> = connection
        .prepare("SELECT ord, repeat, line_id, segment_id FROM placements ORDER BY ord")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(placements.len(), 4);
    assert_eq!(
        placements.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        placements.iter().map(|row| row.1).collect::<Vec<_>>(),
        vec![2, 1, 1, 1]
    );
    assert_eq!(placements[0].2, segments[0].3);
    assert_eq!(placements[2].2, segments[2].3);
    for (index, placement) in placements.iter().enumerate() {
        assert_eq!(placement.3, Some(segments[index].0));
    }
}

#[test]
fn reimport_skips_db_writes_and_reuses_db_stats() {
    let env = TestEnv::new("p2-reimport");
    let gallery = env.work("work/g1");
    write(&gallery, "001.png", &novelai_png_with("pos A", "neg X"));
    write(&gallery, "002.png", &novelai_png_with("pos A", "neg X"));
    write(&gallery, "003.png", &no_metadata_png());
    let gallery_text = path_string(&gallery);

    let first = env.run_strs(&[&gallery_text]);
    assert_eq!(first.code, 0);
    let db_path = env.exe_dir.join("story.db");
    let connection = open(&db_path);
    let counts_before = (
        count(&connection, "galleries"),
        count(&connection, "pages"),
        count(&connection, "segments"),
        count(&connection, "lines"),
        count(&connection, "stories"),
        count(&connection, "placements"),
    );
    let (gallery_run, gallery_imported): (Option<i64>, String) = connection
        .query_row(
            "SELECT import_run_id, imported_at FROM galleries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let own_txt = fs::read(gallery.join("001.txt")).unwrap();

    let second = env.run_strs(&[&gallery_text]);
    assert_eq!(second.code, 0);
    let counts_after = (
        count(&connection, "galleries"),
        count(&connection, "pages"),
        count(&connection, "segments"),
        count(&connection, "lines"),
        count(&connection, "stories"),
        count(&connection, "placements"),
    );
    assert_eq!(counts_before, counts_after, "再取り込みで行が増えない");
    assert_eq!(count(&connection, "import_runs"), 2);
    let (finished, gallery_count, page_count, segment_count, unextracted): (
        Option<String>,
        i64,
        i64,
        i64,
        i64,
    ) = connection
        .query_row(
            "SELECT finished_at, gallery_count, page_count, segment_count, unextracted_count
             FROM import_runs ORDER BY id DESC LIMIT 1",
            [],
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
        .unwrap();
    assert!(finished.is_some());
    assert_eq!(
        (gallery_count, page_count, segment_count, unextracted),
        (0, 0, 0, 0)
    );
    assert!(second
        .log
        .contains("取り込み済みのため DB 書き込みをスキップ"));
    assert!(
        second.log.contains("セグメント: 1"),
        "スキップ時は DB の segment 数を出す"
    );
    assert!(second.log.contains("メタデータなし: 1"));
    assert!(second.log.contains("reason=メタデータなし"));
    assert_eq!(
        fs::read(gallery.join("001.txt")).unwrap(),
        own_txt,
        "既定では txt を上書きしない"
    );
    let (gallery_run_after, gallery_imported_after): (Option<i64>, String) = connection
        .query_row(
            "SELECT import_run_id, imported_at FROM galleries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        gallery_run, gallery_run_after,
        "最初の取り込み実行を指し続ける"
    );
    assert_eq!(gallery_imported, gallery_imported_after);
}

#[test]
fn container_children_and_role_mismatch() {
    let env = TestEnv::new("p2-container");
    let root = env.work("work/root");
    let a = root.join("a");
    let b = root.join("b");
    write(&a, "001.png", &novelai_png_with("a pos", "a neg"));
    write(&b, "001.png", &novelai_png_with("b pos", "b neg"));
    let root_text = path_string(&root);

    let first = env.run_strs(&[&root_text]);
    assert_eq!(first.code, 0);
    let db_path = env.exe_dir.join("story.db");
    let connection = open(&db_path);
    assert_eq!(count(&connection, "galleries"), 3);
    assert_eq!(count(&connection, "stories"), 2);
    let (root_id, root_parent, root_story): (i64, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT id, parent_id, story_id FROM galleries WHERE name = 'root'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(root_parent.is_none());
    assert!(root_story.is_none(), "コンテナにストーリーは無い");
    let child_parents: Vec<Option<i64>> = connection
        .prepare("SELECT parent_id FROM galleries WHERE name IN ('a', 'b') ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(child_parents, vec![Some(root_id), Some(root_id)]);
    let first_run: i64 = connection
        .query_row(
            "SELECT id FROM import_runs ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let a_run_before: Option<i64> = connection
        .query_row(
            "SELECT import_run_id FROM galleries WHERE name = 'a'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // コンテナ配下に子を追加して再投入
    let c = root.join("c");
    write(&c, "001.png", &novelai_png_with("c pos", "c neg"));
    let second = env.run_strs(&[&root_text]);
    assert_eq!(second.code, 0);
    assert_eq!(count(&connection, "galleries"), 4, "増えた子だけ取り込む");
    assert_eq!(count(&connection, "stories"), 3);
    let second_run: i64 = connection
        .query_row(
            "SELECT id FROM import_runs ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(first_run, second_run);
    let (second_gallery_count, second_page_count): (i64, i64) = connection
        .query_row(
            "SELECT gallery_count, page_count FROM import_runs WHERE id = ?1",
            [second_run],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((second_gallery_count, second_page_count), (1, 1));
    let a_run_after: Option<i64> = connection
        .query_row(
            "SELECT import_run_id FROM galleries WHERE name = 'a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(a_run_before, a_run_after, "既存リーフの実行は変わらない");

    // 役割不一致: リーフ a から画像を消してサブフォルダだけにする
    fs::remove_file(a.join("001.png")).unwrap();
    write(
        &a.join("child"),
        "001.png",
        &novelai_png_with("child pos", "child neg"),
    );
    let third = env.run_strs(&[&root_text]);
    assert_eq!(third.code, 0);
    assert!(third.log.contains("既存のリーフを親として再利用"));
    assert_eq!(
        count(&connection, "stories"),
        4,
        "a はリーフのまま、child が増える"
    );
    let a_id: i64 = connection
        .query_row("SELECT id FROM galleries WHERE name = 'a'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let child_of_a: Option<i64> = connection
        .query_row(
            "SELECT parent_id FROM galleries WHERE name = 'child'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(child_of_a, Some(a_id));

    // 役割不一致: コンテナ d を後からリーフ化
    let d = root.join("d");
    write(
        &d.join("sub"),
        "001.png",
        &novelai_png_with("d sub", "d neg"),
    );
    assert_eq!(env.run_strs(&[&root_text]).code, 0);
    let (d_id, d_story, d_run): (i64, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT id, story_id, import_run_id FROM galleries WHERE name = 'd'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(d_story.is_none());
    write(&d, "002.png", &novelai_png_with("d leaf", "d neg"));
    let fifth = env.run_strs(&[&root_text]);
    assert_eq!(fifth.code, 0);
    let (d_story_after, d_run_after): (Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT story_id, import_run_id FROM galleries WHERE name = 'd'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(d_story_after.is_some(), "コンテナ行がリーフ化される");
    assert_ne!(d_run, d_run_after, "変換した実行に更新される");
    let d_story_id: Option<i64> = connection
        .query_row(
            "SELECT story_id FROM galleries WHERE id = ?1",
            [d_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(d_story_id, d_story_after);
}

#[test]
fn legacy_db_backfill_reuses_min_id_and_failure_rolls_back() {
    let env = TestEnv::new("p2-legacy");
    let gallery = env.work("work/g1");
    write(&gallery, "001.png", &novelai_png_with("pos A", "neg X"));
    let gallery_text = path_string(&gallery);

    let legacy = env.work("legacy.db");
    write_legacy_db(
        &legacy,
        &[("pos A", "neg X"), ("pos A", "neg X")],
        HashColumn::Absent,
    )
    .unwrap();
    let legacy_text = path_string(&legacy);
    let result = env.run_strs(&["--db", &legacy_text, &gallery_text]);
    assert_eq!(result.code, 0, "{}", result.log);
    let connection = open(&legacy);
    let expected_hash = hash::pair_hash("pos A", "neg X");
    let lines: Vec<(i64, Option<String>)> = connection
        .prepare("SELECT id, prompt_hash FROM lines ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(lines.len(), 2, "既存行を再利用し、新規行を作らない");
    assert_eq!(lines[0].1.as_deref(), Some(expected_hash.as_str()));
    assert_eq!(lines[1].1.as_deref(), Some(expected_hash.as_str()));
    assert!(result.log.contains("[警告] prompt_hash が重複"));
    let placement_line: i64 = connection
        .query_row("SELECT line_id FROM placements", [], |row| row.get(0))
        .unwrap();
    assert_eq!(placement_line, lines[0].0, "最小 id を採用する");

    // スキーマ用意の失敗はロールバックされ、変更が残らない
    let broken = env.work("broken.db");
    {
        let connection = Connection::open(&broken).unwrap();
        connection
            .execute_batch("CREATE TABLE lines (id INTEGER PRIMARY KEY);")
            .unwrap();
    }
    let broken_text = path_string(&broken);
    let failed = env.run_strs(&["--db", &broken_text, &gallery_text]);
    assert_eq!(failed.code, 1);
    assert!(failed.log.contains("スキーマを用意できない"));
    let connection = open(&broken);
    let columns: Vec<String> = connection
        .prepare("PRAGMA table_info(lines)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        !columns.contains(&"prompt_hash".to_string()),
        "列追加が残らない"
    );
    let tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='import_runs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0, "スキーマ作成がロールバックされる");
}

#[test]
fn dry_run_predicts_without_touching_db() {
    let env = TestEnv::new("p2-dry-run");
    let g1 = env.work("work/g1");
    write(&g1, "001.png", &novelai_png_with("pos A", "neg X"));
    write(&g1, "002.png", &novelai_png_with("pos A", "neg X"));

    let g1_text = path_string(&g1);
    let dry = env.run_strs(&["--dry-run", &g1_text]);
    assert_eq!(dry.code, 0);
    assert!(
        !env.exe_dir.join("story.db").exists(),
        "dry-run は DB を作らない"
    );
    assert!(!g1.join("001.txt").exists());
    assert!(dry.log.contains("セグメント（予測）: 1"));
    assert!(dry.log.contains("集計元: 予測"));
    assert!(!dry.log.contains("story_id:"));

    let normal = env.run_strs(&[&g1_text]);
    assert_eq!(normal.code, 0);
    let db_path = env.exe_dir.join("story.db");
    let connection = open(&db_path);
    let runs_before = count(&connection, "import_runs");
    let galleries_before = count(&connection, "galleries");

    let g2 = env.work("work/g2");
    write(&g2, "001.png", &novelai_png_with("pos B", "neg Z"));
    write(&g2, "002.png", &no_metadata_png());
    let dry2 = env.run_strs(&["--dry-run", &g1_text, &path_string(&g2)]);
    assert_eq!(dry2.code, 0);
    assert_eq!(
        count(&connection, "import_runs"),
        runs_before,
        "import_runs を増やさない"
    );
    assert_eq!(
        count(&connection, "galleries"),
        galleries_before,
        "galleries を増やさない"
    );
    assert!(dry2
        .log
        .contains("取り込み済みのため DB 書き込みをスキップ"));
    assert!(dry2.log.contains("セグメント（予測）: 1"));
    assert!(dry2.log.contains("集計元: 予測"));
    assert!(
        dry2.log.contains("gallery_count: 1"),
        "未取り込みの g2 だけを予測"
    );

    let legacy = env.work("legacy.db");
    write_legacy_db(&legacy, &[("old pos", "old neg")], HashColumn::Absent).unwrap();
    let legacy_text = path_string(&legacy);
    let dry3 = env.run_strs(&["--dry-run", "--db", &legacy_text, &g1_text]);
    assert_eq!(dry3.code, 0);
    let connection = open(&legacy);
    let tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='galleries'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0, "未初期化 DB にスキーマを作らない");
    assert!(dry3
        .log
        .contains("追加テーブルが無いため全フォルダを未取り込み"));
}

#[test]
fn db_open_failure_is_global() {
    let env = TestEnv::new("p2-db-fail");
    let gallery = env.work("work/g1");
    write(&gallery, "001.png", &novelai_png_with("pos A", "neg X"));
    let directory_db = env.work("db-dir");
    fs::create_dir_all(&directory_db).unwrap();
    let result = env.run_strs(&["--db", &path_string(&directory_db), &path_string(&gallery)]);
    assert_eq!(result.code, 1);
    assert!(result.log.contains("DB を開けない"));
}

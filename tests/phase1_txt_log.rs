//! Phase 1: txt サイドカー・ログ・D&D 相当の複数フォルダ起動（DB なし）。

mod common;

#[path = "common/harness.rs"]
mod harness;

use common::folders::{
    body_without_header, create_junction, fastmeta_text, junction_supported, FOREIGN_TXT,
};
use common::*;
use harness::{files_recursive, log_files, path_string, TestEnv};
use std::fs;
use std::path::{Path, PathBuf};

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    fs::create_dir_all(dir).expect("ディレクトリを作成できる");
    fs::write(dir.join(name), bytes).expect("ファイルを書き込める");
}

fn gallery_section<'a>(log: &'a str, path: &Path) -> &'a str {
    let marker = format!("=== ギャラリー: {} ===", path_string(path));
    let Some(start) = log.find(&marker) else {
        return "";
    };
    let rest = &log[start + marker.len()..];
    match rest.find("\n=== ") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

fn page_status(section: &str, file_name: &str) -> Option<String> {
    let needle = format!("file={file_name} status=");
    let line = section.lines().find(|line| line.contains(&needle))?;
    let value = line.split(" status=").nth(1)?;
    Some(value.split_whitespace().next()?.to_string())
}

fn page_order(section: &str) -> Vec<String> {
    section
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("ページ ")?;
            let file = rest
                .split_whitespace()
                .find_map(|field| field.strip_prefix("file="))?;
            Some(file.to_string())
        })
        .collect()
}

fn has_file_named(root: &Path, name: &str) -> bool {
    files_recursive(root)
        .iter()
        .any(|path| path.file_name().and_then(|value| value.to_str()) == Some(name))
}

#[test]
fn multi_folder_run_writes_sidecars_and_log() {
    let env = TestEnv::new("p1-multi");
    let g1 = env.work("work/g1");
    write(&g1, "001.png", &novelai_png());
    write(&g1, "002.png", &no_metadata_png());
    write(&g1, "004.png", &broken_image());
    write(&g1, "005.png", &stealth_only_png());
    write(&g1, "006.png", &a1111_png(A1111_PARAMETERS));
    write(&g1, "note.txt", b"keep me");
    let sub = g1.join("sub");
    write(&sub, "001.png", &novelai_png());

    let g2 = env.work("work/g2");
    write(&g2, "001.png", &novelai_png_with("pos x", "neg y"));

    let empty = env.work("work/empty");
    fs::create_dir_all(&empty).unwrap();
    let file_arg = env.work("work/afile.txt");
    fs::write(&file_arg, b"not a folder").unwrap();

    let before: Vec<(PathBuf, Vec<u8>)> = [&g1, &g2, &sub]
        .iter()
        .flat_map(|directory| {
            files_recursive(directory)
                .into_iter()
                .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("png"))
                .map(|path| {
                    let bytes = fs::read(&path).unwrap();
                    (path, bytes)
                })
        })
        .collect();

    let g1_text = path_string(&g1);
    let duplicate_slash = format!("{g1_text}\\");
    let duplicate_upper = g1_text.to_uppercase();
    let result = env.run_strs(&[
        &g1_text,
        &path_string(&g2),
        &path_string(&empty),
        &path_string(&file_arg),
        &duplicate_slash,
        &duplicate_upper,
    ]);

    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    assert!(result.log_path.is_some(), "ログファイルが作られる");
    let log = result.log;

    let txt = g1.join("001.txt");
    assert!(txt.is_file(), "レコードがある画像には txt が出る");
    assert_eq!(
        fs::read(&txt).unwrap(),
        fastmeta_text(&g1.join("001.png")).into_bytes(),
        "txt は fast_meta CLI と同一の出力"
    );
    assert!(
        !g1.join("002.txt").exists(),
        "メタデータなしには txt を出さない"
    );
    assert!(
        !g1.join("004.txt").exists(),
        "壊れた画像には txt を出さない"
    );
    assert!(
        g1.join("005.txt").is_file(),
        "Stealth のみでもレコードがあれば txt を出す"
    );
    assert!(g1.join("006.txt").is_file(), "A1111 でも txt を出す");
    assert!(
        !sub.join("001.txt").exists(),
        "非再帰ではサブフォルダを処理しない"
    );
    assert_eq!(fs::read(g1.join("note.txt")).unwrap(), b"keep me");
    assert!(
        !has_file_named(&env.work("work"), "_gallery.txt"),
        "索引ファイルは作らない"
    );
    assert!(
        !env.exe_dir.join("story.db").exists(),
        "Phase 1 は DB を作らない"
    );

    assert!(log.contains("種別: リーフ"));
    assert!(log.contains("メタデータなし: 1"));
    assert!(log.contains("読めなかったページ: 1"));
    assert!(log.contains("無視したサブフォルダ: 1 (sub)"));
    assert!(log.contains("=== スキップ: 空フォルダ ==="));
    assert!(log.contains("[無視] フォルダではない引数"));
    assert_eq!(log.matches("[重複] 同じフォルダの重複指定").count(), 2);
    assert!(log.contains("未抽出ページ:"));
    assert!(log.contains("reason=メタデータなし"));

    for (path, bytes) in before {
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "画像は改変されない: {path:?}"
        );
    }
}

#[test]
fn rerun_skips_falls_back_and_overwrites_only_own_files() {
    let env = TestEnv::new("p1-collision");
    let gallery = env.work("work/collision");
    write(&gallery, "001.png", &novelai_png());
    write(&gallery, "004.png", &novelai_png_with("four", "neg four"));
    write(&gallery, "005.png", &novelai_png_with("five", "neg five"));
    write(&gallery, "006.jpg", &a1111_jpeg(A1111_PARAMETERS));
    write(&gallery, "006.png", &a1111_png(A1111_PARAMETERS));
    write(&gallery, "007.png", &novelai_png_with("seven", "neg seven"));
    let gallery_text = path_string(&gallery);

    let first = env.run_strs(&[&gallery_text]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    let section = gallery_section(&first.log, &gallery);
    assert_eq!(page_status(section, "001.png").as_deref(), Some("written"));
    assert_eq!(page_status(section, "006.jpg").as_deref(), Some("written"));
    assert_eq!(
        page_status(section, "006.png").as_deref(),
        Some("fallback_metadata"),
        "同 stem 異拡張子は自然順で先の .jpg が 006.txt を取り、.png は _metadata へ"
    );
    assert_eq!(
        fs::read(gallery.join("006.txt")).unwrap(),
        fastmeta_text(&gallery.join("006.jpg")).into_bytes()
    );
    assert_eq!(
        fs::read(gallery.join("006_metadata.txt")).unwrap(),
        fastmeta_text(&gallery.join("006.png")).into_bytes()
    );

    let own_before = fs::read(gallery.join("001.txt")).unwrap();
    let second = env.run_strs(&[&gallery_text]);
    let section = gallery_section(&second.log, &gallery);
    assert_eq!(
        page_status(section, "001.png").as_deref(),
        Some("skipped_exists")
    );
    assert_eq!(fs::read(gallery.join("001.txt")).unwrap(), own_before);

    let third = env.run_strs(&["--overwrite", &gallery_text]);
    let section = gallery_section(&third.log, &gallery);
    assert_eq!(
        page_status(section, "001.png").as_deref(),
        Some("overwritten")
    );
    assert_eq!(fs::read(gallery.join("001.txt")).unwrap(), own_before);

    // 他者の txt: _metadata へ退避
    fs::write(gallery.join("004.txt"), FOREIGN_TXT).unwrap();
    // 両方が他者: 何も書かず警告
    fs::write(gallery.join("005.txt"), FOREIGN_TXT).unwrap();
    fs::write(gallery.join("005_metadata.txt"), "other metadata\n").unwrap();
    // _metadata が自ツール製で base が他者: --overwrite で _metadata のみ上書き
    fs::rename(gallery.join("007.txt"), gallery.join("007_metadata.txt")).unwrap();
    fs::write(gallery.join("007.txt"), FOREIGN_TXT).unwrap();

    let fourth = env.run_strs(&["--overwrite", &gallery_text]);
    let section = gallery_section(&fourth.log, &gallery);
    assert_eq!(
        page_status(section, "004.png").as_deref(),
        Some("fallback_metadata")
    );
    assert_eq!(
        fs::read(gallery.join("004_metadata.txt")).unwrap(),
        fastmeta_text(&gallery.join("004.png")).into_bytes()
    );
    assert_eq!(
        page_status(section, "005.png").as_deref(),
        Some("skipped_exists")
    );
    assert_eq!(
        fs::read(gallery.join("005.txt")).unwrap(),
        FOREIGN_TXT.as_bytes()
    );
    assert_eq!(
        fs::read(gallery.join("005_metadata.txt")).unwrap(),
        b"other metadata\n"
    );
    assert!(fourth.log.contains("両方が他者のため書き込まない"));
    assert_eq!(
        page_status(section, "007.png").as_deref(),
        Some("overwritten")
    );
    assert_eq!(
        fs::read(gallery.join("007_metadata.txt")).unwrap(),
        fastmeta_text(&gallery.join("007.png")).into_bytes()
    );
    assert_eq!(
        fs::read(gallery.join("007.txt")).unwrap(),
        FOREIGN_TXT.as_bytes()
    );

    // フォルダ移動で見出し行のパスが変わっても、本文一致で自ツール製と判定する
    let moved = env.work("work/moved");
    fs::rename(&gallery, &moved).unwrap();
    let fifth = env.run_strs(&[&path_string(&moved)]);
    let section = gallery_section(&fifth.log, &moved);
    assert_eq!(
        page_status(section, "001.png").as_deref(),
        Some("skipped_exists")
    );
    let moved_text = fs::read(moved.join("001.txt")).unwrap();
    let moved_text = String::from_utf8_lossy(&moved_text).into_owned();
    assert_eq!(
        body_without_header(&moved_text),
        common::folders::own_sidecar_body(&moved.join("001.png"))
    );
}

#[test]
fn container_recursive_and_natural_order() {
    let env = TestEnv::new("p1-gallery");
    let root = env.work("work/root");
    let a = root.join("a");
    let b = root.join("b");
    write(&a, "001.png", &novelai_png());
    write(&b, "001.png", &novelai_png_with("b pos", "b neg"));

    let container = env.run_strs(&[&path_string(&root)]);
    assert_eq!(container.code, 0);
    assert!(container.log.contains("種別: コンテナ"));
    assert!(a.join("001.txt").is_file());
    assert!(b.join("001.txt").is_file());

    let mixed = env.work("work/mixed");
    write(&mixed, "001.png", &novelai_png());
    write(&mixed.join("child"), "002.png", &novelai_png());
    let not_recursive = env.run_strs(&[&path_string(&mixed)]);
    let section = gallery_section(&not_recursive.log, &mixed);
    assert!(section.contains("無視したサブフォルダ: 1 (child)"));
    assert!(!mixed.join("child").join("002.txt").exists());

    let recursive = env.run_strs(&["--recursive", &path_string(&mixed)]);
    assert_eq!(recursive.code, 0);
    assert!(mixed.join("child").join("002.txt").is_file());

    let natural = env.work("work/natural");
    let names = [
        "2.png",
        "10.png",
        "001.png",
        "1.png",
        "01.png",
        "a.png",
        "a1.png",
        "B.png",
        "全角１.png",
    ];
    for name in names {
        write(&natural, name, &no_metadata_png());
    }
    let run = env.run_strs(&[&path_string(&natural)]);
    let section = gallery_section(&run.log, &natural);
    assert_eq!(
        page_order(section),
        vec![
            "1.png",
            "01.png",
            "001.png",
            "2.png",
            "10.png",
            "B.png",
            "a1.png",
            "a.png",
            "全角１.png",
        ]
    );

    if junction_supported(&env.sandbox) {
        let loop_link = a.join("loop");
        create_junction(&loop_link, &root).expect("循環ジャンクションを作れる");
        let looped = env.run_strs(&["--recursive", &path_string(&root)]);
        assert_eq!(looped.code, 0);
        assert!(looped.log.contains("[スキップ] 訪問済みのフォルダ"));

        let alias = env.work("work/root-alias");
        create_junction(&alias, &root).expect("別名ジャンクションを作れる");
        let alias_run = env.run_strs(&[&path_string(&root), &path_string(&alias)]);
        assert_eq!(alias_run.code, 0);
        assert!(alias_run.log.contains("[重複] 同じフォルダの重複指定"));
    } else {
        eprintln!("ジャンクション非対応のため循環・別名テストをスキップ");
    }
}

#[test]
fn dry_run_predicts_without_writing() {
    let env = TestEnv::new("p1-dry-run");
    let gallery = env.work("work/gallery");
    write(&gallery, "001.png", &novelai_png());
    write(&gallery, "002.png", &no_metadata_png());
    let gallery_text = path_string(&gallery);

    let dry = env.run_strs(&["--dry-run", &gallery_text]);
    assert_eq!(dry.code, 0);
    let log_path = dry.log_path.as_ref().expect("dry-run のログ");
    assert!(log_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .starts_with("import_dryrun_"));
    assert!(
        !gallery.join("001.txt").exists(),
        "dry-run は txt を書かない"
    );
    assert!(!env.exe_dir.join("story.db").exists());
    let section = gallery_section(&dry.log, &gallery);
    assert_eq!(page_status(section, "001.png").as_deref(), Some("written"));
    assert_eq!(page_status(section, "002.png").as_deref(), Some("none"));

    let normal = env.run_strs(&[&gallery_text]);
    assert_eq!(normal.code, 0);
    let own = fs::read(gallery.join("001.txt")).unwrap();

    let dry_again = env.run_strs(&["--dry-run", &gallery_text]);
    let section = gallery_section(&dry_again.log, &gallery);
    assert_eq!(
        page_status(section, "001.png").as_deref(),
        Some("skipped_exists")
    );
    assert_eq!(fs::read(gallery.join("001.txt")).unwrap(), own);
}

#[test]
fn version_argument_errors_and_config() {
    let env = TestEnv::new("p1-cli");
    let gallery = env.work("work/gallery");
    write(&gallery, "001.png", &novelai_png());
    let gallery_text = path_string(&gallery);

    let version = env.run_strs(&["--version"]);
    assert_eq!(version.code, 0);
    assert!(version.stdout.contains("sdGalleryImporter"));
    assert!(version.log_path.is_none());
    assert!(
        log_files(&env.exe_dir).is_empty(),
        "version はログを作らない"
    );

    let version_and_error = env.run_strs(&["--version", "--bogus"]);
    assert_eq!(version_and_error.code, 1);
    assert!(version_and_error.log.contains("[引数エラー]"));

    let missing_db = env.run_strs(&["--db"]);
    assert_eq!(missing_db.code, 1);
    assert!(missing_db.log.contains("--db"));
    let duplicate_db = env.run_strs(&["--db", "a.db", "--db", "b.db", &gallery_text]);
    assert_eq!(duplicate_db.code, 1);

    let only_file = env.run_strs(&[&path_string(&env.work("work/afile.txt"))]);
    assert_eq!(only_file.code, 1);
    assert!(only_file.log.contains("有効なフォルダ引数が1つも無い"));

    env.write_config("db_path = 42\n");
    let bad_config = env.run_strs(&[&gallery_text]);
    assert_eq!(bad_config.code, 1);
    assert!(bad_config.log.contains("db_path が文字列ではない"));
    assert!(!env.exe_dir.join("story.db").exists());
    let bad_config_with_db = env.run_strs(&["--db", "x.db", &gallery_text]);
    assert_eq!(bad_config_with_db.code, 1, "設定不正は --db でも異常終了");

    env.write_config("db_path = \"unterminated\n");
    assert_eq!(env.run_strs(&[&gallery_text]).code, 1);

    env.write_config("db_path = \"data\\\\gallery.db\"\n");
    let configured = env.run_strs(&[&gallery_text]);
    assert_eq!(configured.code, 0);
    assert!(configured.log.contains("data\\gallery.db"));
    assert!(!env.exe_dir.join("data").join("gallery.db").exists());

    env.write_config("# db_path なし\n");
    let defaulted = env.run_strs(&[&gallery_text]);
    assert_eq!(defaulted.code, 0);
    assert!(defaulted.log.contains("story.db"));
    assert!(!env.exe_dir.join("story.db").exists());

    let cli_db = env.run_strs(&["--db", "relative.db", &gallery_text]);
    assert_eq!(cli_db.code, 0);
    assert!(cli_db
        .log
        .contains(&format!("{}", env.exe_dir.join("relative.db").display())));
    assert!(!env.exe_dir.join("relative.db").exists());
}

#[test]
fn one_failed_page_or_gallery_does_not_stop_the_run() {
    let env = TestEnv::new("p1-failure");
    let g1 = env.work("work/g1");
    write(&g1, "001.png", &novelai_png());
    fs::create_dir_all(g1.join("001.txt")).unwrap(); // txt を書けない状況
    write(&g1, "002.png", &broken_image());
    let g2 = env.work("work/g2");
    write(&g2, "001.png", &novelai_png());

    let result = env.run_strs(&[&path_string(&g1), &path_string(&g2)]);
    assert_eq!(result.code, 0, "ページ失敗は全体異常にしない");
    let section = gallery_section(&result.log, &g1);
    assert_eq!(page_status(section, "001.png").as_deref(), Some("failed"));
    assert_eq!(page_status(section, "002.png").as_deref(), Some("none"));
    assert!(result.log.contains("読めなかったページ: 1"));
    assert!(g2.join("001.txt").is_file(), "兄弟ギャラリーは処理される");
}

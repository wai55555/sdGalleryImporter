//! Phase 0 で確定した「本ツール以前に作られた DB」のフィクスチャ。
//!
//! 共有スキーマ（仕様書 §4.1）だけを持ち、本ツールの追加列・追加テーブルは無い状態を
//! 再現する。Phase 2 のマイグレーション／バックフィル／重複警告のテストで使う。

use rusqlite::Connection;
use std::path::Path;

/// `lines` の行データ。`prompt_hash` 列の状態を選べる。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashColumn {
    /// 本ツール以前の DB（列そのものが無い）。
    Absent,
    /// 列はあるが全行 NULL（バックフィル対象）。
    Null,
}

/// §4.1 の共有スキーマだけを持つ DB を作る。
///
/// `rows` は (prompt, negative) の一覧。同じ組を複数入れると重複行のフィクスチャになる。
pub fn write_legacy_db(
    path: &Path,
    rows: &[(&str, &str)],
    hash_column: HashColumn,
) -> rusqlite::Result<()> {
    if path.exists() {
        std::fs::remove_file(path).expect("既存の DB を消せる");
    }
    let connection = Connection::open(path)?;
    connection.execute_batch(
        r#"
        CREATE TABLE lines (
          id INTEGER PRIMARY KEY,
          prompt TEXT NOT NULL,
          negative TEXT NOT NULL DEFAULT '',
          width INTEGER, height INTEGER,
          tags TEXT,
          archived INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE stories (
          id INTEGER PRIMARY KEY,
          title TEXT NOT NULL,
          quality_prompt TEXT NOT NULL DEFAULT '',  quality_negative TEXT NOT NULL DEFAULT '',
          style_prompt TEXT NOT NULL DEFAULT '',    style_negative TEXT NOT NULL DEFAULT '',
          character_prompt TEXT NOT NULL DEFAULT '', character_negative TEXT NOT NULL DEFAULT '',
          workflow TEXT, seed_base INTEGER,
          status TEXT NOT NULL DEFAULT 'draft'
        );
        CREATE TABLE placements (
          id INTEGER PRIMARY KEY,
          story_id INTEGER NOT NULL REFERENCES stories(id),
          line_id INTEGER NOT NULL REFERENCES lines(id),
          ord INTEGER NOT NULL,
          repeat INTEGER NOT NULL DEFAULT 1,
          extra TEXT NOT NULL DEFAULT '', extra_neg TEXT NOT NULL DEFAULT ''
        );
        "#,
    )?;
    if hash_column == HashColumn::Null {
        connection.execute_batch("ALTER TABLE lines ADD COLUMN prompt_hash TEXT;")?;
    }
    connection.execute(
        "INSERT INTO stories (title, status) VALUES ('legacy story', 'draft')",
        [],
    )?;
    for (index, (prompt, negative)) in rows.iter().enumerate() {
        connection.execute(
            "INSERT INTO lines (prompt, negative, width, height) VALUES (?1, ?2, 512, 768)",
            rusqlite::params![prompt, negative],
        )?;
        connection.execute(
            "INSERT INTO placements (story_id, line_id, ord) VALUES (1, ?1, ?2)",
            rusqlite::params![index as i64 + 1, index as i64],
        )?;
    }
    Ok(())
}

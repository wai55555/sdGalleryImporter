//! 共有 SQLite のスキーマ用意と取り込み（§4）。
//!
//! - 接続直後に `PRAGMA foreign_keys = ON`
//! - スキーマ作成・列追加・バックフィル・索引作成は 1 トランザクション（REQ-4.3）
//! - リーフ 1 件の書き込みは 1 トランザクション（REQ-4.4）

use crate::hash;
use crate::import::{ImportCounts, ImportedLeaf, LeafPlan, SegmentPlan, SegmentRowInfo};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::Path;

pub struct GalleryRow {
    pub id: i64,
    pub story_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RunTotals {
    pub gallery_count: i64,
    pub page_count: i64,
    pub segment_count: i64,
    pub unextracted_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct GalleryStats {
    pub segment_count: i64,
    pub metadata_less: i64,
    pub unreadable: Vec<(i64, String)>,
    pub unextracted: Vec<(i64, String, String)>,
}

pub struct Database {
    connection: Connection,
    writable: bool,
    has_import_schema: bool,
}

impl Database {
    pub fn open_normal(path: &Path) -> Result<Self, String> {
        let connection =
            Connection::open(path).map_err(|error| format!("DB を開けない: {error}"))?;
        configure(&connection)?;
        Ok(Self {
            connection,
            writable: true,
            has_import_schema: false,
        })
    }

    /// 存在する DB を読み取り専用で開く。存在しなければ Ok(None)。
    pub fn open_readonly(path: &Path) -> Result<Option<Self>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| format!("DB を読み取り専用で開けない: {error}"))?;
        configure(&connection)?;
        let mut database = Self {
            connection,
            writable: false,
            has_import_schema: false,
        };
        database.has_import_schema = database.check_import_schema()?;
        Ok(Some(database))
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    pub fn has_import_schema(&self) -> bool {
        self.has_import_schema
    }

    fn check_import_schema(&self) -> Result<bool, String> {
        for table in ["galleries", "pages", "segments"] {
            if !self.table_exists(table)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn table_exists(&self, table: &str) -> Result<bool, String> {
        let count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(|error| format!("DB のスキーマを確認できない: {error}"))?;
        Ok(count > 0)
    }

    /// スキーマ・列・バックフィル・索引を 1 トランザクションで用意する。
    pub fn prepare_schema(&mut self) -> Result<Vec<(String, Vec<i64>)>, String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("スキーマ用トランザクションを開始できない: {error}"))?;

        transaction
            .execute_batch(SCHEMA_SQL)
            .map_err(|error| format!("スキーマを作成できない: {error}"))?;
        add_column(&transaction, "lines", "prompt_hash", "TEXT")?;
        add_column(
            &transaction,
            "lines",
            "derived_from_line_id",
            "INTEGER REFERENCES lines(id)",
        )?;
        add_column(&transaction, "lines", "origin", "TEXT")?;
        add_column(
            &transaction,
            "placements",
            "segment_id",
            "INTEGER REFERENCES segments(id)",
        )?;
        add_column(
            &transaction,
            "stories",
            "origin_gallery_id",
            "INTEGER REFERENCES galleries(id)",
        )?;

        backfill_prompt_hash(&transaction)?;
        let duplicates = duplicate_hashes(&transaction)?;

        transaction
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_lines_prompt_hash ON lines(prompt_hash);",
            )
            .map_err(|error| format!("索引を作成できない: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("スキーマの確定に失敗した: {error}"))?;
        self.has_import_schema = true;
        Ok(duplicates)
    }

    pub fn start_run(&mut self, started_at: &str, log_path: Option<&str>) -> Result<i64, String> {
        self.connection
            .execute(
                "INSERT INTO import_runs (started_at, log_path) VALUES (?1, ?2)",
                params![started_at, log_path],
            )
            .map_err(|error| format!("import_runs を作成できない: {error}"))?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn finish_run(
        &mut self,
        run_id: i64,
        finished_at: &str,
        counts: ImportCounts,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE import_runs SET finished_at = ?1, gallery_count = ?2, page_count = ?3,
                 segment_count = ?4, unextracted_count = ?5 WHERE id = ?6",
                params![
                    finished_at,
                    counts.gallery_count as i64,
                    counts.page_count as i64,
                    counts.segment_count as i64,
                    counts.unextracted_count as i64,
                    run_id
                ],
            )
            .map_err(|error| format!("import_runs を更新できない: {error}"))?;
        Ok(())
    }

    pub fn run_totals(&self, run_id: i64) -> Result<RunTotals, String> {
        self.connection
            .query_row(
                "SELECT gallery_count, page_count, segment_count, unextracted_count
                 FROM import_runs WHERE id = ?1",
                [run_id],
                |row| {
                    Ok(RunTotals {
                        gallery_count: row.get(0)?,
                        page_count: row.get(1)?,
                        segment_count: row.get(2)?,
                        unextracted_count: row.get(3)?,
                    })
                },
            )
            .map_err(|error| format!("import_runs を読めない: {error}"))
    }

    pub fn existing_gallery(&self, root_path: &str) -> Result<Option<GalleryRow>, String> {
        if !self.has_import_schema {
            return Ok(None);
        }
        self.connection
            .query_row(
                "SELECT id, story_id FROM galleries WHERE root_path = ?1",
                [root_path],
                |row| {
                    Ok(GalleryRow {
                        id: row.get(0)?,
                        story_id: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("galleries を読めない: {error}"))
    }

    pub fn insert_container(
        &mut self,
        root_path: &str,
        name: &str,
        parent_id: Option<i64>,
        run_id: i64,
        imported_at: &str,
    ) -> Result<i64, String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("コンテナ用トランザクションを開始できない: {error}"))?;
        transaction
            .execute(
                "INSERT INTO galleries (parent_id, root_path, name, story_id, import_run_id, imported_at)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                params![parent_id, root_path, name, run_id, imported_at],
            )
            .map_err(|error| format!("galleries を作成できない: {error}"))?;
        let id = transaction.last_insert_rowid();
        transaction
            .commit()
            .map_err(|error| format!("コンテナの確定に失敗した: {error}"))?;
        Ok(id)
    }

    /// リーフ 1 件を 1 トランザクションで取り込む。
    ///
    /// `existing_id` が Some の場合は、コンテナ行をリーフへ変換する（REQ-9.2）。
    pub fn import_leaf(
        &mut self,
        plan: &LeafPlan,
        existing_id: Option<i64>,
        run_id: i64,
    ) -> Result<ImportedLeaf, String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("リーフ用トランザクションを開始できない: {error}"))?;

        let gallery_id = match existing_id {
            Some(id) => {
                transaction
                    .execute(
                        "UPDATE galleries SET import_run_id = ?1, imported_at = ?2 WHERE id = ?3",
                        params![run_id, plan.imported_at, id],
                    )
                    .map_err(|error| format!("galleries を更新できない: {error}"))?;
                id
            }
            None => {
                transaction
                    .execute(
                        "INSERT INTO galleries (parent_id, root_path, name, story_id, import_run_id, imported_at)
                         VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                        params![
                            plan.parent_id,
                            plan.root_path,
                            plan.name,
                            run_id,
                            plan.imported_at
                        ],
                    )
                    .map_err(|error| format!("galleries を作成できない: {error}"))?;
                transaction.last_insert_rowid()
            }
        };

        transaction
            .execute(
                "INSERT INTO stories (title, status, origin_gallery_id) VALUES (?1, 'draft', ?2)",
                params![plan.name, gallery_id],
            )
            .map_err(|error| format!("stories を作成できない: {error}"))?;
        let story_id = transaction.last_insert_rowid();
        transaction
            .execute(
                "UPDATE galleries SET story_id = ?1 WHERE id = ?2",
                params![story_id, gallery_id],
            )
            .map_err(|error| format!("galleries.story_id を更新できない: {error}"))?;

        let mut segment_rows: Vec<SegmentRowInfo> = Vec::with_capacity(plan.segments.len());
        for segment in &plan.segments {
            let line_id = find_or_create_line(&transaction, segment)?;
            transaction
                .execute(
                    "INSERT INTO segments
                     (gallery_id, seq, run_length, line_id, prompt_full, negative_full, width, height)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        gallery_id,
                        segment.seq,
                        segment.run_length,
                        line_id,
                        segment.pos,
                        segment.neg,
                        segment.width.map(i64::from),
                        segment.height.map(i64::from)
                    ],
                )
                .map_err(|error| format!("segments を作成できない: {error}"))?;
            segment_rows.push(SegmentRowInfo {
                id: transaction.last_insert_rowid(),
                line_id,
                seq: segment.seq,
                run_length: segment.run_length,
            });
        }

        for page in &plan.pages {
            let segment_id = page
                .segment_index
                .and_then(|index| segment_rows.get(index))
                .map(|row| row.id);
            transaction
                .execute(
                    "INSERT INTO pages
                     (gallery_id, file_name, seq, width, height, source, prompt_hash, segment_id, txt_status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        gallery_id,
                        page.file_name,
                        page.seq,
                        page.width.map(i64::from),
                        page.height.map(i64::from),
                        page.source,
                        page.prompt_hash,
                        segment_id,
                        page.txt_status
                    ],
                )
                .map_err(|error| format!("pages を作成できない ({}): {error}", page.file_name))?;
        }

        for row in &segment_rows {
            transaction
                .execute(
                    "INSERT INTO placements (story_id, line_id, ord, repeat, segment_id)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![story_id, row.line_id, row.seq, row.run_length, row.id],
                )
                .map_err(|error| format!("placements を作成できない: {error}"))?;
        }

        transaction
            .commit()
            .map_err(|error| format!("リーフの確定に失敗した: {error}"))?;

        Ok(ImportedLeaf {
            gallery_id,
            story_id,
            segments: segment_rows,
        })
    }

    pub fn gallery_stats(&self, gallery_id: i64) -> Result<GalleryStats, String> {
        let segment_count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE gallery_id = ?1",
                [gallery_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("segments を数えられない: {error}"))?;
        let metadata_less: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM pages WHERE gallery_id = ?1 AND source = 'none'",
                [gallery_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("pages を数えられない: {error}"))?;

        let mut unreadable = Vec::new();
        let mut statement = self
            .connection
            .prepare(
                "SELECT seq, file_name FROM pages
                 WHERE gallery_id = ?1 AND source = 'error' ORDER BY seq",
            )
            .map_err(|error| format!("pages を準備できない: {error}"))?;
        let rows = statement
            .query_map([gallery_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| format!("pages を読めない: {error}"))?;
        for row in rows {
            unreadable.push(row.map_err(|error| format!("pages を読めない: {error}"))?);
        }

        let mut unextracted = Vec::new();
        let mut statement = self
            .connection
            .prepare(
                "SELECT seq, file_name, source FROM pages
                 WHERE gallery_id = ?1 AND prompt_hash IS NULL ORDER BY seq",
            )
            .map_err(|error| format!("pages を準備できない: {error}"))?;
        let rows = statement
            .query_map([gallery_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|error| format!("pages を読めない: {error}"))?;
        for row in rows {
            unextracted.push(row.map_err(|error| format!("pages を読めない: {error}"))?);
        }

        Ok(GalleryStats {
            segment_count,
            metadata_less,
            unreadable,
            unextracted,
        })
    }
}

fn configure(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| format!("外部キーを有効化できない: {error}"))?;
    Ok(())
}

fn add_column(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    if table_columns(transaction, table)?
        .iter()
        .any(|name| name == column)
    {
        return Ok(());
    }
    transaction
        .execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition};"
        ))
        .map_err(|error| format!("{table}.{column} を追加できない: {error}"))
}

fn table_columns(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
) -> Result<Vec<String>, String> {
    let mut statement = transaction
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("{table} の列を確認できない: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("{table} の列を確認できない: {error}"))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(|error| format!("{table} の列を確認できない: {error}"))?);
    }
    Ok(columns)
}

fn backfill_prompt_hash(transaction: &rusqlite::Transaction<'_>) -> Result<(), String> {
    let mut rows = Vec::new();
    {
        let mut statement = transaction
            .prepare("SELECT id, prompt, negative FROM lines WHERE prompt_hash IS NULL")
            .map_err(|error| format!("バックフィル対象を読めない: {error}"))?;
        let mapped = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| format!("バックフィル対象を読めない: {error}"))?;
        for row in mapped {
            rows.push(row.map_err(|error| format!("バックフィル対象を読めない: {error}"))?);
        }
    }
    for (id, prompt, negative) in rows {
        transaction
            .execute(
                "UPDATE lines SET prompt_hash = ?1 WHERE id = ?2",
                params![hash::pair_hash(&prompt, &negative), id],
            )
            .map_err(|error| format!("prompt_hash を更新できない: {error}"))?;
    }
    Ok(())
}

fn duplicate_hashes(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<Vec<(String, Vec<i64>)>, String> {
    let mut rows = Vec::new();
    {
        let mut statement = transaction
            .prepare(
                "SELECT prompt_hash, id FROM lines
                 WHERE prompt_hash IS NOT NULL ORDER BY prompt_hash, id",
            )
            .map_err(|error| format!("重複ハッシュを確認できない: {error}"))?;
        let mapped = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| format!("重複ハッシュを確認できない: {error}"))?;
        for row in mapped {
            rows.push(row.map_err(|error| format!("重複ハッシュを確認できない: {error}"))?);
        }
    }
    let mut duplicates = Vec::new();
    let mut current: Option<(String, Vec<i64>)> = None;
    for (prompt_hash, id) in rows {
        match &mut current {
            Some((hash_value, ids)) if *hash_value == prompt_hash => ids.push(id),
            Some((hash_value, ids)) => {
                if ids.len() > 1 {
                    duplicates.push((hash_value.clone(), ids.clone()));
                }
                current = Some((prompt_hash, vec![id]));
            }
            None => current = Some((prompt_hash, vec![id])),
        }
    }
    if let Some((hash_value, ids)) = current {
        if ids.len() > 1 {
            duplicates.push((hash_value, ids));
        }
    }
    Ok(duplicates)
}

fn find_or_create_line(
    transaction: &rusqlite::Transaction<'_>,
    segment: &SegmentPlan,
) -> Result<i64, String> {
    let existing: Option<i64> = transaction
        .query_row(
            "SELECT id FROM lines WHERE prompt_hash = ?1 ORDER BY id LIMIT 1",
            [&segment.prompt_hash],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("lines を照合できない: {error}"))?;
    if let Some(id) = existing {
        return Ok(id);
    }
    transaction
        .execute(
            "INSERT INTO lines (prompt, negative, width, height, origin, prompt_hash)
             VALUES (?1, ?2, ?3, ?4, 'cg_import', ?5)",
            params![
                segment.pos,
                segment.neg,
                segment.width.map(i64::from),
                segment.height.map(i64::from),
                segment.prompt_hash
            ],
        )
        .map_err(|error| format!("lines を作成できない: {error}"))?;
    Ok(transaction.last_insert_rowid())
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS lines (
  id INTEGER PRIMARY KEY,
  prompt TEXT NOT NULL,
  negative TEXT NOT NULL DEFAULT '',
  width INTEGER, height INTEGER,
  tags TEXT,
  archived INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS stories (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL,
  quality_prompt TEXT NOT NULL DEFAULT '',  quality_negative TEXT NOT NULL DEFAULT '',
  style_prompt TEXT NOT NULL DEFAULT '',    style_negative TEXT NOT NULL DEFAULT '',
  character_prompt TEXT NOT NULL DEFAULT '', character_negative TEXT NOT NULL DEFAULT '',
  workflow TEXT, seed_base INTEGER,
  status TEXT NOT NULL DEFAULT 'draft'
);
CREATE TABLE IF NOT EXISTS placements (
  id INTEGER PRIMARY KEY,
  story_id INTEGER NOT NULL REFERENCES stories(id),
  line_id INTEGER NOT NULL REFERENCES lines(id),
  ord INTEGER NOT NULL,
  repeat INTEGER NOT NULL DEFAULT 1,
  extra TEXT NOT NULL DEFAULT '', extra_neg TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS import_runs (
  id INTEGER PRIMARY KEY,
  started_at TEXT NOT NULL, finished_at TEXT,
  log_path TEXT,
  gallery_count INTEGER NOT NULL DEFAULT 0,
  page_count INTEGER NOT NULL DEFAULT 0,
  segment_count INTEGER NOT NULL DEFAULT 0,
  unextracted_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS galleries (
  id INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES galleries(id),
  root_path TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  story_id INTEGER REFERENCES stories(id),
  import_run_id INTEGER REFERENCES import_runs(id),
  imported_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pages (
  id INTEGER PRIMARY KEY,
  gallery_id INTEGER NOT NULL REFERENCES galleries(id),
  file_name TEXT NOT NULL,
  seq INTEGER NOT NULL,
  width INTEGER, height INTEGER,
  source TEXT NOT NULL,
  prompt_hash TEXT,
  segment_id INTEGER REFERENCES segments(id),
  txt_status TEXT NOT NULL,
  UNIQUE(gallery_id, file_name)
);
CREATE TABLE IF NOT EXISTS segments (
  id INTEGER PRIMARY KEY,
  gallery_id INTEGER NOT NULL REFERENCES galleries(id),
  seq INTEGER NOT NULL,
  run_length INTEGER NOT NULL,
  line_id INTEGER NOT NULL REFERENCES lines(id),
  prompt_full TEXT NOT NULL, negative_full TEXT NOT NULL,
  width INTEGER, height INTEGER,
  UNIQUE(gallery_id, seq)
);
"#;

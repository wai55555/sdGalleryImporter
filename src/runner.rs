//! 実行本体: フォルダ引数 → ギャラリー境界 → ページ列挙 → txt → 抽出 → DB → ログ。
//!
//! Phase 2 では NovelAI の pos/neg 抽出、寸法、segment、下書きストーリー、
//! 行の照合、再取り込みスキップまでを扱う。

use crate::cli::Cli;
use crate::config::Settings;
use crate::db::{Database, GalleryStats};
use crate::dimensions;
use crate::extract::{self, Analysis, SourceKind};
use crate::hash;
use crate::import::{ImportCounts, LeafPlan, PagePlan, SegmentPlan};
use crate::logger::Logger;
use crate::natural;
use crate::paths;
use crate::txt::{self, TxtOutcome, TxtStatus};
use fast_meta::Format;
use std::fs;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

struct InputFolder {
    path: PathBuf,
    key: String,
}

pub fn run(cli: &Cli, settings: &Settings, logger: &mut Logger) -> i32 {
    log_conditions(cli, settings, logger);

    let inputs = collect_inputs(cli, logger);
    if inputs.is_empty() {
        logger.line("[異常] 有効なフォルダ引数が1つも無い");
        return 1;
    }
    logger.line(format!("有効なフォルダ数: {}", inputs.len()));
    logger.blank();

    let mut database = if cli.dry_run {
        match Database::open_readonly(&settings.db_path) {
            Ok(database) => database,
            Err(error) => {
                logger.line(format!("[異常] {error}"));
                return 1;
            }
        }
    } else {
        match Database::open_normal(&settings.db_path) {
            Ok(database) => Some(database),
            Err(error) => {
                logger.line(format!("[異常] {error}"));
                return 1;
            }
        }
    };

    let mut run_id = None;
    if !cli.dry_run {
        let database = database.as_mut().expect("通常実行では DB を開いている");
        match database.prepare_schema() {
            Ok(duplicates) => {
                for (prompt_hash, ids) in duplicates {
                    logger.line(format!(
                        "[警告] prompt_hash が重複している既存行: {prompt_hash} ids={ids:?}（最小 id を採用）"
                    ));
                }
            }
            Err(error) => {
                logger.line(format!("[異常] スキーマを用意できない: {error}"));
                return 1;
            }
        }
        let log_path = log_path_string(logger);
        match database.start_run(&logger.started_at(), log_path.as_deref()) {
            Ok(id) => run_id = Some(id),
            Err(error) => {
                logger.line(format!("[異常] {error}"));
                return 1;
            }
        }
    } else if let Some(database) = database.as_ref() {
        if database.has_import_schema() {
            logger.line("DB: 読み取り専用で開いた（dry-run のため書き込みなし）");
        } else {
            logger.line("DB: 追加テーブルが無いため全フォルダを未取り込みとして扱う");
        }
    } else {
        logger.line("DB: 存在しないため全フォルダを未取り込みとして扱う");
    }

    let exit_code = {
        let mut runner = Runner {
            cli,
            logger,
            visited: std::collections::HashSet::new(),
            database: database.as_mut(),
            run_id,
            counts: ImportCounts::default(),
        };
        for input in inputs {
            runner.process_folder(&input.path, input.key, None);
        }
        runner.finish()
    };

    exit_code
}

fn log_path_string(logger: &Logger) -> Option<String> {
    logger
        .path()
        .map(|path| path.to_string_lossy().into_owned())
}

fn log_conditions(cli: &Cli, settings: &Settings, logger: &mut Logger) {
    logger.line(format!("sdGalleryImporter {}", crate::VERSION));
    logger.line(format!("実行日時: {}", logger.started_at()));
    logger.line(format!(
        "モード: {}",
        if cli.dry_run { "dry-run" } else { "通常" }
    ));
    logger.line(format!(
        "DB: {} (由来: {}{})",
        settings.db_path.display(),
        settings.db_path_source.label(),
        if cli.dry_run {
            " / 読み取り専用"
        } else {
            ""
        }
    ));
    logger.line(format!(
        "オプション: overwrite={} recursive={} dry_run={}",
        cli.overwrite, cli.recursive, cli.dry_run
    ));
    logger.line("対象フォルダ引数:");
    if cli.folders.is_empty() {
        logger.line("  (なし)");
    }
    for folder in &cli.folders {
        logger.line(format!("  - {folder}"));
    }
    logger.blank();
}

fn collect_inputs(cli: &Cli, logger: &mut Logger) -> Vec<InputFolder> {
    let mut inputs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in &cli.folders {
        let path = PathBuf::from(raw);
        match fs::metadata(&path) {
            Err(error) => {
                logger.line(format!(
                    "[無視] 存在しない、または読めない引数: {raw} ({error})"
                ));
                continue;
            }
            Ok(metadata) if !metadata.is_dir() => {
                logger.line(format!("[無視] フォルダではない引数: {raw}"));
                continue;
            }
            Ok(_) => {}
        }
        let absolute = paths::absolute(&path);
        let key = match paths::normalize_key(&absolute) {
            Ok(key) => key,
            Err(error) => {
                logger.line(format!(
                    "[失敗] パスを正規化できない: {} ({error})",
                    absolute.display()
                ));
                continue;
            }
        };
        if !seen.insert(key.clone()) {
            logger.line(format!(
                "[重複] 同じフォルダの重複指定: {}",
                absolute.display()
            ));
            continue;
        }
        inputs.push(InputFolder {
            path: absolute,
            key,
        });
    }
    inputs
}

struct Runner<'a> {
    cli: &'a Cli,
    logger: &'a mut Logger,
    visited: std::collections::HashSet<String>,
    database: Option<&'a mut Database>,
    run_id: Option<i64>,
    counts: ImportCounts,
}

#[derive(Default)]
struct TxtCounts {
    written: usize,
    overwritten: usize,
    skipped_exists: usize,
    fallback_metadata: usize,
    failed: usize,
    none: usize,
}

impl TxtCounts {
    fn add(&mut self, status: TxtStatus) {
        match status {
            TxtStatus::Written => self.written += 1,
            TxtStatus::Overwritten => self.overwritten += 1,
            TxtStatus::SkippedExists => self.skipped_exists += 1,
            TxtStatus::FallbackMetadata => self.fallback_metadata += 1,
            TxtStatus::Failed => self.failed += 1,
            TxtStatus::None => self.none += 1,
        }
    }

    fn log(&self, logger: &mut Logger, pages: usize) {
        logger.line(format!(
            "集計: pages={pages} written={} overwritten={} skipped_exists={} fallback_metadata={} failed={} none={}",
            self.written,
            self.overwritten,
            self.skipped_exists,
            self.fallback_metadata,
            self.failed,
            self.none
        ));
    }
}

struct PageState {
    file_name: String,
    seq: i64,
    width: Option<u32>,
    height: Option<u32>,
    source: SourceKind,
    pos: Option<String>,
    neg: String,
    prompt_hash: Option<String>,
    segment_index: Option<usize>,
    txt_status: TxtStatus,
    record_count: usize,
    format: Option<Format>,
    error: Option<String>,
    metadata_less: bool,
    note: Option<String>,
}

impl PageState {
    fn plan(&self) -> PagePlan {
        PagePlan {
            file_name: self.file_name.clone(),
            seq: self.seq,
            width: self.width,
            height: self.height,
            source: self.source.as_str(),
            prompt_hash: self.prompt_hash.clone(),
            segment_index: self.segment_index,
            txt_status: self.txt_status.as_str(),
        }
    }
}

impl<'a> Runner<'a> {
    fn process_folder(&mut self, path: &Path, key: String, parent_id: Option<i64>) {
        if !self.visited.insert(key.clone()) {
            self.logger
                .line(format!("[スキップ] 訪問済みのフォルダ: {}", path.display()));
            return;
        }

        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                self.logger.line(format!(
                    "[失敗] フォルダを列挙できない: {} ({error})",
                    path.display()
                ));
                return;
            }
        };

        let mut images: Vec<(String, PathBuf)> = Vec::new();
        let mut directories: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.logger.line(format!(
                        "[警告] フォルダ内の項目を読めない: {} ({error})",
                        path.display()
                    ));
                    continue;
                }
            };
            let child = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if child.is_dir() {
                directories.push((name, child));
            } else if child.is_file() && paths::is_image_file(&child) {
                images.push((name, child));
            }
        }
        images.sort_by(|left, right| natural::compare(&left.0, &right.0));
        directories.sort_by(|left, right| natural::compare(&left.0, &right.0));

        if images.is_empty() {
            if directories.is_empty() {
                self.logger.line("=== スキップ: 空フォルダ ===");
                self.logger.line(format!("パス: {}", path.display()));
                self.logger.blank();
                return;
            }
            self.process_container(path, &key, &directories, parent_id);
            return;
        }

        self.process_leaf(path, &key, &images, &directories, parent_id);
    }

    fn process_leaf(
        &mut self,
        path: &Path,
        key: &str,
        images: &[(String, PathBuf)],
        directories: &[(String, PathBuf)],
        parent_id: Option<i64>,
    ) {
        self.logger
            .line(format!("=== ギャラリー: {} ===", path.display()));
        self.logger.line("種別: リーフ");
        self.logger.line(format!("ページ数: {}", images.len()));

        let existing = match self.database.as_deref() {
            Some(database) => match database.existing_gallery(key) {
                Ok(existing) => existing,
                Err(error) => {
                    self.logger.line(format!("[失敗] {error}"));
                    self.logger.blank();
                    return;
                }
            },
            None => None,
        };

        if let Some(row) = existing.as_ref().filter(|row| row.story_id.is_some()) {
            self.process_imported_leaf(path, images, directories, row.id);
            return;
        }

        let mut pages = self.process_pages_fresh(images);
        let segments = build_segments(&mut pages);
        let plan = LeafPlan {
            root_path: key.to_string(),
            name: folder_name(path),
            parent_id,
            imported_at: now_iso(),
            pages: pages.iter().map(PageState::plan).collect(),
            segments,
        };

        let imported = if self.cli.dry_run {
            None
        } else {
            let existing_id = existing.as_ref().map(|row| row.id);
            match self.database.as_deref_mut() {
                Some(database) => match database.import_leaf(
                    &plan,
                    existing_id,
                    self.run_id.expect("通常実行では import_run がある"),
                ) {
                    Ok(imported) => Some(imported),
                    Err(error) => {
                        self.logger.line(format!(
                            "[失敗] リーフを取り込めない: {} ({error})",
                            path.display()
                        ));
                        None
                    }
                },
                None => None,
            }
        };

        let mut txt_counts = TxtCounts::default();
        let mut metadata_less = Vec::new();
        let mut unreadable = Vec::new();
        for page in &pages {
            txt_counts.add(page.txt_status);
            if page.metadata_less {
                metadata_less.push(page);
            } else if page.error.is_some() {
                unreadable.push(page);
            }
        }
        txt_counts.log(self.logger, pages.len());
        self.log_segment_composition(&plan, imported.as_ref());
        if let Some(imported) = &imported {
            self.counts.add_leaf(&plan, true);
            self.logger.line(format!("story_id: {}", imported.story_id));
        } else if self.cli.dry_run {
            self.counts.add_leaf(&plan, true);
        }
        self.logger
            .line(format!("メタデータなし: {}", metadata_less.len()));
        self.logger
            .line(format!("読めなかったページ: {}", unreadable.len()));
        self.log_unextracted(&pages, None);

        let child_parent = imported
            .as_ref()
            .map(|imported| imported.gallery_id)
            .or_else(|| existing.as_ref().map(|row| row.id));

        if self.cli.recursive {
            self.recurse(directories, child_parent);
        } else if !directories.is_empty() {
            self.log_ignored(directories);
        }
        self.logger.blank();
    }

    fn process_imported_leaf(
        &mut self,
        path: &Path,
        images: &[(String, PathBuf)],
        directories: &[(String, PathBuf)],
        gallery_id: i64,
    ) {
        self.logger.line("取り込み済みのため DB 書き込みをスキップ");
        let mut txt_counts = TxtCounts::default();
        for (seq, (file_name, image_path)) in images.iter().enumerate() {
            let page = self.process_page_txt_only(seq as i64, file_name, image_path);
            txt_counts.add(page.txt_status);
            self.log_page(&page);
        }
        txt_counts.log(self.logger, images.len());

        let stats = match self.database.as_deref() {
            Some(database) => database.gallery_stats(gallery_id).ok(),
            None => None,
        };
        if let Some(stats) = stats.as_ref() {
            self.logger
                .line(format!("セグメント: {}", stats.segment_count));
            self.logger
                .line(format!("メタデータなし: {}", stats.metadata_less));
            self.logger
                .line(format!("読めなかったページ: {}", stats.unreadable.len()));
            self.log_unextracted(&[], Some(stats));
        } else {
            self.logger.line("[警告] 既存 DB から集計を読めない");
            self.logger.line("セグメント: -");
            self.logger.line("メタデータなし: -");
            self.logger.line("読めなかったページ: -");
            self.logger.line("未抽出ページ:");
            self.logger.line("  なし");
        }

        if self.cli.recursive {
            self.recurse(directories, Some(gallery_id));
        } else if !directories.is_empty() {
            self.log_ignored(directories);
        }
        let _ = path;
        self.logger.blank();
    }

    fn process_container(
        &mut self,
        path: &Path,
        key: &str,
        directories: &[(String, PathBuf)],
        parent_id: Option<i64>,
    ) {
        let existing = match self.database.as_deref() {
            Some(database) => match database.existing_gallery(key) {
                Ok(existing) => existing,
                Err(error) => {
                    self.logger.line(format!("[失敗] {error}"));
                    return;
                }
            },
            None => None,
        };

        self.logger
            .line(format!("=== ギャラリー: {} ===", path.display()));
        self.logger.line("種別: コンテナ");
        self.logger.line("ページ数: 0");

        let gallery_id = match existing.as_ref() {
            Some(row) => {
                if row.story_id.is_some() {
                    self.logger
                        .line("既存のリーフを親として再利用（DB 書き込みなし）");
                } else {
                    self.logger.line("既存のコンテナを再利用");
                }
                Some(row.id)
            }
            None => {
                if self.cli.dry_run {
                    self.counts.add_container();
                    None
                } else {
                    match self.database.as_deref_mut() {
                        Some(database) => match database.insert_container(
                            key,
                            &folder_name(path),
                            parent_id,
                            self.run_id.expect("通常実行では import_run がある"),
                            &now_iso(),
                        ) {
                            Ok(id) => {
                                self.counts.add_container();
                                Some(id)
                            }
                            Err(error) => {
                                self.logger.line(format!(
                                    "[失敗] コンテナを作成できない: {} ({error})",
                                    path.display()
                                ));
                                None
                            }
                        },
                        None => None,
                    }
                }
            }
        };
        self.logger.line("セグメント: 0");
        self.recurse(directories, gallery_id);
        self.logger.blank();
    }

    fn process_pages_fresh(&mut self, images: &[(String, PathBuf)]) -> Vec<PageState> {
        let mut pages = Vec::with_capacity(images.len());
        for (seq, (file_name, image_path)) in images.iter().enumerate() {
            let page = self.process_page_fresh(seq as i64, file_name, image_path);
            self.log_page(&page);
            pages.push(page);
        }
        pages
    }

    fn process_page_fresh(&mut self, seq: i64, file_name: &str, image_path: &Path) -> PageState {
        let scan = extract::scan(image_path);
        let mut page = PageState {
            file_name: file_name.to_string(),
            seq,
            width: None,
            height: None,
            source: SourceKind::Unknown,
            pos: None,
            neg: String::new(),
            prompt_hash: None,
            segment_index: None,
            txt_status: TxtStatus::None,
            record_count: scan.record_count,
            format: scan.format,
            error: None,
            metadata_less: false,
            note: None,
        };

        if let Some(error) = scan.read_error {
            page.source = SourceKind::Error;
            page.error = Some(format!("読み取り失敗: {error}"));
            return page;
        }
        let Some(format) = scan.format else {
            page.source = SourceKind::Error;
            page.error = Some("画像として読めない（形式不明）".to_string());
            return page;
        };
        if let Some((width, height)) = dimensions::read(image_path, format) {
            page.width = Some(width);
            page.height = Some(height);
        }

        if scan.record_count == 0 {
            page.source = SourceKind::None;
            page.metadata_less = true;
            return page;
        }

        let directory = image_path.parent().unwrap_or_else(|| Path::new("."));
        let stem = paths::file_stem(image_path).unwrap_or_else(|| file_name.to_string());
        let base = directory.join(format!("{stem}.txt"));
        let metadata = directory.join(format!("{stem}_metadata.txt"));
        let txt_outcome = match extract::render_text(image_path) {
            Ok(generated) => txt::apply(
                &generated,
                &base,
                &metadata,
                self.cli.overwrite,
                self.cli.dry_run,
            ),
            Err(error) => TxtOutcome {
                status: TxtStatus::Failed,
                note: Some(format!("txt を生成できない: {error}")),
            },
        };
        page.txt_status = txt_outcome.status;
        page.note = txt_outcome.note;

        let analysis = extract::analyze(&scan.records);
        apply_analysis(&mut page, analysis);
        page
    }

    fn process_page_txt_only(&mut self, seq: i64, file_name: &str, image_path: &Path) -> PageState {
        let mut page = PageState {
            file_name: file_name.to_string(),
            seq,
            width: None,
            height: None,
            source: SourceKind::Unknown,
            pos: None,
            neg: String::new(),
            prompt_hash: None,
            segment_index: None,
            txt_status: TxtStatus::None,
            record_count: 0,
            format: None,
            error: None,
            metadata_less: false,
            note: None,
        };
        let generated = match extract::render_text(image_path) {
            Ok(generated) => generated,
            Err(error) => {
                page.source = SourceKind::Error;
                page.error = Some(format!("読み取り失敗: {error}"));
                return page;
            }
        };
        let scan = extract::scan(image_path);
        page.format = scan.format;
        page.record_count = scan.record_count;
        if scan.read_error.is_some() || scan.format.is_none() {
            page.source = SourceKind::Error;
            page.error = Some("画像として読めない（形式不明）".to_string());
            return page;
        }
        if scan.record_count == 0 {
            page.source = SourceKind::None;
            page.metadata_less = true;
            return page;
        }
        let directory = image_path.parent().unwrap_or_else(|| Path::new("."));
        let stem = paths::file_stem(image_path).unwrap_or_else(|| file_name.to_string());
        let base = directory.join(format!("{stem}.txt"));
        let metadata = directory.join(format!("{stem}_metadata.txt"));
        let outcome = txt::apply(
            &generated,
            &base,
            &metadata,
            self.cli.overwrite,
            self.cli.dry_run,
        );
        page.txt_status = outcome.status;
        page.note = outcome.note;
        page
    }

    fn log_page(&mut self, page: &PageState) {
        let note = page
            .note
            .as_ref()
            .map(|note| format!(" note={note}"))
            .unwrap_or_default();
        self.logger.line(format!(
            "  ページ seq={:04} file={} status={} records={} format={} source={}{}",
            page.seq,
            page.file_name,
            page.txt_status.as_str(),
            page.record_count,
            format_label(page.format),
            page.source.as_str(),
            note
        ));
    }

    fn log_segment_composition(
        &mut self,
        plan: &LeafPlan,
        imported: Option<&crate::import::ImportedLeaf>,
    ) {
        if self.cli.dry_run {
            self.logger
                .line(format!("セグメント（予測）: {}", plan.segments.len()));
        } else {
            self.logger
                .line(format!("セグメント: {}", plan.segments.len()));
        }
        for (index, segment) in plan.segments.iter().enumerate() {
            let line_id = imported
                .and_then(|imported| imported.segments.get(index))
                .map(|row| row.line_id.to_string())
                .unwrap_or_else(|| "-".to_string());
            self.logger.line(format!(
                "  seq={} run_length={} repeat={} line={} prompt={}",
                segment.seq,
                segment.run_length,
                segment.run_length,
                line_id,
                shorten(&segment.pos, 80)
            ));
        }
    }

    fn log_unextracted(&mut self, pages: &[PageState], stats: Option<&GalleryStats>) {
        self.logger.line("未抽出ページ:");
        if let Some(stats) = stats {
            if stats.unextracted.is_empty() {
                self.logger.line("  なし");
            } else {
                for (seq, file_name, source) in &stats.unextracted {
                    let reason = match source.as_str() {
                        "none" => "メタデータなし",
                        "error" => "読めない",
                        _ => "pos 未抽出",
                    };
                    self.logger
                        .line(format!("  seq={seq:04} file={file_name} reason={reason}"));
                }
            }
            return;
        }
        if pages.iter().all(|page| page.prompt_hash.is_some()) {
            self.logger.line("  なし");
            return;
        }
        for page in pages.iter().filter(|page| page.prompt_hash.is_none()) {
            let reason = if page.metadata_less {
                "メタデータなし".to_string()
            } else if let Some(error) = &page.error {
                error.clone()
            } else {
                "pos 未抽出".to_string()
            };
            self.logger.line(format!(
                "  seq={:04} file={} reason={}",
                page.seq, page.file_name, reason
            ));
        }
    }

    fn log_ignored(&mut self, directories: &[(String, PathBuf)]) {
        let names: Vec<&str> = directories.iter().map(|(name, _)| name.as_str()).collect();
        self.logger.line(format!(
            "無視したサブフォルダ: {} ({})",
            directories.len(),
            names.join(", ")
        ));
    }

    fn recurse(&mut self, directories: &[(String, PathBuf)], parent_id: Option<i64>) {
        for (_, child) in directories {
            let key = match paths::normalize_key(child) {
                Ok(key) => key,
                Err(error) => {
                    self.logger.line(format!(
                        "[失敗] 子フォルダを正規化できない: {} ({error})",
                        child.display()
                    ));
                    continue;
                }
            };
            self.process_folder(child, key, parent_id);
        }
    }

    fn finish(&mut self) -> i32 {
        if let (Some(database), Some(run_id)) = (self.database.as_deref_mut(), self.run_id) {
            let finished_at = now_iso();
            if let Err(error) = database.finish_run(run_id, &finished_at, self.counts) {
                self.logger.line(format!("[異常] {error}"));
                self.logger.blank();
                self.logger.line("=== 終了 ===");
                self.logger.line("終了コード: 1");
                return 1;
            }
            match database.run_totals(run_id) {
                Ok(totals) => self.log_totals(
                    "import_runs",
                    totals.gallery_count as u64,
                    totals.page_count as u64,
                    totals.segment_count as u64,
                    totals.unextracted_count as u64,
                ),
                Err(error) => {
                    self.logger.line(format!("[異常] {error}"));
                    self.logger.blank();
                    self.logger.line("=== 終了 ===");
                    self.logger.line("終了コード: 1");
                    return 1;
                }
            }
        } else {
            self.log_totals(
                "予測",
                self.counts.gallery_count,
                self.counts.page_count,
                self.counts.segment_count,
                self.counts.unextracted_count,
            );
        }
        self.logger.blank();
        self.logger.line("=== 終了 ===");
        self.logger.line("終了コード: 0");
        0
    }

    fn log_totals(
        &mut self,
        label: &str,
        gallery_count: u64,
        page_count: u64,
        segment_count: u64,
        unextracted_count: u64,
    ) {
        self.logger.blank();
        self.logger.line("=== 合計 ===");
        self.logger.line(format!("集計元: {label}"));
        self.logger.line(format!("gallery_count: {gallery_count}"));
        self.logger.line(format!("page_count: {page_count}"));
        self.logger.line(format!("segment_count: {segment_count}"));
        self.logger
            .line(format!("unextracted_count: {unextracted_count}"));
    }
}

fn apply_analysis(page: &mut PageState, analysis: Analysis) {
    page.source = analysis.source;
    if let Some(pos) = analysis.pos {
        let neg = analysis.neg.unwrap_or_default();
        page.prompt_hash = Some(hash::pair_hash(&pos, &neg));
        page.pos = Some(pos);
        page.neg = neg;
    }
}

fn build_segments(pages: &mut [PageState]) -> Vec<SegmentPlan> {
    let mut segments: Vec<SegmentPlan> = Vec::new();
    let mut previous: Option<(String, String)> = None;
    for page in pages.iter_mut() {
        let Some(pos) = page.pos.clone() else {
            previous = None;
            continue;
        };
        let neg = page.neg.clone();
        let continues = previous
            .as_ref()
            .is_some_and(|(last_pos, last_neg)| last_pos == &pos && last_neg == &neg);
        if continues {
            if let Some(segment) = segments.last_mut() {
                segment.run_length += 1;
                page.segment_index = Some(segments.len() - 1);
            }
        } else {
            let index = segments.len();
            page.segment_index = Some(index);
            segments.push(SegmentPlan {
                seq: index as i64,
                run_length: 1,
                pos: pos.clone(),
                neg: neg.clone(),
                prompt_hash: page
                    .prompt_hash
                    .clone()
                    .expect("pos があるページにはハッシュがある"),
                width: page.width,
                height: page.height,
            });
        }
        previous = Some((pos, neg));
    }
    segments
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn format_label(format: Option<Format>) -> &'static str {
    match format {
        Some(Format::Png) => "png",
        Some(Format::Jpeg) => "jpeg",
        Some(Format::Webp) => "webp",
        None => "none",
    }
}

fn shorten(text: &str, limit: usize) -> String {
    let mut shortened: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        shortened.push('…');
    }
    shortened
}

fn now_iso() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    now.format(&Rfc3339).unwrap_or_else(|_| {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        )
    })
}

//! Phase 1 の実行本体: フォルダ引数 → ギャラリー境界 → ページ列挙 → txt → ログ。
//!
//! DB には一切触れない（§10 Phase 1）。segment 数・story_id・全体合計もログに出さない。

use crate::cli::Cli;
use crate::config::Settings;
use crate::logger::Logger;
use crate::natural;
use crate::paths;
use crate::txt::{self, TxtOutcome, TxtStatus};
use fast_meta::{extract_to_writer, Format, ScanOptions};
use std::fs;
use std::path::{Path, PathBuf};

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

    let mut runner = Runner {
        cli,
        logger,
        visited: std::collections::HashSet::new(),
    };
    for input in inputs {
        runner.process_folder(&input.path, input.key);
    }

    logger.blank();
    logger.line("=== 終了 ===");
    logger.line("終了コード: 0");
    0
}

fn log_conditions(cli: &Cli, settings: &Settings, logger: &mut Logger) {
    logger.line(format!("sdGalleryImporter {}", crate::VERSION));
    logger.line(format!("実行日時: {}", logger.started_at()));
    logger.line(format!(
        "モード: {}",
        if cli.dry_run { "dry-run" } else { "通常" }
    ));
    logger.line(format!(
        "DB: {} (由来: {} / Phase 1 では使用しない)",
        settings.db_path.display(),
        settings.db_path_source.label()
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
}

#[derive(Default)]
struct Counts {
    written: usize,
    overwritten: usize,
    skipped_exists: usize,
    fallback_metadata: usize,
    failed: usize,
    none: usize,
}

struct PageResult {
    seq: usize,
    file_name: String,
    status: TxtStatus,
    record_count: usize,
    format: Option<Format>,
    error: Option<String>,
    metadata_less: bool,
    note: Option<String>,
}

impl<'a> Runner<'a> {
    fn process_folder(&mut self, path: &Path, key: String) {
        if !self.visited.insert(key) {
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
            self.logger
                .line(format!("=== ギャラリー: {} ===", path.display()));
            self.logger.line("種別: コンテナ");
            self.logger.line("ページ数: 0");
            self.logger.line(format!("子候補: {}", directories.len()));
            self.logger.blank();
            self.recurse(&directories);
            return;
        }

        self.logger
            .line(format!("=== ギャラリー: {} ===", path.display()));
        self.logger.line("種別: リーフ");
        self.logger.line(format!("ページ数: {}", images.len()));

        let mut counts = Counts::default();
        let mut metadata_less: Vec<PageResult> = Vec::new();
        let mut unreadable: Vec<PageResult> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        for (seq, (file_name, image_path)) in images.iter().enumerate() {
            let result = self.process_page(seq, file_name, image_path);
            self.log_page(&result);
            match result.status {
                TxtStatus::Written => counts.written += 1,
                TxtStatus::Overwritten => counts.overwritten += 1,
                TxtStatus::SkippedExists => counts.skipped_exists += 1,
                TxtStatus::FallbackMetadata => counts.fallback_metadata += 1,
                TxtStatus::Failed => counts.failed += 1,
                TxtStatus::None => counts.none += 1,
            }
            if result.metadata_less {
                metadata_less.push(result);
            } else if result.error.is_some() {
                unreadable.push(result);
            } else if let Some(note) = &result.note {
                if result.status == TxtStatus::SkippedExists && note.contains("他者") {
                    warnings.push(format!("{}: {note}", result.file_name));
                }
            }
        }

        self.logger.line(format!(
            "集計: pages={} written={} overwritten={} skipped_exists={} fallback_metadata={} failed={} none={}",
            images.len(),
            counts.written,
            counts.overwritten,
            counts.skipped_exists,
            counts.fallback_metadata,
            counts.failed,
            counts.none
        ));
        self.logger
            .line(format!("メタデータなし: {}", metadata_less.len()));
        self.logger
            .line(format!("読めなかったページ: {}", unreadable.len()));
        self.logger.line("未抽出ページ:");
        if metadata_less.is_empty() && unreadable.is_empty() {
            self.logger.line("  なし");
        } else {
            for page in metadata_less.iter().chain(unreadable.iter()) {
                let reason = if page.metadata_less {
                    "メタデータなし".to_string()
                } else {
                    page.error.clone().unwrap_or_else(|| "読めない".to_string())
                };
                self.logger.line(format!(
                    "  seq={:04} file={} reason={}",
                    page.seq, page.file_name, reason
                ));
            }
        }
        for warning in warnings {
            self.logger.line(format!("警告: {warning}"));
        }

        if self.cli.recursive {
            self.recurse(&directories);
        } else if !directories.is_empty() {
            let names: Vec<&str> = directories.iter().map(|(name, _)| name.as_str()).collect();
            self.logger.line(format!(
                "無視したサブフォルダ: {} ({})",
                directories.len(),
                names.join(", ")
            ));
        }
        self.logger.blank();
    }

    fn process_page(&self, seq: usize, file_name: &str, image_path: &Path) -> PageResult {
        let mut output = Vec::new();
        match extract_to_writer(image_path, &ScanOptions::default(), &mut output) {
            Err(error) => PageResult {
                seq,
                file_name: file_name.to_string(),
                status: TxtStatus::None,
                record_count: 0,
                format: None,
                error: Some(format!("読み取り失敗: {error}")),
                metadata_less: false,
                note: None,
            },
            Ok(outcome) => match outcome.format {
                None => PageResult {
                    seq,
                    file_name: file_name.to_string(),
                    status: TxtStatus::None,
                    record_count: 0,
                    format: None,
                    error: Some("画像として読めない（形式不明）".to_string()),
                    metadata_less: false,
                    note: None,
                },
                Some(format) if outcome.record_count == 0 => PageResult {
                    seq,
                    file_name: file_name.to_string(),
                    status: TxtStatus::None,
                    record_count: 0,
                    format: Some(format),
                    error: None,
                    metadata_less: true,
                    note: None,
                },
                Some(format) => {
                    let directory = image_path.parent().unwrap_or_else(|| Path::new("."));
                    let stem =
                        paths::file_stem(image_path).unwrap_or_else(|| file_name.to_string());
                    let base = directory.join(format!("{stem}.txt"));
                    let metadata = directory.join(format!("{stem}_metadata.txt"));
                    let txt_outcome: TxtOutcome = txt::apply(
                        &output,
                        &base,
                        &metadata,
                        self.cli.overwrite,
                        self.cli.dry_run,
                    );
                    PageResult {
                        seq,
                        file_name: file_name.to_string(),
                        status: txt_outcome.status,
                        record_count: outcome.record_count,
                        format: Some(format),
                        error: None,
                        metadata_less: false,
                        note: txt_outcome.note,
                    }
                }
            },
        }
    }

    fn log_page(&mut self, page: &PageResult) {
        let format = format_label(page.format);
        let note = page
            .note
            .as_ref()
            .map(|note| format!(" note={note}"))
            .unwrap_or_default();
        self.logger.line(format!(
            "  ページ seq={:04} file={} status={} records={} format={}{}",
            page.seq,
            page.file_name,
            page.status.as_str(),
            page.record_count,
            format,
            note
        ));
    }

    fn recurse(&mut self, directories: &[(String, PathBuf)]) {
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
            self.process_folder(child, key);
        }
    }
}

fn format_label(format: Option<Format>) -> &'static str {
    match format {
        Some(Format::Png) => "png",
        Some(Format::Jpeg) => "jpeg",
        Some(Format::Webp) => "webp",
        None => "none",
    }
}

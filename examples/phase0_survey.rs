//! Phase 0: 実サンプルギャラリーの run 構成・メタデータ欠落を fast_meta ライブラリで実測する。
//!
//! 使い方:
//!   cargo run --release --example phase0_survey -- <ギャラリーディレクトリ>
//!
//! 出力は Phase 0 の記録用。ページ順は REQ-3.7 の自然順、run は pos+neg の完全一致で数える。

#[path = "../tests/common/mod.rs"]
mod common;

use fast_meta::{scan_file, Format, RecordSink, RecordView, ScanOptions, Source};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Collector {
    records: Vec<OwnedRecord>,
    error: bool,
    format: Option<Format>,
}

struct OwnedRecord {
    source: Source,
    keyword: Option<String>,
    body: String,
}

impl RecordSink for Collector {
    fn on_record(&mut self, record: &RecordView<'_>) {
        self.records.push(OwnedRecord {
            source: record.source,
            keyword: record.keyword.as_ref().map(ToString::to_string),
            body: record.body.to_string(),
        });
    }
}

fn main() {
    let Some(directory) = std::env::args().nth(1) else {
        eprintln!(
            "使い方: cargo run --release --example phase0_survey -- <ギャラリーディレクトリ>"
        );
        std::process::exit(1);
    };
    let directory = PathBuf::from(directory);
    let mut files = collect_images(&directory);
    files.sort_by(|left, right| natural_compare(file_name(left), file_name(right)));

    println!("gallery: {}", directory.display());
    println!("pages: {}", files.len());

    let mut counts = SourceCounts::default();
    let mut pages: Vec<Page> = Vec::new();

    for (seq, path) in files.iter().enumerate() {
        let mut collector = Collector::default();
        match scan_file(path, &ScanOptions::default(), &mut collector) {
            Ok(outcome) => {
                collector.format = outcome.format;
            }
            Err(_) => collector.error = true,
        }
        let page = classify(seq, path, &collector);
        counts.add(&page);
        pages.push(page);
    }

    println!(
        "sources: novelai={} comfyui={} a1111={} swarmui={} invokeai={} unknown={} none={} error={}",
        counts.novelai,
        counts.comfyui,
        counts.a1111,
        counts.swarmui,
        counts.invokeai,
        counts.unknown,
        counts.none,
        counts.error
    );

    let mut runs: Vec<Run> = Vec::new();
    for page in &pages {
        if let (Some(pos), Some(neg)) = (&page.pos, &page.neg) {
            let same = runs
                .last()
                .is_some_and(|run| &run.pos == pos && &run.neg == neg);
            if same {
                if let Some(run) = runs.last_mut() {
                    run.length += 1;
                }
            } else {
                runs.push(Run {
                    pos: pos.clone(),
                    neg: neg.clone(),
                    length: 1,
                });
            }
        }
    }
    let mut distribution = std::collections::BTreeMap::new();
    for run in &runs {
        *distribution.entry(run.length).or_insert(0usize) += 1;
    }
    println!(
        "segments: {} (distinct pos+neg: {})",
        runs.len(),
        runs.iter()
            .map(|run| (&run.pos, &run.neg))
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
    println!(
        "run_length distribution: {:?}",
        distribution.into_iter().collect::<Vec<_>>()
    );
    println!(
        "longest run: {}",
        runs.iter().map(|run| run.length).max().unwrap_or(0)
    );

    let no_metadata: Vec<usize> = pages
        .iter()
        .filter(|page| page.source == "none")
        .map(|page| page.seq)
        .collect();
    println!(
        "metadata-less pages: {} seq={:?}",
        no_metadata.len(),
        limit(&no_metadata)
    );

    let unextracted: Vec<usize> = pages
        .iter()
        .filter(|page| page.source != "none" && page.pos.is_none())
        .map(|page| page.seq)
        .collect();
    println!(
        "novelai/other records but no pos: {} seq={:?}",
        unextracted.len(),
        limit(&unextracted)
    );

    let breaks: Vec<usize> = pages
        .iter()
        .filter(|page| page.pos.is_none())
        .map(|page| page.seq)
        .collect();
    println!(
        "pages that break runs (no pos): {} seq={:?}",
        breaks.len(),
        limit(&breaks)
    );

    println!("first 20 pages:");
    for page in pages.iter().take(20) {
        println!(
            "  seq={:03} source={} pos={} neg={} file={}",
            page.seq,
            page.source,
            page.pos
                .as_deref()
                .map(shorten)
                .unwrap_or_else(|| "-".to_string()),
            page.neg
                .as_deref()
                .map(shorten)
                .unwrap_or_else(|| "-".to_string()),
            file_name(&page.path)
        );
    }
}

struct Page {
    seq: usize,
    path: PathBuf,
    source: &'static str,
    pos: Option<String>,
    neg: Option<String>,
}

#[derive(Default)]
struct SourceCounts {
    novelai: usize,
    comfyui: usize,
    a1111: usize,
    swarmui: usize,
    invokeai: usize,
    unknown: usize,
    none: usize,
    error: usize,
}

impl SourceCounts {
    fn add(&mut self, page: &Page) {
        match page.source {
            "novelai" => self.novelai += 1,
            "comfyui" => self.comfyui += 1,
            "a1111" => self.a1111 += 1,
            "swarmui" => self.swarmui += 1,
            "invokeai" => self.invokeai += 1,
            "none" => self.none += 1,
            "error" => self.error += 1,
            _ => self.unknown += 1,
        }
    }
}

struct Run {
    pos: String,
    neg: String,
    length: usize,
}

fn classify(seq: usize, path: &Path, collector: &Collector) -> Page {
    let mut page = Page {
        seq,
        path: path.to_path_buf(),
        source: "unknown",
        pos: None,
        neg: None,
    };
    if collector.error || collector.format.is_none() {
        page.source = "error";
        return page;
    }
    if collector.records.is_empty() {
        page.source = "none";
        return page;
    }

    let description = collector
        .records
        .iter()
        .find(|record| {
            record.source == Source::PngText && record.keyword.as_deref() == Some("Description")
        })
        .map(|record| record.body.clone());

    let novelai = collector
        .records
        .iter()
        .filter(|record| {
            record.source == Source::PngText && record.keyword.as_deref() == Some("Comment")
        })
        .find_map(|record| {
            let value: serde_json::Value = serde_json::from_str(&record.body).ok()?;
            let object = value.as_object()?;
            if object.contains_key("prompt") || object.contains_key("uc") {
                Some(object.clone())
            } else {
                None
            }
        });

    if let Some(comment) = novelai {
        page.source = "novelai";
        let pos = description
            .filter(|text| !text.is_empty())
            .or_else(|| {
                comment
                    .get("prompt")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
            .filter(|text| !text.is_empty());
        page.pos = pos;
        page.neg = Some(
            comment
                .get("uc")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string(),
        );
        return page;
    }

    let has_parameters = collector.records.iter().any(|record| {
        record.keyword.as_deref() == Some("parameters")
            || (record.source == Source::JpegComment
                && (record.body.contains("Negative prompt:") || record.body.contains("Steps:")))
    });
    let has_prompt = collector
        .records
        .iter()
        .any(|record| record.keyword.as_deref() == Some("prompt"));
    let has_swarm = collector.records.iter().any(|record| {
        record.keyword.as_deref() == Some("sui_image_params")
            || record.body.contains("\"sui_image_params\"")
    });
    let has_invokeai = collector.records.iter().any(|record| {
        matches!(
            record.keyword.as_deref(),
            Some("invokeai_metadata" | "sd-metadata")
        )
    });
    page.source = if has_prompt {
        "comfyui"
    } else if has_parameters {
        "a1111"
    } else if has_swarm {
        "swarmui"
    } else if has_invokeai {
        "invokeai"
    } else {
        "unknown"
    };
    page
}

fn collect_images(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .expect("ディレクトリを読める")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_image(path))
        .collect()
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp")
    )
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

fn limit(values: &[usize]) -> Vec<usize> {
    values.iter().copied().take(20).collect()
}

fn shorten(text: &str) -> String {
    let mut shortened: String = text.chars().take(24).collect();
    if text.chars().count() > 24 {
        shortened.push('…');
    }
    shortened
}

// --- REQ-3.7 の自然順（Phase 0 の実測用に同じ規則を実装） ---

#[derive(Debug, PartialEq)]
enum Chunk<'a> {
    Digits(&'a str),
    Text(&'a str),
}

fn chunks(name: &str) -> Vec<Chunk<'_>> {
    let bytes = name.as_bytes();
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut current_digits = None;
    for (index, byte) in bytes.iter().enumerate() {
        let is_digit = byte.is_ascii_digit();
        match current_digits {
            None => current_digits = Some(is_digit),
            Some(previous) if previous != is_digit => {
                let chunk = &name[start..index];
                result.push(if previous {
                    Chunk::Digits(chunk)
                } else {
                    Chunk::Text(chunk)
                });
                start = index;
                current_digits = Some(is_digit);
            }
            _ => {}
        }
    }
    if let Some(digits) = current_digits {
        let chunk = &name[start..];
        result.push(if digits {
            Chunk::Digits(chunk)
        } else {
            Chunk::Text(chunk)
        });
    }
    result
}

fn natural_compare(left: &str, right: &str) -> Ordering {
    let left_chunks = chunks(left);
    let right_chunks = chunks(right);
    for (a, b) in left_chunks.iter().zip(right_chunks.iter()) {
        let ordering = match (a, b) {
            (Chunk::Digits(x), Chunk::Digits(y)) => compare_digits(x, y),
            (Chunk::Text(x), Chunk::Text(y)) => x.cmp(y),
            (Chunk::Digits(_), Chunk::Text(_)) => Ordering::Less,
            (Chunk::Text(_), Chunk::Digits(_)) => Ordering::Greater,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    if left_chunks.len() != right_chunks.len() {
        return left.cmp(right);
    }
    left.cmp(right)
}

fn compare_digits(left: &str, right: &str) -> Ordering {
    let left_trimmed = left.trim_start_matches('0');
    let right_trimmed = right.trim_start_matches('0');
    match left_trimmed.len().cmp(&right_trimmed.len()) {
        Ordering::Equal => {}
        other => return other,
    }
    match left_trimmed.cmp(right_trimmed) {
        Ordering::Equal => left.len().cmp(&right.len()),
        other => other,
    }
}

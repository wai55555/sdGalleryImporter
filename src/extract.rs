//! 画像のメタデータ走査と pos/neg 抽出。
//!
//! Phase 2 では NovelAI のみを抽出する。他形式の判定・抽出は Phase 3/4 で追加する。

use fast_meta::{
    extract_to_writer, scan_file, Format, RecordSink, RecordView, ScanOptions, Source,
};

#[derive(Debug, Clone)]
pub struct RecordData {
    pub source: Source,
    pub keyword: Option<String>,
    pub body: String,
}

#[derive(Debug, Default)]
pub struct ScanData {
    pub format: Option<Format>,
    pub record_count: usize,
    pub records: Vec<RecordData>,
    pub read_error: Option<String>,
}

#[derive(Default)]
struct Collector {
    records: Vec<RecordData>,
}

impl RecordSink for Collector {
    fn on_record(&mut self, record: &RecordView<'_>) {
        self.records.push(RecordData {
            source: record.source,
            keyword: record.keyword.as_ref().map(ToString::to_string),
            body: record.body.to_string(),
        });
    }
}

pub fn scan(path: &std::path::Path) -> ScanData {
    let mut collector = Collector::default();
    match scan_file(path, &ScanOptions::default(), &mut collector) {
        Ok(outcome) => ScanData {
            format: outcome.format,
            record_count: outcome.record_count,
            records: collector.records,
            read_error: None,
        },
        Err(error) => ScanData {
            read_error: Some(error.to_string()),
            ..ScanData::default()
        },
    }
}

/// CLI と同一の txt 本文（REQ-5.2）。
pub fn render_text(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    extract_to_writer(path, &ScanOptions::default(), &mut output)
        .map(|_| output)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Novelai,
    Comfyui,
    A1111,
    Swarmui,
    Invokeai,
    Unknown,
    None,
    Error,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Novelai => "novelai",
            Self::Comfyui => "comfyui",
            Self::A1111 => "a1111",
            Self::Swarmui => "swarmui",
            Self::Invokeai => "invokeai",
            Self::Unknown => "unknown",
            Self::None => "none",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub source: SourceKind,
    pub pos: Option<String>,
    pub neg: Option<String>,
}

/// レコードから pos/neg と生成元を決める（Phase 2: NovelAI のみ）。
pub fn analyze(records: &[RecordData]) -> Analysis {
    let description = records
        .iter()
        .find(|record| {
            record.source == Source::PngText && record.keyword.as_deref() == Some("Description")
        })
        .map(|record| record.body.as_str())
        .filter(|body| !body.is_empty())
        .map(ToString::to_string);

    let comment = records
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

    if let Some(comment) = comment {
        let pos = description
            .or_else(|| {
                comment
                    .get("prompt")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
            .filter(|text| !text.is_empty());
        let neg = comment
            .get("uc")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        return Analysis {
            source: SourceKind::Novelai,
            pos,
            neg: Some(neg),
        };
    }

    Analysis {
        source: SourceKind::Unknown,
        pos: None,
        neg: None,
    }
}

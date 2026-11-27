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

/// レコードから pos/neg と生成元を決める（§6.1 / REQ-6.7）。
pub fn analyze(records: &[RecordData]) -> Analysis {
    let mut description: Option<String> = None;
    let mut novelai: Option<(Option<String>, String)> = None;
    let mut comfy: Vec<String> = Vec::new();
    let mut a1111: Vec<String> = Vec::new();
    let mut swarm: Vec<String> = Vec::new();
    let mut invoke_modern: Vec<String> = Vec::new();
    let mut invoke_old: Vec<String> = Vec::new();
    let mut detected = Detection::default();

    for record in records {
        if is_xmp(record.source) {
            continue; // XMP 由来のレコードは振り分け対象外（REQ-6.7）
        }
        let keyword = record.keyword.as_deref();
        match keyword {
            Some("Description") if record.source == Source::PngText => {
                if description.is_none() && !record.body.is_empty() {
                    description = Some(record.body.clone());
                }
            }
            Some("Comment") if record.source == Source::PngText => {
                if novelai.is_none() {
                    if let Some(pair) = novelai_pair(&record.body, &description) {
                        detected.novelai = true;
                        novelai = Some(pair);
                    }
                }
            }
            Some("prompt") if matches!(record.source, Source::PngText | Source::Trailing) => {
                detected.comfyui = true;
                comfy.push(record.body.clone());
            }
            Some("parameters") if matches!(record.source, Source::PngText | Source::Trailing) => {
                if let Some(params) = swarm_body_from_json(&record.body) {
                    detected.swarmui = true;
                    swarm.push(params);
                } else {
                    detected.a1111 = true;
                    a1111.push(record.body.clone());
                }
            }
            Some("sui_image_params") => {
                detected.swarmui = true;
                swarm.push(record.body.clone());
            }
            Some("invokeai_metadata") => {
                detected.invokeai = true;
                if parse_object(&record.body).is_some() {
                    invoke_modern.push(record.body.clone());
                }
            }
            Some("sd-metadata") => {
                detected.invokeai = true;
                if parse_object(&record.body).is_some() {
                    invoke_old.push(record.body.clone());
                }
            }
            _ => {}
        }

        match record.source {
            Source::JpegComment => {
                if looks_like_parameters(&record.body) {
                    detected.a1111 = true;
                    a1111.push(record.body.clone());
                }
            }
            Source::JpegExif | Source::PngExif | Source::WebpExif => match keyword {
                Some("UserComment") => {
                    if parse_object(&record.body).is_some() {
                        detected.comfyui = true;
                        comfy.push(record.body.clone());
                        if let Some(params) = swarm_body_from_json(&record.body) {
                            detected.swarmui = true;
                            swarm.push(params);
                        }
                    } else if looks_like_parameters(&record.body) {
                        detected.a1111 = true;
                        a1111.push(record.body.clone());
                    }
                }
                Some("Model") => {
                    if let Some(params) = swarm_body_from_json(&record.body) {
                        detected.swarmui = true;
                        swarm.push(params);
                    }
                }
                Some("ImageDescription") => {
                    if looks_like_parameters(&record.body) {
                        detected.a1111 = true;
                        a1111.push(record.body.clone());
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    if let Some((Some(pos), neg)) = novelai.as_ref() {
        return Analysis {
            source: SourceKind::Novelai,
            pos: Some(pos.clone()),
            neg: Some(neg.clone()),
        };
    }
    for body in &comfy {
        if let Some((Some(pos), neg)) = traverse_comfyui(body) {
            return Analysis {
                source: SourceKind::Comfyui,
                pos: Some(pos),
                neg: Some(neg),
            };
        }
    }
    for text in &a1111 {
        let split = split_a1111(text);
        if !split.pos.is_empty() {
            return Analysis {
                source: SourceKind::A1111,
                pos: Some(split.pos),
                neg: Some(split.neg),
            };
        }
    }
    for body in &swarm {
        if let Some((Some(pos), neg)) = extract_swarmui(body) {
            return Analysis {
                source: SourceKind::Swarmui,
                pos: Some(pos),
                neg: Some(neg),
            };
        }
    }
    for body in &invoke_modern {
        if let Some((Some(pos), neg)) = extract_invokeai_modern(body) {
            return Analysis {
                source: SourceKind::Invokeai,
                pos: Some(pos),
                neg: Some(neg),
            };
        }
    }
    for body in &invoke_old {
        if let Some((Some(pos), neg)) = extract_invokeai_old(body) {
            return Analysis {
                source: SourceKind::Invokeai,
                pos: Some(pos),
                neg: Some(neg),
            };
        }
    }

    if let Some(pos) = description {
        return Analysis {
            source: SourceKind::Unknown,
            pos: Some(pos),
            neg: Some(String::new()),
        };
    }

    Analysis {
        source: detected.top_priority(),
        pos: None,
        neg: None,
    }
}

#[derive(Default)]
struct Detection {
    novelai: bool,
    comfyui: bool,
    a1111: bool,
    swarmui: bool,
    invokeai: bool,
}

impl Detection {
    fn top_priority(&self) -> SourceKind {
        if self.novelai {
            SourceKind::Novelai
        } else if self.comfyui {
            SourceKind::Comfyui
        } else if self.a1111 {
            SourceKind::A1111
        } else if self.swarmui {
            SourceKind::Swarmui
        } else if self.invokeai {
            SourceKind::Invokeai
        } else {
            SourceKind::Unknown
        }
    }
}

fn is_xmp(source: Source) -> bool {
    matches!(
        source,
        Source::PngXmp | Source::JpegXmp | Source::WebpXmp | Source::TiffXmp
    )
}

fn parse_object(text: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()?
        .as_object()
        .cloned()
}

fn novelai_pair(body: &str, description: &Option<String>) -> Option<(Option<String>, String)> {
    let object = parse_object(body)?;
    if !object.contains_key("prompt") && !object.contains_key("uc") {
        return None;
    }
    let pos = description
        .clone()
        .or_else(|| {
            object
                .get("prompt")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .filter(|text| !text.is_empty());
    let neg = object
        .get("uc")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    Some((pos, neg))
}

/// SwarmUI の JSON からパラメータ本体の JSON 文字列を取り出す。
fn swarm_body_from_json(body: &str) -> Option<String> {
    let object = parse_object(body)?;
    if let Some(inner) = object
        .get("sui_image_params")
        .and_then(|value| value.as_object())
    {
        return serde_json::to_string(&serde_json::Value::Object(inner.clone())).ok();
    }
    if object.contains_key("negativeprompt")
        || object.contains_key("swarm_version")
        || object.contains_key("sui_extra_data")
    {
        return serde_json::to_string(&serde_json::Value::Object(object)).ok();
    }
    None
}

fn extract_swarmui(body: &str) -> Option<(Option<String>, String)> {
    let object = parse_object(body)?;
    let params = object
        .get("sui_image_params")
        .and_then(|value| value.as_object())
        .unwrap_or(&object);
    let pos = params
        .get("prompt")
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty())
        .map(ToString::to_string);
    let neg = params
        .get("negativeprompt")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    Some((pos, neg))
}

fn extract_invokeai_modern(body: &str) -> Option<(Option<String>, String)> {
    let object = parse_object(body)?;
    let pos = object
        .get("positive_prompt")
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty())
        .map(ToString::to_string);
    let neg = object
        .get("negative_prompt")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    Some((pos, neg))
}

fn extract_invokeai_old(body: &str) -> Option<(Option<String>, String)> {
    let object = parse_object(body)?;
    let image = object.get("image")?.as_object()?;
    let prompt = image.get("prompt")?;
    Some((invokeai_old_pos(prompt), String::new()))
}

fn invokeai_old_pos(prompt: &serde_json::Value) -> Option<String> {
    match prompt {
        serde_json::Value::String(text) if !text.is_empty() => Some(text.clone()),
        serde_json::Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                let Some(object) = item.as_object() else {
                    continue;
                };
                let Some(text) = object.get("prompt").and_then(|value| value.as_str()) else {
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                match object.get("weight") {
                    Some(serde_json::Value::Number(weight)) => {
                        parts.push(format!("({text}:{weight})"));
                    }
                    _ => parts.push(text.to_string()),
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(", "))
            }
        }
        _ => None,
    }
}

/// Phase 4 で実装する ComfyUI の辿り。Phase 3 では常に未抽出。
fn traverse_comfyui(_body: &str) -> Option<(Option<String>, String)> {
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A1111Split {
    pub pos: String,
    pub neg: String,
}

/// REQ-6.2 の分割規則。
pub fn split_a1111(text: &str) -> A1111Split {
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.last().is_some_and(|line| line.starts_with("Steps:")) {
        lines.pop();
    }

    let Some(index) = lines
        .iter()
        .position(|line| line.starts_with("Negative prompt:"))
    else {
        return A1111Split {
            pos: lines.join("\n"),
            neg: String::new(),
        };
    };

    let pos = lines[..index].join("\n");
    let first = lines[index]
        .strip_prefix("Negative prompt:")
        .unwrap_or(lines[index]);
    // A1111 は `Negative prompt: ` と空白付きで連結する。ラベルの区切りとして
    // 直後の空白 1 個だけを除き、それ以外の空白は保持する（REQ-6.2）。
    let first = first.strip_prefix(' ').unwrap_or(first);
    let mut neg = first.to_string();
    for line in &lines[index + 1..] {
        neg.push('\n');
        neg.push_str(line);
    }
    A1111Split { pos, neg }
}

/// `parameters` 形式の本文か（REQ-6.7 の振り分け用）。
pub fn looks_like_parameters(text: &str) -> bool {
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
        .iter()
        .any(|line| line.starts_with("Negative prompt:"))
        || lines.last().is_some_and(|line| line.starts_with("Steps:"))
}

#[cfg(test)]
mod tests {
    use super::SourceKind;
    use super::{analyze, looks_like_parameters, split_a1111, RecordData};
    use fast_meta::Source;

    fn record(source: Source, keyword: &str, body: &str) -> RecordData {
        RecordData {
            source,
            keyword: Some(keyword.to_string()),
            body: body.to_string(),
        }
    }

    #[test]
    fn a1111_splits_negative_and_steps() {
        let split = split_a1111("pos line\nNegative prompt: neg line\nSteps: 20, Sampler: Euler");
        assert_eq!(split.pos, "pos line");
        assert_eq!(split.neg, "neg line");
    }

    #[test]
    fn a1111_without_negative_keeps_all_as_pos() {
        let split = split_a1111("only pos\nsecond line");
        assert_eq!(split.pos, "only pos\nsecond line");
        assert_eq!(split.neg, "");
    }

    #[test]
    fn a1111_without_steps_keeps_negative_to_end() {
        let split = split_a1111("pos\nNegative prompt: neg\nsecond neg");
        assert_eq!(split.pos, "pos");
        assert_eq!(split.neg, "neg\nsecond neg");
    }

    #[test]
    fn a1111_uses_first_negative_line_and_strips_cr_and_trailing_blanks() {
        let split = split_a1111("pos\r\nNegative prompt: neg\r\nsecond\r\n\r\n");
        assert_eq!(split.pos, "pos");
        assert_eq!(split.neg, "neg\nsecond");
    }

    #[test]
    fn a1111_steps_line_is_removed_only_when_last() {
        let split = split_a1111("Steps: 20\nNegative prompt: neg\nSteps: 30");
        assert_eq!(split.pos, "Steps: 20");
        assert_eq!(split.neg, "neg");
    }

    #[test]
    fn a1111_empty_negative_remainder_keeps_newline() {
        let split = split_a1111("pos\nNegative prompt:\nline");
        assert_eq!(split.pos, "pos");
        assert_eq!(split.neg, "\nline");
    }

    #[test]
    fn parameters_detection_requires_negative_or_last_steps() {
        assert!(looks_like_parameters("pos\nNegative prompt: neg"));
        assert!(looks_like_parameters("pos\nSteps: 20"));
        assert!(!looks_like_parameters("postprocess upscale by: 3"));
        assert!(!looks_like_parameters("Steps: 20\njust text"));
    }

    #[test]
    fn xmp_user_comment_is_not_routed() {
        let records = vec![record(
            Source::PngXmp,
            "UserComment",
            "pos\nNegative prompt: neg\nSteps: 20",
        )];
        let analysis = analyze(&records);
        assert_eq!(analysis.source, SourceKind::Unknown);
        assert!(analysis.pos.is_none());
    }

    #[test]
    fn direct_sui_keyword_is_detected_even_without_prompt() {
        let records = vec![record(
            Source::PngText,
            "sui_image_params",
            r#"{"model":"x"}"#,
        )];
        let analysis = analyze(&records);
        assert_eq!(analysis.source, SourceKind::Swarmui);
        assert!(analysis.pos.is_none());
    }

    #[test]
    fn invokeai_old_array_joins_with_weight() {
        let records = vec![record(
            Source::PngText,
            "sd-metadata",
            r#"{"image":{"prompt":[{"prompt":"a","weight":1.0},{"prompt":"b","weight":0.8}]}}"#,
        )];
        let analysis = analyze(&records);
        assert_eq!(analysis.source, SourceKind::Invokeai);
        assert_eq!(analysis.pos.as_deref(), Some("(a:1.0), (b:0.8)"));
        assert_eq!(analysis.neg.as_deref(), Some(""));
    }

    #[test]
    fn invokeai_old_string_prompt_is_kept() {
        let records = vec![record(
            Source::PngText,
            "sd-metadata",
            r#"{"image":{"prompt":"plain prompt"}}"#,
        )];
        let analysis = analyze(&records);
        assert_eq!(analysis.pos.as_deref(), Some("plain prompt"));
    }
}

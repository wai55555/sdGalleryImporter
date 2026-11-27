//! txt サイドカーの命名・衝突判定（REQ-5.1〜REQ-5.5, REQ-5.7）。
//!
//! 「自ツール製かどうか」は、既存 txt の本文（見出し行を除く）が今回の出力本文と
//! バイト一致するかで判定する。見出し行にはパスが埋め込まれるため、比較から除く。

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxtStatus {
    Written,
    FallbackMetadata,
    SkippedExists,
    Overwritten,
    Failed,
    None,
}

impl TxtStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Written => "written",
            Self::FallbackMetadata => "fallback_metadata",
            Self::SkippedExists => "skipped_exists",
            Self::Overwritten => "overwritten",
            Self::Failed => "failed",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TxtOutcome {
    pub status: TxtStatus,
    pub note: Option<String>,
}

impl TxtOutcome {
    fn new(status: TxtStatus, note: Option<String>) -> Self {
        Self { status, note }
    }
}

/// 画像 `001.png` に対する `001.txt` / `001_metadata.txt` を処理する。
///
/// `generated` は fast_meta の CLI 同一出力（見出し行を含む）。
pub fn apply(
    generated: &[u8],
    base: &Path,
    metadata: &Path,
    overwrite: bool,
    dry_run: bool,
) -> TxtOutcome {
    if !base.exists() {
        return write(base, generated, dry_run, TxtStatus::Written);
    }

    let base_bytes = match fs::read(base) {
        Ok(bytes) => bytes,
        Err(error) => {
            return TxtOutcome::new(
                TxtStatus::Failed,
                Some(format!("既存 txt を読めない: {error}")),
            )
        }
    };

    if same_body(&base_bytes, generated) {
        if overwrite {
            return write(base, generated, dry_run, TxtStatus::Overwritten);
        }
        return TxtOutcome::new(TxtStatus::SkippedExists, None);
    }

    if !metadata.exists() {
        return write(metadata, generated, dry_run, TxtStatus::FallbackMetadata);
    }

    let metadata_bytes = match fs::read(metadata) {
        Ok(bytes) => bytes,
        Err(error) => {
            return TxtOutcome::new(
                TxtStatus::Failed,
                Some(format!("既存 _metadata.txt を読めない: {error}")),
            )
        }
    };
    if same_body(&metadata_bytes, generated) {
        if overwrite {
            return write(metadata, generated, dry_run, TxtStatus::Overwritten);
        }
        return TxtOutcome::new(TxtStatus::SkippedExists, None);
    }

    TxtOutcome::new(
        TxtStatus::SkippedExists,
        Some("既存 txt と _metadata.txt の両方が他者のため書き込まない".to_string()),
    )
}

fn write(path: &Path, generated: &[u8], dry_run: bool, status: TxtStatus) -> TxtOutcome {
    if dry_run {
        return TxtOutcome::new(status, Some("dry-run のため書き込みなし".to_string()));
    }
    match fs::write(path, generated) {
        Ok(()) => TxtOutcome::new(status, None),
        Err(error) => TxtOutcome::new(
            TxtStatus::Failed,
            Some(format!("txt を書き込めない ({}): {error}", path.display())),
        ),
    }
}

/// 既存 txt と今回の出力を、見出し行を除いた本文で比較する。
pub fn same_body(existing: &[u8], generated: &[u8]) -> bool {
    body(existing) == body(generated)
}

/// 先頭行が見出し行なら、その行を除いた本文を返す。
pub fn body(data: &[u8]) -> &[u8] {
    let Some(newline) = data.iter().position(|byte| *byte == b'\n') else {
        return if is_header_line(trim_cr(data)) {
            &[]
        } else {
            data
        };
    };
    if is_header_line(trim_cr(&data[..newline])) {
        &data[newline + 1..]
    } else {
        data
    }
}

/// `=== PNG File: <パス> ===` の形か。
pub fn is_header_line(line: &[u8]) -> bool {
    line.starts_with(b"=== ")
        && line.ends_with(b" ===")
        && line.windows(7).any(|window| window == b" File: ")
}

fn trim_cr(line: &[u8]) -> &[u8] {
    match line.last() {
        Some(b'\r') => &line[..line.len() - 1],
        _ => line,
    }
}

#[cfg(test)]
mod tests {
    use super::{body, is_header_line, same_body};

    #[test]
    fn detects_fast_meta_header_lines() {
        assert!(is_header_line(b"=== PNG File: C:\\a\\001.png ==="));
        assert!(is_header_line(b"=== JPEG File: C:\\a\\001.jpg ==="));
        assert!(is_header_line(b"=== WebP File: C:\\a\\001.webp ==="));
        assert!(!is_header_line(b"=== something else ==="));
        assert!(!is_header_line(b"--- PNG File: x ==="));
    }

    #[test]
    fn strips_header_before_comparing() {
        let a = b"=== PNG File: C:\\old\\001.png ===\n--- Software ---\nNovelAI\n";
        let b = b"=== PNG File: D:\\new\\001.png ===\n--- Software ---\nNovelAI\n";
        assert!(same_body(a, b));

        let c = b"--- Software ---\nNovelAI\n";
        assert!(same_body(a, c), "見出しの無い本文も同じ本文として扱う");
    }

    #[test]
    fn different_body_does_not_match() {
        let a = b"=== PNG File: x ===\n--- Software ---\nNovelAI\n";
        let b = b"=== PNG File: x ===\n--- Software ---\nOther\n";
        assert!(!same_body(a, b));
    }

    #[test]
    fn keeps_body_when_no_header() {
        assert_eq!(body(b"hello\nworld\n"), b"hello\nworld\n");
    }

    #[test]
    fn header_only_file_has_empty_body() {
        assert_eq!(body(b"=== PNG File: x ==="), b"");
        assert_eq!(body(b"=== PNG File: x ===\n"), b"");
        assert_eq!(body(b"=== PNG File: x ===\r\n"), b"");
    }
}

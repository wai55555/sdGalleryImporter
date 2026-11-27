//! パスの正規化（REQ-9.1）と画像拡張子の判定（REQ-3.8）。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 同じギャラリーを指す別表記を吸収するための正規化キー。
///
/// `canonicalize` → `\\?\` 除去 → 区切り `\` → 末尾区切り除去（ドライブルート除く）
/// → 小文字化、の順に適用する。
pub fn normalize_key(path: &Path) -> io::Result<String> {
    let canonical = fs::canonicalize(path)?;
    let raw = canonical.to_string_lossy();
    let without_prefix = match raw.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix("UNC\\") {
            Some(unc) => format!(r"\\{unc}"),
            None => rest.to_string(),
        },
        None => raw.into_owned(),
    };
    let mut key = without_prefix.replace('/', r"\");
    while key.len() > 3 && key.ends_with('\\') {
        key.pop();
    }
    Ok(key.to_lowercase())
}

pub fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp")
    )
}

/// txt サイドカーの stem。`001.png` → `001`、`a.b.png` → `a.b`。
pub fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// 引数のフォルダを絶対パス表記にする。失敗したらそのまま返す。
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::is_image_file;
    use std::path::Path;

    #[test]
    fn image_extensions_are_case_insensitive() {
        for name in ["a.png", "a.PNG", "a.Jpg", "a.jpeg", "a.WebP"] {
            assert!(is_image_file(Path::new(name)), "{name}");
        }
        for name in ["a.gif", "a.txt", "a.png.txt", "a"] {
            assert!(!is_image_file(Path::new(name)), "{name}");
        }
    }
}

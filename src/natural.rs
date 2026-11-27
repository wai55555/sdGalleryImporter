//! ファイル名の自然順（REQ-3.7）。
//!
//! 数字は ASCII 0-9 のみ。数字チャンクは先行ゼロを除いて長さ比較し、同長なら辞書順。
//! 数値として同値で表記だけ違う場合は、元の桁数が少ない方を先にする。

use std::cmp::Ordering;

#[derive(Debug, PartialEq)]
enum Chunk<'a> {
    Digits(&'a str),
    Text(&'a str),
}

pub fn compare(left: &str, right: &str) -> Ordering {
    let left_chunks = split(left);
    let right_chunks = split(right);
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
    // 全チャンクが同値、または一方が他方の接頭辞。最終規則はファイル名全体の
    // Unicode コードポイント順（UTF-8 のバイト順と一致する）。
    left.cmp(right)
}

fn split(name: &str) -> Vec<Chunk<'_>> {
    let bytes = name.as_bytes();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut digits: Option<bool> = None;
    for (index, byte) in bytes.iter().enumerate() {
        let is_digit = byte.is_ascii_digit();
        match digits {
            None => digits = Some(is_digit),
            Some(previous) if previous != is_digit => {
                chunks.push(chunk(&name[start..index], previous));
                start = index;
                digits = Some(is_digit);
            }
            _ => {}
        }
    }
    if let Some(previous) = digits {
        chunks.push(chunk(&name[start..], previous));
    }
    chunks
}

fn chunk(text: &str, digits: bool) -> Chunk<'_> {
    if digits {
        Chunk::Digits(text)
    } else {
        Chunk::Text(text)
    }
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

#[cfg(test)]
mod tests {
    use super::compare;
    use std::cmp::Ordering;

    #[test]
    fn numeric_chunks_sort_by_value_without_parsing() {
        let mut names = vec!["10.png", "2.png", "1.png", "01.png", "001.png"];
        names.sort_by(|a, b| compare(a, b));
        assert_eq!(names, vec!["1.png", "01.png", "001.png", "2.png", "10.png"]);
    }

    #[test]
    fn longer_digit_run_wins_even_beyond_u64() {
        let big = "99999999999999999999999999999999";
        let bigger = "100000000000000000000000000000000";
        assert_eq!(compare(big, bigger), Ordering::Less);
        assert_eq!(compare(bigger, big), Ordering::Greater);
    }

    #[test]
    fn digits_sort_before_text_at_same_position() {
        assert_eq!(compare("1a", "a1"), Ordering::Less);
        assert_eq!(compare("a1", "1a"), Ordering::Greater);
    }

    #[test]
    fn text_chunks_use_codepoint_order() {
        assert_eq!(compare("A.png", "a.png"), Ordering::Less);
        assert_eq!(compare("全角１.png", "全角2.png"), Ordering::Greater);
    }

    #[test]
    fn prefix_uses_whole_name_order() {
        assert_eq!(compare("a", "a1"), Ordering::Less);
        assert_eq!(compare("a1", "a"), Ordering::Greater);
    }

    #[test]
    fn equal_names_compare_equal() {
        assert_eq!(compare("001.png", "001.png"), Ordering::Equal);
    }
}

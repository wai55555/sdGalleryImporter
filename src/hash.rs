//! 行の同一性ハッシュ（REQ-4.1）。

use sha2::{Digest, Sha256};

/// `SHA-256(prompt || "\x00" || negative)` の小文字16進。
pub fn pair_hash(prompt: &str, negative: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    hasher.update([0u8]);
    hasher.update(negative.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::pair_hash;

    #[test]
    fn hash_separates_pos_and_neg() {
        assert_ne!(pair_hash("a", "b"), pair_hash("ab", ""));
        assert_ne!(pair_hash("a", ""), pair_hash("a", "b"));
    }

    #[test]
    fn hash_is_lowercase_hex_and_deterministic() {
        let hash = pair_hash("hello", "world");
        assert_eq!(hash.len(), 64);
        assert!(hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(hash, pair_hash("hello", "world"));
    }

    #[test]
    fn hash_uses_nul_separator() {
        use sha2::{Digest, Sha256};
        let expected = format!("{:x}", Sha256::digest(b"a\0b"));
        assert_eq!(pair_hash("a", "b"), expected);
    }
}

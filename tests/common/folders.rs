//! フォルダ構成・txt 衝突のフィクスチャ用ヘルパー。
//!
//! 実ファイルを伴うため、画像 Fixture（`common::*`）と組み合わせて使う。

use fast_meta::{extract_to_writer, ScanOptions};
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

/// fast_meta CLI と同じ経路で txt 本文を生成する（REQ-5.2）。
pub fn fastmeta_text(image: &Path) -> String {
    let mut output = Vec::new();
    extract_to_writer(image, &ScanOptions::default(), &mut output)
        .expect("fast_meta で txt を生成できる");
    String::from_utf8(output).expect("fast_meta の出力は UTF-8")
}

/// 見出し行（`=== ... File: <パス> ===`）を除いた本文。REQ-5.3 の比較対象。
pub fn body_without_header(text: &str) -> &str {
    match text.split_once('\n') {
        Some((_header, body)) => body,
        None => "",
    }
}

/// 自ツール製として判定されるサイドカー txt の中身（見出し行なし）。
pub fn own_sidecar_body(image: &Path) -> String {
    body_without_header(&fastmeta_text(image)).to_string()
}

/// 他者が置いた txt の例。REQ-5.3 の「内容が一致しない」ケース。
pub const FOREIGN_TXT: &str = "これは他者が置いたメモです。\n";

/// Windows のジャンクションを作る。`mklink /J` は管理者権限を要求しない。
pub fn create_junction(link: &Path, target: &Path) -> io::Result<()> {
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("mklink /J が失敗した: {status}")))
    }
}

/// ジャンクションが作れる環境かを確認する（作れない環境では該当テストをスキップ）。
pub fn junction_supported(sandbox: &super::Sandbox) -> bool {
    let target = sandbox.dir("junction-target");
    let link = sandbox.path("junction-link");
    if create_junction(&link, &target).is_ok() {
        let _ = fs::remove_dir(&link);
        true
    } else {
        false
    }
}

#![allow(dead_code)]
//! Phase 1 以降の統合テスト用ハーネス。
//!
//! ビルド済み exe をテストごとの一時フォルダへコピーして実行する。こうすると
//! exe 隣の設定ファイル・ログ・DB がテストごとに隔離され、D&D 起動と同じ条件になる。

use crate::common::Sandbox;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TestEnv {
    pub sandbox: Sandbox,
    pub exe_dir: PathBuf,
    exe: PathBuf,
}

pub struct RunResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub log_path: Option<PathBuf>,
    pub log: String,
}

impl TestEnv {
    pub fn new(label: &str) -> Self {
        let sandbox = Sandbox::new(label);
        let exe_dir = sandbox.dir("exe");
        let exe = exe_dir.join("sdGalleryImporter.exe");
        fs::copy(env!("CARGO_BIN_EXE_sdGalleryImporter"), &exe).expect("exe をコピーできる");
        Self {
            sandbox,
            exe_dir,
            exe,
        }
    }

    pub fn work(&self, relative: &str) -> PathBuf {
        // テスト内の "/" 区切りを Windows の区切りへ揃える。絶対パス表記の
        // ゆれでログの見出しと比較対象がずれるのを防ぐ。
        let mut path = self.sandbox.root().to_path_buf();
        for part in relative.split('/').filter(|part| !part.is_empty()) {
            path.push(part);
        }
        path
    }

    pub fn write_config(&self, text: &str) {
        fs::write(self.exe_dir.join("sdGalleryImporter.toml"), text).expect("設定を書ける");
    }

    pub fn run_strs(&self, args: &[&str]) -> RunResult {
        self.run(args.iter().map(OsString::from))
    }

    pub fn run<I>(&self, args: I) -> RunResult
    where
        I: IntoIterator<Item = OsString>,
    {
        let output = Command::new(&self.exe)
            .args(args)
            .output()
            .expect("sdGalleryImporter を実行できる");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let log_path = stdout
            .lines()
            .find_map(|line| line.strip_prefix("log: "))
            .map(PathBuf::from);
        let log = log_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        RunResult {
            code: output.status.code().unwrap_or(-1),
            stdout,
            stderr,
            log_path,
            log,
        }
    }
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// ディレクトリ以下のファイルを再帰的に集める（ジャンクションは辿らない）。
pub fn files_recursive(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(root, &mut files);
    files.sort();
    files
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect(&path, files);
        } else if metadata.is_file() {
            files.push(path);
        }
    }
}

pub fn log_files(directory: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<PathBuf> = fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("log"))
                .collect()
        })
        .unwrap_or_default();
    logs.sort();
    logs
}

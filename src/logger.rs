//! ログファイル（REQ-8.7 / REQ-8.8）。
//!
//! 実行ごとに exe と同じフォルダへ `import_YYYYMMDD_HHMMSS.log` を作る。
//! 同名があれば `_2`, `_3` … を付ける。exe フォルダに作れなければカレント
//! ディレクトリ、それも駄目ならコンソール出力だけになる。

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub struct Logger {
    lines: Vec<String>,
    path: Option<PathBuf>,
    now: OffsetDateTime,
}

impl Logger {
    pub fn create(exe_dir: &Path, dry_run: bool) -> Self {
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let prefix = if dry_run { "import_dryrun" } else { "import" };
        let stamp = format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        );

        let path = create_log_file(exe_dir, prefix, &stamp).or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| create_log_file(&cwd, prefix, &stamp))
        });

        Self {
            lines: Vec::new(),
            path,
            now,
        }
    }

    /// ログの冒頭に置く ISO 8601（ローカル時刻・オフセット付き）の日時。
    pub fn started_at(&self) -> String {
        self.now.format(&Rfc3339).unwrap_or_else(|_| {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                self.now.year(),
                u8::from(self.now.month()),
                self.now.day(),
                self.now.hour(),
                self.now.minute(),
                self.now.second()
            )
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn line(&mut self, text: impl AsRef<str>) {
        self.lines.push(text.as_ref().to_string());
    }

    pub fn blank(&mut self) {
        self.lines.push(String::new());
    }

    /// ログを書き出す。書けなかった場合はコンソールへ出す（REQ-8.7 のフォールバック）。
    pub fn finish(self) {
        let mut console_fallback = self.path.is_none();
        if let Some(path) = &self.path {
            match write_lines(path, &self.lines) {
                Ok(()) => return,
                Err(error) => {
                    console_fallback = true;
                    eprintln!(
                        "ログファイルに書き込めなかった ({}): {error}",
                        path.display()
                    );
                }
            }
        }
        if console_fallback {
            for line in &self.lines {
                println!("{line}");
            }
        }
    }
}

fn write_lines(path: &Path, lines: &[String]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    file.flush()
}

fn create_log_file(directory: &Path, prefix: &str, stamp: &str) -> Option<PathBuf> {
    for index in 0..1000usize {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!("_{}", index + 1)
        };
        let name = format!("{prefix}_{stamp}{suffix}.log");
        let path = directory.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Some(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// ログが作成できたか（テスト用）。
pub fn log_files(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension().and_then(|extension| extension.to_str()) == Some("log")
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

//! 設定ファイルと取り込み先 DB パスの解決（REQ-8.3 / REQ-8.4）。
//!
//! Phase 1 では DB を開かないが、ログの実行条件と引数エラーの整合のため、
//! ここでパスだけ確定しておく。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE_NAME: &str = "sdGalleryImporter.toml";
pub const DEFAULT_DB_FILE_NAME: &str = "story.db";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbPathSource {
    Cli,
    Config,
    Default,
}

impl DbPathSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cli => "--db",
            Self::Config => "設定ファイル",
            Self::Default => "既定",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub db_path: PathBuf,
    pub db_path_source: DbPathSource,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

pub fn resolve(cli_db: Option<&str>, exe_dir: &Path) -> Result<Settings, ConfigError> {
    let config_path = exe_dir.join(CONFIG_FILE_NAME);
    let config_db = match fs::read_to_string(&config_path) {
        Ok(text) => {
            let value: toml::Value = toml::from_str(&text).map_err(|error| {
                ConfigError(format!(
                    "設定ファイルを TOML として解析できない ({}): {error}",
                    config_path.display()
                ))
            })?;
            match value.get("db_path") {
                None => None,
                Some(toml::Value::String(path)) => Some(path.clone()),
                Some(_) => {
                    return Err(ConfigError(format!(
                        "設定ファイルの db_path が文字列ではない ({})",
                        config_path.display()
                    )))
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(ConfigError(format!(
                "設定ファイルを読めない ({}): {error}",
                config_path.display()
            )))
        }
    };

    let (db_path, db_path_source) = match cli_db {
        Some(path) => (resolve_against(exe_dir, path), DbPathSource::Cli),
        None => match config_db {
            Some(path) => (resolve_against(exe_dir, &path), DbPathSource::Config),
            None => (exe_dir.join(DEFAULT_DB_FILE_NAME), DbPathSource::Default),
        },
    };

    Ok(Settings {
        db_path,
        db_path_source,
        config_path,
    })
}

fn resolve_against(base: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve, DbPathSource};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("sdgi-config-{}-{id}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_config_uses_exe_relative_story_db() {
        let dir = TempDir::new();
        let settings = resolve(None, &dir.0).unwrap();
        assert_eq!(settings.db_path, dir.0.join("story.db"));
        assert_eq!(settings.db_path_source, DbPathSource::Default);
    }

    #[test]
    fn config_relative_path_resolves_against_exe_dir() {
        let dir = TempDir::new();
        fs::write(
            dir.0.join(super::CONFIG_FILE_NAME),
            "db_path = \"data\\\\gallery.db\"\n",
        )
        .unwrap();
        let settings = resolve(None, &dir.0).unwrap();
        assert_eq!(settings.db_path, dir.0.join("data").join("gallery.db"));
        assert_eq!(settings.db_path_source, DbPathSource::Config);
    }

    #[test]
    fn cli_overrides_config() {
        let dir = TempDir::new();
        fs::write(
            dir.0.join(super::CONFIG_FILE_NAME),
            "db_path = \"config.db\"\n",
        )
        .unwrap();
        let settings = resolve(Some("cli.db"), &dir.0).unwrap();
        assert_eq!(settings.db_path, dir.0.join("cli.db"));
        assert_eq!(settings.db_path_source, DbPathSource::Cli);
    }

    #[test]
    fn invalid_config_is_an_error_even_with_cli_db() {
        let dir = TempDir::new();
        fs::write(dir.0.join(super::CONFIG_FILE_NAME), "db_path = [1, 2]\n").unwrap();
        assert!(resolve(None, &dir.0).is_err());
        assert!(resolve(Some("cli.db"), &dir.0).is_err());
    }

    #[test]
    fn non_string_db_path_is_an_error() {
        let dir = TempDir::new();
        fs::write(dir.0.join(super::CONFIG_FILE_NAME), "db_path = 42\n").unwrap();
        assert!(resolve(None, &dir.0).is_err());
    }

    #[test]
    fn broken_toml_is_an_error() {
        let dir = TempDir::new();
        fs::write(
            dir.0.join(super::CONFIG_FILE_NAME),
            "db_path = \"unterminated\n",
        )
        .unwrap();
        assert!(resolve(None, &dir.0).is_err());
    }

    #[test]
    fn absolute_db_path_is_kept() {
        let dir = TempDir::new();
        let absolute = dir.0.join("absolute.db");
        let settings = resolve(Some(absolute.to_str().unwrap()), &dir.0).unwrap();
        assert_eq!(settings.db_path, absolute);
        let _ = Path::new(&settings.db_path);
    }
}

//! コマンドライン引数（REQ-8.5）。
//!
//! 受理するのは `--db <path>` / `--overwrite` / `--recursive` / `--dry-run` /
//! `--version` と、位置引数のフォルダだけ。それ以外は引数エラー。

#[derive(Debug, Default, Clone)]
pub struct Cli {
    pub db: Option<String>,
    pub overwrite: bool,
    pub recursive: bool,
    pub dry_run: bool,
    pub version: bool,
    pub folders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgError(pub String);

pub fn parse<I>(args: I) -> Result<Cli, ArgError>
where
    I: IntoIterator<Item = String>,
{
    let mut cli = Cli::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--overwrite" => cli.overwrite = true,
            "--recursive" => cli.recursive = true,
            "--dry-run" => cli.dry_run = true,
            "--version" => cli.version = true,
            "--db" => {
                if cli.db.is_some() {
                    return Err(ArgError("--db が重複して指定された".to_string()));
                }
                match iter.next() {
                    Some(value) if !value.is_empty() && !value.starts_with("--") => {
                        cli.db = Some(value)
                    }
                    _ => return Err(ArgError("--db の値が指定されていない".to_string())),
                }
            }
            other if other.starts_with('-') => {
                return Err(ArgError(format!("不明なオプション: {other}")));
            }
            other => cli.folders.push(other.to_string()),
        }
    }
    Ok(cli)
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn parse_str(args: &[&str]) -> Result<super::Cli, super::ArgError> {
        parse(args.iter().map(|value| value.to_string()))
    }

    #[test]
    fn parses_known_options_and_folders() {
        let cli = parse_str(&[
            "--overwrite",
            "--recursive",
            "--dry-run",
            "--db",
            "x.db",
            "C:\\a",
            "C:\\b",
        ])
        .unwrap();
        assert!(cli.overwrite && cli.recursive && cli.dry_run);
        assert_eq!(cli.db.as_deref(), Some("x.db"));
        assert_eq!(cli.folders, vec!["C:\\a", "C:\\b"]);
    }

    #[test]
    fn rejects_unknown_option() {
        assert!(parse_str(&["--help"]).is_err());
        assert!(parse_str(&["-x"]).is_err());
    }

    #[test]
    fn rejects_missing_or_duplicate_db() {
        assert!(parse_str(&["--db"]).is_err());
        assert!(parse_str(&["--db", "--overwrite"]).is_err());
        assert!(parse_str(&["--db", "a", "--db", "b"]).is_err());
    }

    #[test]
    fn version_is_a_flag() {
        assert!(parse_str(&["--version"]).unwrap().version);
    }
}

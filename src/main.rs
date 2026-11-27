use sd_gallery_importer::{cli, config, logger::Logger, runner, VERSION};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe_dir = exe_directory();

    let cli = match cli::parse(args) {
        Ok(cli) => cli,
        Err(error) => {
            let mut logger = Logger::create(&exe_dir, false);
            print_log_path(&logger);
            logger.line(format!("sdGalleryImporter {VERSION}"));
            logger.line(format!("実行日時: {}", logger.started_at()));
            logger.blank();
            logger.line(format!("[引数エラー] {}", error.0));
            logger.blank();
            logger.line("終了コード: 1");
            logger.finish();
            eprintln!("引数エラー: {}", error.0);
            return ExitCode::from(1);
        }
    };

    if cli.version {
        println!("sdGalleryImporter {VERSION}");
        return ExitCode::SUCCESS;
    }

    let mut logger = Logger::create(&exe_dir, cli.dry_run);
    print_log_path(&logger);

    let settings = match config::resolve(cli.db.as_deref(), &exe_dir) {
        Ok(settings) => settings,
        Err(error) => {
            logger.line(format!("sdGalleryImporter {VERSION}"));
            logger.line(format!("実行日時: {}", logger.started_at()));
            logger.blank();
            logger.line(format!("[異常] {}", error.0));
            logger.blank();
            logger.line("終了コード: 1");
            logger.finish();
            return ExitCode::from(1);
        }
    };

    let exit_code = runner::run(&cli, &settings, &mut logger);
    logger.finish();
    if exit_code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn exe_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn print_log_path(logger: &Logger) {
    if let Some(path) = logger.path() {
        println!("log: {}", path.display());
    }
}

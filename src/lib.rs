//! sdGalleryImporter のライブラリ本体。

pub mod cli;
pub mod config;
pub mod logger;
pub mod natural;
pub mod paths;
pub mod runner;
pub mod txt;

/// クレートのバージョン。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

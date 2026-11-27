//! Phase 0: fast_meta v2.1.0 をプロセス内で使った処理速度の実測。
//!
//! 使い方:
//!   cargo run --release --example phase0_bench -- <画像ディレクトリ>
//!   cargo run --release --example phase0_bench -- <出力ディレクトリ> --bulk 2000
//!
//! 計測は「extract_to_writer で txt 本文を生成する」経路。ウォームアップ 1 回のあと
//! 3 回計測し、中央値を出す。画像1枚ごとの子プロセス起動は行わない。

#[path = "../tests/common/mod.rs"]
mod common;

use common::*;
use fast_meta::{extract_to_writer, ScanOptions};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "使い方: cargo run --release --example phase0_bench -- <ディレクトリ> [--bulk <枚数>]"
        );
        std::process::exit(1);
    }
    let directory = PathBuf::from(&args[0]);
    let bulk = args
        .iter()
        .position(|arg| arg == "--bulk")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<usize>().ok());
    let bulk_copy = args
        .iter()
        .position(|arg| arg == "--bulk-copy")
        .and_then(|index| {
            let source = args.get(index + 1)?;
            let count = args.get(index + 2)?.parse::<usize>().ok()?;
            Some((PathBuf::from(source), count))
        });

    if let Some(count) = bulk {
        fs::create_dir_all(&directory).expect("出力ディレクトリを作成できる");
        write_bulk(&directory, count);
    }
    if let Some((source, count)) = bulk_copy {
        fs::create_dir_all(&directory).expect("出力ディレクトリを作成できる");
        copy_bulk(&source, &directory, count);
    }

    let files = collect_images(&directory);
    if files.is_empty() {
        eprintln!("画像が見つからない: {}", directory.display());
        std::process::exit(1);
    }
    let total_bytes: u64 = files
        .iter()
        .map(|path| fs::metadata(path).map(|meta| meta.len()).unwrap_or(0))
        .sum();

    println!("directory: {}", directory.display());
    println!("images: {}", files.len());
    println!("bytes: {}", total_bytes);
    println!("options: strict=false, stealth=auto");

    // ウォームアップ（結果は破棄）
    let (records, elapsed) = run_once(&files);
    println!(
        "warmup: {:.3}s ({:.0} img/s, {:.0} rec/s)",
        elapsed.as_secs_f64(),
        files.len() as f64 / elapsed.as_secs_f64(),
        records as f64 / elapsed.as_secs_f64()
    );

    let mut samples = Vec::new();
    for round in 1..=3 {
        let (records, elapsed) = run_once(&files);
        println!(
            "run {}: {:.3}s ({:.0} img/s, {:.0} rec/s, {} records)",
            round,
            elapsed.as_secs_f64(),
            files.len() as f64 / elapsed.as_secs_f64(),
            records as f64 / elapsed.as_secs_f64(),
            records
        );
        samples.push(elapsed);
    }
    samples.sort();
    let median = samples[1];
    let images_per_second = files.len() as f64 / median.as_secs_f64();
    let mib_per_second = (total_bytes as f64 / (1024.0 * 1024.0)) / median.as_secs_f64();
    println!(
        "median: {:.3}s ({:.1} img/s, {:.1} MiB/s)",
        median.as_secs_f64(),
        images_per_second,
        mib_per_second
    );
    println!(
        "extrapolation: 10000 images = {:.1}s, 30000 images = {:.1}s",
        10000.0 / images_per_second,
        30000.0 / images_per_second
    );
}

fn run_once(files: &[PathBuf]) -> (usize, Duration) {
    let options = ScanOptions::default();
    let mut sink = Vec::new();
    let mut records = 0usize;
    let started = Instant::now();
    for path in files {
        sink.clear();
        if let Ok(outcome) = extract_to_writer(path, &options, &mut sink) {
            records += outcome.record_count;
        }
    }
    (records, started.elapsed())
}

fn collect_images(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)
        .expect("ディレクトリを読める")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_image(path))
        .collect();
    files.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    files
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp")
    )
}

/// 実画像を巡回コピーして、実サイズの大量コーパスを作る。
fn copy_bulk(source: &Path, directory: &Path, count: usize) {
    let originals = collect_images(source);
    assert!(!originals.is_empty(), "コピー元に画像が無い");
    for index in 0..count {
        let original = &originals[index % originals.len()];
        let extension = original
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("png");
        let name = format!("{index:06}.{extension}");
        fs::copy(original, directory.join(name)).expect("実画像をコピーできる");
    }
}

fn write_bulk(directory: &Path, count: usize) {
    let samples: Vec<Vec<u8>> = vec![
        novelai_png(),
        a1111_png(A1111_PARAMETERS),
        comfyui_png(COMFYUI_PROMPT),
        swarmui_png(SWARMUI_PARAMS),
        no_metadata_png(),
    ];
    for index in 0..count {
        let bytes = &samples[index % samples.len()];
        let name = format!("{index:06}.png");
        fs::write(directory.join(name), bytes).expect("合成画像を書き込める");
    }
}

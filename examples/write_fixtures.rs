//! Phase 0 のフィクスチャを実ファイルとして書き出す開発用ツール。
//!
//! 使い方: `cargo run --example write_fixtures -- <出力ディレクトリ>`
//! テストはバイト列を直接生成するため、この example は目視確認・CLI 比較用。

#[path = "../tests/common/mod.rs"]
mod common;

use common::*;
use std::fs;
use std::path::PathBuf;

fn main() {
    let Some(output) = std::env::args().nth(1) else {
        eprintln!("使い方: cargo run --example write_fixtures -- <出力ディレクトリ>");
        std::process::exit(1);
    };
    let output = PathBuf::from(output);
    fs::create_dir_all(&output).expect("出力ディレクトリを作成できる");

    let files: Vec<(&str, Vec<u8>)> = vec![
        ("novelai.png", novelai_png()),
        (
            "description_only.png",
            description_only_png("a lone description"),
        ),
        ("a1111.png", a1111_png(A1111_PARAMETERS)),
        ("a1111.jpg", a1111_jpeg(A1111_PARAMETERS)),
        ("a1111_com.jpg", a1111_jpeg_comment(A1111_PARAMETERS)),
        ("comfyui.png", comfyui_png(COMFYUI_PROMPT)),
        ("comfyui.jpg", comfyui_jpeg(COMFYUI_PROMPT)),
        ("swarmui.png", swarmui_png(SWARMUI_PARAMS)),
        (
            "swarmui_comment.jpg",
            swarmui_jpeg_user_comment(SWARMUI_PARAMS),
        ),
        ("swarmui_model.jpg", swarmui_jpeg_model(SWARMUI_PARAMS)),
        ("invokeai.png", invokeai_png()),
        ("invokeai_sd_metadata.png", invokeai_sd_metadata_png()),
        (
            "png_exif_user_comment.png",
            png_exif_user_comment(r#"{"prompt":{"1":{"class_type":"KSampler","inputs":{}}}}"#),
        ),
        (
            "trailing_parameters.png",
            png_trailing_parameters(A1111_PARAMETERS),
        ),
        ("xmp_only.png", xmp_only_png()),
        ("stealth_only.png", stealth_only_png()),
        ("no_metadata.png", no_metadata_png()),
        ("no_metadata.jpg", no_metadata_jpeg()),
        (
            "webp_exif.webp",
            webp_with_exif_user_comment(r#"{"prompt":{"1":{}}}"#),
        ),
        ("broken.png", broken_image()),
        ("empty.png", empty_file()),
    ];

    for (name, bytes) in files {
        let path = output.join(name);
        fs::write(&path, bytes).expect("フィクスチャを書き込める");
        println!("{}", path.display());
    }
}

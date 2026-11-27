//! Phase 0: フィクスチャが fast_meta v2.1.0 で期待どおり読めることを確認する。
//!
//! ここで固定したバイト列・レコード構成が、以降の Phase のテストの土台になる。

mod common;

use common::db::{write_legacy_db, HashColumn};
use common::folders::{
    body_without_header, create_junction, fastmeta_text, junction_supported, own_sidecar_body,
    FOREIGN_TXT,
};
use common::*;
use fast_meta::{scan_file, Format, RecordSink, RecordView, ScanOptions, Source};
use std::path::Path;

#[derive(Debug, Clone)]
struct Record {
    source: Source,
    label: String,
    body: String,
    keyword: Option<String>,
}

#[derive(Default)]
struct Collector {
    records: Vec<Record>,
}

impl RecordSink for Collector {
    fn on_record(&mut self, record: &RecordView<'_>) {
        self.records.push(Record {
            source: record.source,
            label: record.label.to_string(),
            body: record.body.to_string(),
            keyword: record.keyword.as_ref().map(ToString::to_string),
        });
    }
}

fn scan(path: &Path) -> (bool, Option<Format>, Vec<Record>) {
    let mut collector = Collector::default();
    let result = scan_file(path, &ScanOptions::default(), &mut collector);
    match result {
        Ok(outcome) => (true, outcome.format, collector.records),
        Err(_) => (false, None, Vec::new()),
    }
}

fn scan_bytes(
    sandbox: &Sandbox,
    relative: &str,
    bytes: &[u8],
) -> (bool, Option<Format>, Vec<Record>) {
    let path = sandbox.write(relative, bytes);
    scan(&path)
}

fn find<'a>(records: &'a [Record], keyword: &str) -> Option<&'a Record> {
    records
        .iter()
        .find(|record| record.keyword.as_deref() == Some(keyword))
}

fn has_source(records: &[Record], source: Source) -> bool {
    records.iter().any(|record| record.source == source)
}

#[test]
fn novelai_fixture_carries_description_and_comment() {
    let sandbox = Sandbox::new("novelai");
    let (ok, format, records) = scan_bytes(&sandbox, "001.png", &novelai_png());
    assert!(ok);
    assert_eq!(format, Some(Format::Png));

    let description = find(&records, "Description").expect("Description レコード");
    assert_eq!(description.source, Source::PngText);
    assert_eq!(description.body, NOVELAI_DESCRIPTION);
    assert_eq!(description.label, "NovelAI [Prompt]");

    let comment = find(&records, "Comment").expect("Comment レコード");
    assert_eq!(comment.source, Source::PngText);
    assert_eq!(comment.label, "NovelAI [Settings]");
    // fast_meta は JSON 設定を整形して返す。実物のキーが残っていることを確認する。
    let parsed: serde_json::Value = serde_json::from_str(&comment.body).expect("Comment は JSON");
    assert!(parsed.get("prompt").is_some());
    assert!(parsed.get("uc").is_some());
    assert_eq!(parsed["prompt"], NOVELAI_DESCRIPTION);
}

#[test]
fn novelai_same_pos_different_neg_yields_different_comment() {
    let sandbox = Sandbox::new("novelai-neg");
    let (_, _, first) = scan_bytes(&sandbox, "a.png", &novelai_png_with("same pos", "neg A"));
    let (_, _, second) = scan_bytes(&sandbox, "b.png", &novelai_png_with("same pos", "neg B"));
    let first_comment = find(&first, "Comment").unwrap();
    let second_comment = find(&second, "Comment").unwrap();
    assert_ne!(first_comment.body, second_comment.body);
    let first_json: serde_json::Value = serde_json::from_str(&first_comment.body).unwrap();
    let second_json: serde_json::Value = serde_json::from_str(&second_comment.body).unwrap();
    assert_eq!(first_json["prompt"], second_json["prompt"]);
    assert_ne!(first_json["uc"], second_json["uc"]);
}

#[test]
fn novelai_pos_only_has_no_uc() {
    let sandbox = Sandbox::new("novelai-pos-only");
    let (_, _, records) = scan_bytes(&sandbox, "001.png", &novelai_pos_only_png("pos only"));
    let comment = find(&records, "Comment").unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&comment.body).unwrap();
    assert_eq!(parsed["prompt"], "pos only");
    assert!(parsed.get("uc").is_none());
}

#[test]
fn novelai_comment_without_prompt_is_not_novelai_candidate() {
    let sandbox = Sandbox::new("novelai-no-prompt");
    let (_, _, records) = scan_bytes(&sandbox, "001.png", &novelai_comment_without_prompt_png());
    let comment = find(&records, "Comment").expect("Comment レコードは出る");
    let parsed: serde_json::Value = serde_json::from_str(&comment.body).unwrap();
    assert!(parsed.get("prompt").is_none());
    assert!(parsed.get("uc").is_none());
}

#[test]
fn a1111_png_and_jpeg_routes_are_readable() {
    let sandbox = Sandbox::new("a1111");
    let (_, format, png_records) = scan_bytes(&sandbox, "001.png", &a1111_png(A1111_PARAMETERS));
    assert_eq!(format, Some(Format::Png));
    let png_parameters = find(&png_records, "parameters").expect("PNG parameters");
    assert_eq!(png_parameters.body, A1111_PARAMETERS);

    let (_, format, jpeg_records) = scan_bytes(&sandbox, "002.jpg", &a1111_jpeg(A1111_PARAMETERS));
    assert_eq!(format, Some(Format::Jpeg));
    let jpeg_comment = find(&jpeg_records, "UserComment").expect("UserComment");
    assert_eq!(jpeg_comment.source, Source::JpegExif);
    assert_eq!(jpeg_comment.body, A1111_PARAMETERS);

    let (_, _, com_records) =
        scan_bytes(&sandbox, "003.jpg", &a1111_jpeg_comment(A1111_PARAMETERS));
    let com = com_records
        .iter()
        .find(|record| record.source == Source::JpegComment)
        .expect("JPEG Comment");
    assert_eq!(com.body, A1111_PARAMETERS);
    assert_eq!(com.label, "Stable Diffusion (A1111)");
}

#[test]
fn comfyui_png_and_jpeg_wrapper_are_readable() {
    let sandbox = Sandbox::new("comfyui");
    let (_, _, png_records) = scan_bytes(&sandbox, "001.png", &comfyui_png(COMFYUI_PROMPT));
    let prompt = find(&png_records, "prompt").expect("prompt");
    assert_eq!(prompt.body, COMFYUI_PROMPT);
    assert!(find(&png_records, "workflow").is_none());

    let (_, _, jpeg_records) = scan_bytes(&sandbox, "002.jpg", &comfyui_jpeg(COMFYUI_PROMPT));
    let user_comment = find(&jpeg_records, "UserComment").expect("UserComment");
    let parsed: serde_json::Value = serde_json::from_str(&user_comment.body).unwrap();
    assert!(parsed.get("prompt").is_some());
    assert!(parsed.get("workflow").is_some());
}

#[test]
fn swarmui_routes_are_readable() {
    let sandbox = Sandbox::new("swarmui");
    // SwarmUI の PNG は parameters tEXt に sui_image_params を含む JSON を書く。
    let (_, _, png_records) = scan_bytes(&sandbox, "001.png", &swarmui_png(SWARMUI_PARAMS));
    let params = find(&png_records, "parameters").expect("parameters");
    assert_eq!(params.body, SWARMUI_PARAMS);
    let parsed: serde_json::Value = serde_json::from_str(&params.body).unwrap();
    assert!(parsed.get("sui_image_params").is_some());

    let (_, _, user_comment_records) = scan_bytes(
        &sandbox,
        "002.jpg",
        &swarmui_jpeg_user_comment(SWARMUI_PARAMS),
    );
    let user_comment = find(&user_comment_records, "UserComment").expect("UserComment");
    let parsed: serde_json::Value = serde_json::from_str(&user_comment.body).unwrap();
    assert!(parsed.get("sui_image_params").is_some());

    let (_, _, model_records) =
        scan_bytes(&sandbox, "003.jpg", &swarmui_jpeg_model(SWARMUI_PARAMS));
    let model = find(&model_records, "Model").expect("EXIF Model");
    assert_eq!(model.source, Source::JpegExif);
    let parsed: serde_json::Value = serde_json::from_str(&model.body).unwrap();
    assert!(parsed.get("sui_image_params").is_some());
}

#[test]
fn invokeai_modern_and_legacy_routes_are_readable() {
    let sandbox = Sandbox::new("invokeai");
    let (_, _, modern) = scan_bytes(&sandbox, "001.png", &invokeai_png());
    let metadata = find(&modern, "invokeai_metadata").expect("invokeai_metadata");
    assert_eq!(metadata.body, INVOKEAI_METADATA);
    let parsed: serde_json::Value = serde_json::from_str(&metadata.body).unwrap();
    assert_eq!(
        parsed["positive_prompt"],
        "Controlled example: three folded paper stars"
    );
    assert_eq!(parsed["negative_prompt"], "text");

    let (_, _, legacy) = scan_bytes(&sandbox, "002.png", &invokeai_sd_metadata_png());
    let sd_metadata = find(&legacy, "sd-metadata").expect("sd-metadata");
    let parsed: serde_json::Value = serde_json::from_str(&sd_metadata.body).unwrap();
    assert!(parsed.get("model").is_some());
    assert!(parsed.get("image").is_some());
    assert_eq!(
        parsed["image"]["prompt"][0]["prompt"],
        "an astronaut riding a horse"
    );
    assert!(parsed["image"].get("negative_prompt").is_none());
}

#[test]
fn unknown_source_routes_are_readable() {
    let sandbox = Sandbox::new("unknown");
    let (_, _, description) = scan_bytes(
        &sandbox,
        "001.png",
        &description_only_png("a lone description"),
    );
    assert!(!has_source(&description, Source::PngStealth));
    let record = find(&description, "Description").expect("Description");
    assert_eq!(record.body, "a lone description");

    let (_, _, xmp) = scan_bytes(&sandbox, "002.png", &xmp_only_png());
    assert!(has_source(&xmp, Source::PngXmp));

    let (_, _, stealth) = scan_bytes(&sandbox, "003.png", &stealth_only_png());
    assert!(has_source(&stealth, Source::PngStealth));
    assert!(!has_source(&stealth, Source::PngText));
}

#[test]
fn png_exif_user_comment_route_is_readable() {
    let sandbox = Sandbox::new("png-exif");
    let json = r#"{"prompt":{"1":{"class_type":"KSampler","inputs":{}}}}"#;
    let (_, _, records) = scan_bytes(&sandbox, "001.png", &png_exif_user_comment(json));
    let user_comment = find(&records, "UserComment").expect("UserComment");
    assert_eq!(user_comment.source, Source::PngExif);
    let parsed: serde_json::Value = serde_json::from_str(&user_comment.body).unwrap();
    assert!(parsed.get("prompt").is_some());
}

#[test]
fn trailing_parameters_route_is_readable() {
    let sandbox = Sandbox::new("trailing");
    let (_, _, records) = scan_bytes(
        &sandbox,
        "001.png",
        &png_trailing_parameters(A1111_PARAMETERS),
    );
    let trailing = records
        .iter()
        .find(|record| {
            record.source == Source::Trailing && record.keyword.as_deref() == Some("parameters")
        })
        .expect("Trailing parameters");
    assert_eq!(trailing.body, A1111_PARAMETERS);
}

#[test]
fn webp_exif_route_is_readable() {
    let sandbox = Sandbox::new("webp");
    let (_, format, records) = scan_bytes(
        &sandbox,
        "001.webp",
        &webp_with_exif_user_comment(r#"{"prompt":{"1":{}}}"#),
    );
    assert_eq!(format, Some(Format::Webp));
    let user_comment = find(&records, "UserComment").expect("UserComment");
    assert_eq!(user_comment.source, Source::WebpExif);
}

#[test]
fn non_image_files_are_detected_as_unreadable() {
    let sandbox = Sandbox::new("broken");
    let (_, format, records) = scan_bytes(&sandbox, "broken.png", &broken_image());
    assert_eq!(format, None);
    assert!(records.is_empty());

    let path = sandbox.write("empty.png", &empty_file());
    assert!(!scan(&path).0, "0 バイトは fast_meta が Err を返す");
}

#[test]
fn no_metadata_images_have_no_records() {
    let sandbox = Sandbox::new("no-metadata");
    let (_, format, records) = scan_bytes(&sandbox, "001.png", &no_metadata_png());
    assert_eq!(format, Some(Format::Png));
    assert!(records.is_empty());

    let (_, format, records) = scan_bytes(&sandbox, "002.jpg", &no_metadata_jpeg());
    assert_eq!(format, Some(Format::Jpeg));
    assert!(records.is_empty());
}

#[test]
fn txt_and_folder_fixture_helpers_work() {
    let sandbox = Sandbox::new("folders");
    let image = sandbox.write("gallery/001.png", &novelai_png());
    let text = fastmeta_text(&image);
    assert!(text.starts_with("=== PNG File: "), "見出し行から始まる");
    let own = own_sidecar_body(&image);
    assert_eq!(body_without_header(&text), own);
    assert!(!own.starts_with("=== "));
    assert_ne!(own, FOREIGN_TXT);

    if junction_supported(&sandbox) {
        let target = sandbox.dir("real-gallery");
        let link = sandbox.path("alias-gallery");
        create_junction(&link, &target).expect("ジャンクションを作れる");
        assert!(link.is_dir());
    } else {
        eprintln!("ジャンクション非対応のためスキップ");
    }
}

#[test]
fn legacy_db_fixtures_have_expected_rows_and_null_hashes() {
    let sandbox = Sandbox::new("legacy-db");
    let plain = sandbox.path("legacy.db");
    write_legacy_db(
        &plain,
        &[("legacy pos A", "neg A"), ("legacy pos B", "neg B")],
        HashColumn::Absent,
    )
    .unwrap();
    let connection = rusqlite::Connection::open(&plain).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM lines", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
    let columns: Vec<String> = connection
        .prepare("PRAGMA table_info(lines)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!columns.contains(&"prompt_hash".to_string()));

    let null_hash = sandbox.path("legacy_null_hash.db");
    write_legacy_db(
        &null_hash,
        &[("dup pos", "dup neg"), ("dup pos", "dup neg")],
        HashColumn::Null,
    )
    .unwrap();
    let connection = rusqlite::Connection::open(&null_hash).unwrap();
    let nulls: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM lines WHERE prompt_hash IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(nulls, 2, "重複行かつ prompt_hash が NULL のフィクスチャ");
}

#[test]
fn fixture_dimensions_are_encoded_in_headers() {
    // Phase 2 の寸法取得（REQ-6.5）が読む位置をここで固定しておく。
    let png = novelai_png();
    assert_eq!(&png[16..20], &64u32.to_be_bytes());
    assert_eq!(&png[20..24], &48u32.to_be_bytes());

    let jpeg = a1111_jpeg(A1111_PARAMETERS);
    // SOI, APP1(EXIF), SOF0 の順。SOF0 のデータ部先頭は precision, height, width。
    let sof = jpeg
        .windows(2)
        .position(|window| window == [0xff, 0xc0])
        .expect("SOF0");
    assert_eq!(&jpeg[sof + 5..sof + 7], &768u16.to_be_bytes());
    assert_eq!(&jpeg[sof + 7..sof + 9], &512u16.to_be_bytes());

    let webp = webp_with_exif_user_comment("x");
    assert_eq!(&webp[12..16], b"VP8X");
    let mut width = [0u8; 4];
    width[..3].copy_from_slice(&webp[24..27]);
    assert_eq!(u32::from_le_bytes(width) + 1, 512);
    let mut height = [0u8; 4];
    height[..3].copy_from_slice(&webp[27..30]);
    assert_eq!(u32::from_le_bytes(height) + 1, 512);
}

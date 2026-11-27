#![allow(dead_code)]
//! Phase 0 で確定したテスト用フィクスチャ一式。
//!
//! 画像はすべてこのモジュールがバイト列として生成する。実物サンプルから採取した
//! メタデータ本文だけを `tests/assets/` に置き、それを各形式のコンテナに埋め込む。
//! これにより、リポジトリに大きな画像をコミットせずに、実物のキー構成・本文を
//! そのまま検証できる。

pub mod db;
pub mod folders;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// 実物メタデータ（採取元は tests/assets/provenance.md）
// ---------------------------------------------------------------------------

pub const NOVELAI_DESCRIPTION: &str = include_str!("../assets/novelai/description.txt");
pub const NOVELAI_COMMENT: &str = include_str!("../assets/novelai/comment.json");
pub const A1111_PARAMETERS: &str = include_str!("../assets/a1111/parameters.txt");
pub const COMFYUI_PROMPT: &str = include_str!("../assets/comfyui/prompt.json");
pub const COMFYUI_WORKFLOW: &str = include_str!("../assets/comfyui/workflow.json");
pub const COMFYUI_PROMPT_MULTI_SAMPLER: &str =
    include_str!("../assets/comfyui/prompt_multi_sampler.json");
pub const SWARMUI_PARAMS: &str = include_str!("../assets/swarmui/image_params.json");
pub const INVOKEAI_METADATA: &str = include_str!("../assets/invokeai/metadata.json");
pub const INVOKEAI_SD_METADATA: &str = include_str!("../assets/invokeai/sd_metadata.json");

// ---------------------------------------------------------------------------
// 一時ディレクトリ
// ---------------------------------------------------------------------------

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// テストごとに独立した作業ディレクトリ。Drop で中身ごと削除する。
pub struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    pub fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sdgi-{}-{}-{}",
            std::process::id(),
            id,
            sanitize(label)
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("古いサンドボックスを削除できる");
        }
        fs::create_dir_all(&root).expect("サンドボックスを作成できる");
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn dir(&self, relative: &str) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(&path).expect("ディレクトリを作成できる");
        path
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("親ディレクトリを作成できる");
        }
        fs::write(&path, bytes).expect("ファイルを書き込める");
        path
    }

    pub fn write_str(&self, relative: &str, text: &str) -> PathBuf {
        self.write(relative, text.as_bytes())
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sanitize(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------

pub struct PngSpec {
    pub width: u32,
    pub height: u32,
    /// (keyword, text) を tEXt チャンクとして順に書く。
    pub text: Vec<(&'static str, String)>,
    /// eXIf チャンクの中身（TIFF ヘッダから始まる生バイト）。
    pub exif: Option<Vec<u8>>,
    /// iTXt の XML:com.adobe.xmp として書く XMP パケット。
    pub xmp: Option<String>,
    /// IEND の後に付加する生テキスト。
    pub trailing: Option<String>,
    /// Stealth PNG Info を alpha の LSB に埋め込む本文。
    pub stealth: Option<String>,
}

impl Default for PngSpec {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            text: Vec::new(),
            exif: None,
            xmp: None,
            trailing: None,
            stealth: None,
        }
    }
}

impl PngSpec {
    pub fn text(mut self, keyword: &'static str, text: impl Into<String>) -> Self {
        self.text.push((keyword, text.into()));
        self
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn exif(mut self, tiff: Vec<u8>) -> Self {
        self.exif = Some(tiff);
        self
    }

    pub fn xmp(mut self, packet: impl Into<String>) -> Self {
        self.xmp = Some(packet.into());
        self
    }

    pub fn trailing(mut self, text: impl Into<String>) -> Self {
        self.trailing = Some(text.into());
        self
    }

    pub fn stealth(mut self, payload: impl Into<String>) -> Self {
        self.stealth = Some(payload.into());
        self
    }
}

pub fn png(spec: &PngSpec) -> Vec<u8> {
    let width = spec.width;
    let height = spec.height;
    assert!(width > 0 && height > 0);

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    if let Some(payload) = &spec.stealth {
        embed_stealth(&mut pixels, payload);
    }

    let mut rows = Vec::with_capacity((height as usize) * ((width as usize) * 4 + 1));
    for y in 0..height as usize {
        rows.push(0u8); // filter type 0
        let start = y * (width as usize) * 4;
        rows.extend_from_slice(&pixels[start..start + (width as usize) * 4]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8bit RGBA, no interlace
    out.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    for (keyword, text) in &spec.text {
        let mut body = keyword.as_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&png_chunk(b"tEXt", &body));
    }
    if let Some(exif) = &spec.exif {
        out.extend_from_slice(&png_chunk(b"eXIf", exif));
    }
    if let Some(xmp) = &spec.xmp {
        let mut body = b"XML:com.adobe.xmp\0".to_vec();
        body.push(0); // compression flag
        body.push(0); // compression method
        body.push(0); // language tag (empty)
        body.push(0); // translated keyword (empty)
        body.extend_from_slice(xmp.as_bytes());
        out.extend_from_slice(&png_chunk(b"iTXt", &body));
    }
    out.extend_from_slice(&png_chunk(b"IDAT", &zlib_compress(&rows)));
    out.extend_from_slice(&png_chunk(b"IEND", &[]));
    if let Some(trailing) = &spec.trailing {
        out.extend_from_slice(trailing.as_bytes());
    }
    out
}

/// Stealth PNG Info のビット列を alpha の LSB に埋め込む。
/// fast_meta は 8bit RGBA の alpha LSB を MSB-first で読む。読み出しは
/// 列優先（x ごとに y を回す）なので、埋め込みも同じ順序に合わせる。
fn embed_stealth(pixels: &mut [u8], payload: &str) {
    let mut bits = Vec::new();
    push_bits(&mut bits, b"stealth_pnginfo");
    push_bits(
        &mut bits,
        &(u32::try_from(payload.len() * 8).expect("payload bit length fits u32")).to_be_bytes(),
    );
    push_bits(&mut bits, payload.as_bytes());
    let width = 64usize;
    let height = 64usize;
    let capacity = pixels.len() / 4;
    assert!(
        bits.len() <= capacity,
        "stealth payload が画像の容量を超えている: {} bits > {} pixels",
        bits.len(),
        capacity
    );
    for (index, bit) in bits.iter().enumerate() {
        let x = index / height;
        let y = index % height;
        pixels[(y * width + x) * 4 + 3] = *bit;
    }
}

fn push_bits(bits: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(data.len() + 4);
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// PNG の IDAT 用 zlib ストリーム。実画像に近いサイズ感にするため圧縮する。
fn zlib_compress(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("IDAT を圧縮できる");
    encoder.finish().expect("IDAT の圧縮を完了できる")
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// TIFF / EXIF
// ---------------------------------------------------------------------------

/// IFD0 に 1 枚だけ持つ最小の TIFF。タグは (tag, type, data) で指定する。
/// type 2 (ASCII) は末尾 NUL を付与し、type 7 (UNDEFINED) はそのまま格納する。
pub fn tiff(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    let entries_start = out.len();
    let mut data_offset = entries_start + entries.len() * 12 + 4;
    let mut data_area = Vec::new();
    let mut body = Vec::new();

    for (tag, field_type, raw) in entries {
        let value = if *field_type == 2 {
            let mut value = raw.clone();
            if value.last() != Some(&0) {
                value.push(0);
            }
            value
        } else {
            raw.clone()
        };
        body.extend_from_slice(&tag.to_le_bytes());
        body.extend_from_slice(&field_type.to_le_bytes());
        body.extend_from_slice(&(value.len() as u32).to_le_bytes());
        if value.len() <= 4 {
            let mut inline = value.clone();
            inline.resize(4, 0);
            body.extend_from_slice(&inline);
        } else {
            body.extend_from_slice(&(data_offset as u32).to_le_bytes());
            data_area.extend_from_slice(&value);
            data_offset += value.len();
        }
    }
    out.extend_from_slice(&body);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&data_area);
    out
}

pub fn exif_user_comment(text: &str) -> Vec<u8> {
    let mut data = b"ASCII\0\0\0".to_vec();
    data.extend_from_slice(text.as_bytes());
    data
}

// ---------------------------------------------------------------------------
// JPEG
// ---------------------------------------------------------------------------

pub struct JpegSpec {
    pub width: u16,
    pub height: u16,
    pub user_comment: Option<String>,
    pub comment: Option<String>,
    pub image_description: Option<String>,
    pub model: Option<String>,
    pub software: Option<String>,
}

impl Default for JpegSpec {
    fn default() -> Self {
        Self {
            width: 64,
            height: 48,
            user_comment: None,
            comment: None,
            image_description: None,
            model: None,
            software: None,
        }
    }
}

pub fn jpeg(spec: &JpegSpec) -> Vec<u8> {
    let mut out = vec![0xff, 0xd8];

    let mut entries: Vec<(u16, u16, Vec<u8>)> = Vec::new();
    if let Some(text) = &spec.image_description {
        entries.push((0x010e, 2, text.as_bytes().to_vec()));
    }
    if let Some(text) = &spec.software {
        entries.push((0x0131, 2, text.as_bytes().to_vec()));
    }
    if let Some(text) = &spec.model {
        entries.push((0x0110, 2, text.as_bytes().to_vec()));
    }
    if let Some(text) = &spec.user_comment {
        entries.push((0x9286, 7, exif_user_comment(text)));
    }
    if !entries.is_empty() {
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff(&entries));
        out.extend_from_slice(&jpeg_segment(0xe1, &app1));
    }

    let mut sof = Vec::new();
    sof.push(8); // precision
    sof.extend_from_slice(&spec.height.to_be_bytes());
    sof.extend_from_slice(&spec.width.to_be_bytes());
    sof.push(3); // components
    sof.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);
    out.extend_from_slice(&jpeg_segment(0xc0, &sof));

    if let Some(comment) = &spec.comment {
        out.extend_from_slice(&jpeg_segment(0xfe, comment.as_bytes()));
    }

    // SOS。fast_meta は SOS マーカーで走査を止めるため中身は最小限でよい。
    out.extend_from_slice(&[
        0xff, 0xda, 0x00, 0x0c, 0x01, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3f, 0x00,
    ]);
    out.extend_from_slice(&[0x00, 0xff, 0xd9]);
    out
}

fn jpeg_segment(marker: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![0xff, marker];
    out.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(data);
    out
}

// ---------------------------------------------------------------------------
// WebP
// ---------------------------------------------------------------------------

pub struct WebpSpec {
    pub width: u32,
    pub height: u32,
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<String>,
}

pub fn webp(spec: &WebpSpec) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();

    let mut vp8x = Vec::new();
    vp8x.push(0u8); // flags
    vp8x.extend_from_slice(&[0, 0, 0]); // reserved
    put_u24(&mut vp8x, spec.width - 1);
    put_u24(&mut vp8x, spec.height - 1);
    body.extend_from_slice(&webp_chunk(b"VP8X", &vp8x));

    if let Some(exif) = &spec.exif {
        body.extend_from_slice(&webp_chunk(b"EXIF", exif));
    }
    if let Some(xmp) = &spec.xmp {
        body.extend_from_slice(&webp_chunk(b"XMP ", xmp.as_bytes()));
    }

    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn put_u24(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes()[..3]);
}

fn webp_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

// ---------------------------------------------------------------------------
// 形式別の Fixture
// ---------------------------------------------------------------------------

/// NovelAI 正常系。Description / Comment は実物サンプルの本文をそのまま使う。
pub fn novelai_png() -> Vec<u8> {
    png(&PngSpec::default()
        .size(64, 48)
        .text("Description", NOVELAI_DESCRIPTION)
        .text("Comment", NOVELAI_COMMENT)
        .text("Software", "NovelAI")
        .text("Source", "NovelAI Diffusion V4.5"))
}

/// pos と neg を指定して NovelAI 形式を作る（Comment に prompt / uc を持つ）。
pub fn novelai_png_with(pos: &str, neg: &str) -> Vec<u8> {
    let comment = serde_json::json!({
        "prompt": pos,
        "uc": neg,
        "steps": 28,
        "sampler": "k_euler",
        "seed": 1234,
        "width": 64,
        "height": 48,
    })
    .to_string();
    png(&PngSpec::default()
        .size(64, 48)
        .text("Description", pos)
        .text("Comment", comment)
        .text("Software", "NovelAI"))
}

/// NovelAI で neg がない（uc キーなし）ケース。
pub fn novelai_pos_only_png(pos: &str) -> Vec<u8> {
    let comment = serde_json::json!({"prompt": pos}).to_string();
    png(&PngSpec::default()
        .size(64, 48)
        .text("Description", pos)
        .text("Comment", comment)
        .text("Software", "NovelAI"))
}

/// Description が無く、Comment の prompt だけを持つ NovelAI。
pub fn novelai_comment_only_png(pos: &str, neg: &str) -> Vec<u8> {
    let comment = serde_json::json!({"prompt": pos, "uc": neg}).to_string();
    png(&PngSpec::default()
        .size(64, 48)
        .text("Comment", comment)
        .text("Software", "NovelAI"))
}

/// Comment が JSON だが prompt / uc を持たない（NovelAI 判定不成立）。
pub fn novelai_comment_without_prompt_png() -> Vec<u8> {
    png(&PngSpec::default()
        .size(64, 48)
        .text("Comment", r#"{"steps": 28, "sampler": "k_euler"}"#)
        .text("Software", "NovelAI"))
}

/// Description だけを持つ画像（生成元不明・pos は Description）。
pub fn description_only_png(text: &str) -> Vec<u8> {
    png(&PngSpec::default().size(64, 48).text("Description", text))
}

/// A1111 の parameters を PNG テキストチャンクに持つ。
pub fn a1111_png(parameters: &str) -> Vec<u8> {
    png(&PngSpec::default()
        .size(512, 768)
        .text("parameters", parameters))
}

/// A1111 の parameters を JPEG の EXIF UserComment に持つ。
pub fn a1111_jpeg(parameters: &str) -> Vec<u8> {
    jpeg(&JpegSpec {
        width: 512,
        height: 768,
        user_comment: Some(parameters.to_string()),
        ..JpegSpec::default()
    })
}

/// A1111 の parameters を JPEG の COM セグメントに持つ。
pub fn a1111_jpeg_comment(parameters: &str) -> Vec<u8> {
    jpeg(&JpegSpec {
        width: 512,
        height: 768,
        comment: Some(parameters.to_string()),
        ..JpegSpec::default()
    })
}

/// ComfyUI の prompt グラフを PNG に持つ。
pub fn comfyui_png(prompt_graph: &str) -> Vec<u8> {
    png(&PngSpec::default()
        .size(832, 1216)
        .text("prompt", prompt_graph))
}

/// ComfyUI の prompt グラフを JPEG の UserComment にラッパ形式で持つ。
pub fn comfyui_jpeg(prompt_graph: &str) -> Vec<u8> {
    let wrapper = format!(r#"{{"prompt": {prompt_graph}, "workflow": {{}}}}"#);
    jpeg(&JpegSpec {
        width: 832,
        height: 1216,
        user_comment: Some(wrapper),
        ..JpegSpec::default()
    })
}

pub fn swarmui_png(params: &str) -> Vec<u8> {
    png(&PngSpec::default()
        .size(1024, 1024)
        .text("parameters", params))
}

pub fn swarmui_jpeg_user_comment(params: &str) -> Vec<u8> {
    jpeg(&JpegSpec {
        width: 1024,
        height: 1024,
        user_comment: Some(params.to_string()),
        ..JpegSpec::default()
    })
}

/// SwarmUI の旧形式（EXIF Model タグに sui_image_params を含む JSON）。
pub fn swarmui_jpeg_model(params: &str) -> Vec<u8> {
    jpeg(&JpegSpec {
        width: 1024,
        height: 1024,
        model: Some(params.to_string()),
        ..JpegSpec::default()
    })
}

pub fn invokeai_png() -> Vec<u8> {
    png(&PngSpec::default()
        .size(1024, 768)
        .text("invokeai_metadata", INVOKEAI_METADATA))
}

pub fn invokeai_sd_metadata_png() -> Vec<u8> {
    png(&PngSpec::default()
        .size(512, 512)
        .text("sd-metadata", INVOKEAI_SD_METADATA))
}

/// PNG の eXIf チャンクに UserComment(JSON) を持つ（ComfyUI 候補）。
pub fn png_exif_user_comment(text: &str) -> Vec<u8> {
    png(&PngSpec::default()
        .size(64, 48)
        .exif(tiff(&[(0x9286, 7, exif_user_comment(text))])))
}

/// IEND の後に parameters テキストを付加した PNG（Trailing 経由）。
pub fn png_trailing_parameters(parameters: &str) -> Vec<u8> {
    png(&PngSpec::default()
        .size(64, 48)
        .trailing(format!("parameters:{parameters}")))
}

/// XMP に Description だけを持つ画像（生成元判定には使わない）。
pub fn xmp_only_png() -> Vec<u8> {
    let packet = "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:description>a photo</dc:description></rdf:Description></rdf:RDF></x:xmpmeta>";
    png(&PngSpec::default().size(64, 48).xmp(packet.to_string()))
}

/// Stealth PNG Info のみを持ち、通常レコードを持たない画像。
pub fn stealth_only_png() -> Vec<u8> {
    // 19 byte のヘッダ + 本文が 64x64 の alpha に収まること。
    png(&PngSpec::default()
        .size(64, 64)
        .stealth(r#"{"prompt":"stealth only"}"#))
}

pub fn no_metadata_png() -> Vec<u8> {
    png(&PngSpec::default().size(64, 48))
}

pub fn no_metadata_jpeg() -> Vec<u8> {
    jpeg(&JpegSpec::default())
}

pub fn broken_image() -> Vec<u8> {
    b"this is not an image file".to_vec()
}

pub fn empty_file() -> Vec<u8> {
    Vec::new()
}

pub fn webp_with_exif_user_comment(text: &str) -> Vec<u8> {
    webp(&WebpSpec {
        width: 512,
        height: 512,
        exif: Some(tiff(&[(0x9286, 7, exif_user_comment(text))])),
        xmp: None,
    })
}

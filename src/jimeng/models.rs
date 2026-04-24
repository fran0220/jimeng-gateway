//! Model name mappings, resolution tables, and material type definitions.

use std::collections::HashMap;

/// Map user-facing model names to internal jimeng model keys.
pub fn model_map() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("seedance-2.0", "dreamina_seedance_40_pro"),
        ("seedance-2.0-pro", "dreamina_seedance_40_pro"),
        ("seedance-2.0-fast", "dreamina_seedance_40"),
        ("seedance-2.0-lite", "seedance_2_0_lite"),
        ("seedance-1-lite", "seedance_2_0_lite"),
    ])
}

/// Map model names to their draft content version.
pub fn draft_version(model: &str) -> &'static str {
    match model {
        "seedance-2.0" | "seedance-2.0-pro" | "seedance-2.0-fast" => "3.3.9",
        "jimeng-5.0" => "3.3.9",
        _ => "3.3.9",
    }
}

/// Map model names to their benefit_type.
pub fn seedance_benefit_type(model: &str) -> &'static str {
    match model {
        "seedance-2.0" | "seedance-2.0-pro" => "dreamina_video_seedance_20_pro",
        "seedance-2.0-fast" => "dreamina_seedance_20_fast",
        "seedance-2.0-lite" | "seedance-1-lite" => "seedance_2_0_lite",
        _ => "dreamina_video_seedance_20_pro",
    }
}

/// Resolve user model name to internal model key.
pub fn resolve_model(model: &str) -> &str {
    let map = model_map();
    map.get(model).copied().unwrap_or("dreamina_seedance_40_pro")
}

/// Video resolution dimensions.
#[derive(Debug, Clone, Copy)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Resolve video resolution from resolution string and ratio string.
pub fn resolve_video_resolution(resolution: &str, ratio: &str) -> Result<Resolution, String> {
    let table: HashMap<(&str, &str), (u32, u32)> = HashMap::from([
        // 480p
        (("480p", "1:1"), (480, 480)),
        (("480p", "4:3"), (640, 480)),
        (("480p", "3:4"), (480, 640)),
        (("480p", "16:9"), (854, 480)),
        (("480p", "9:16"), (480, 854)),
        // 720p
        (("720p", "1:1"), (720, 720)),
        (("720p", "4:3"), (960, 720)),
        (("720p", "3:4"), (720, 960)),
        (("720p", "16:9"), (1280, 720)),
        (("720p", "9:16"), (720, 1280)),
        // 1080p
        (("1080p", "1:1"), (1080, 1080)),
        (("1080p", "4:3"), (1440, 1080)),
        (("1080p", "3:4"), (1080, 1440)),
        (("1080p", "16:9"), (1920, 1080)),
        (("1080p", "9:16"), (1080, 1920)),
    ]);

    match table.get(&(resolution, ratio)) {
        Some(&(w, h)) => Ok(Resolution { width: w, height: h }),
        None => Err(format!("Unsupported resolution/ratio: {resolution}/{ratio}")),
    }
}

/// Material type for Seedance multi-modal upload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaterialType {
    Image,
    Video,
    Audio,
}

impl MaterialType {
    /// Numeric code used in materialTypes array.
    pub fn code(&self) -> u32 {
        match self {
            Self::Image => 1,
            Self::Video => 2,
            Self::Audio => 3,
        }
    }

    /// String name used in material_type / meta_type fields.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }
}

/// Detect material type from MIME type string.
pub fn detect_material_type_from_mime(mime: &str) -> MaterialType {
    let mime = mime.to_lowercase();
    if mime.starts_with("image/") {
        MaterialType::Image
    } else if mime.starts_with("video/") {
        MaterialType::Video
    } else if mime.starts_with("audio/") {
        MaterialType::Audio
    } else {
        MaterialType::Image // default fallback
    }
}

/// Detect material type from file extension.
#[allow(dead_code)]
pub fn detect_material_type_from_ext(filename: &str) -> MaterialType {
    let lower = filename.to_lowercase();
    if let Some(dot_pos) = lower.rfind('.') {
        match &lower[dot_pos..] {
            ".jpg" | ".jpeg" | ".png" | ".webp" | ".gif" | ".bmp" => MaterialType::Image,
            ".mp4" | ".mov" | ".m4v" => MaterialType::Video,
            ".mp3" | ".wav" => MaterialType::Audio,
            _ => MaterialType::Image,
        }
    } else {
        MaterialType::Image
    }
}

/// Uploaded material result (unified across ImageX and VOD).
#[derive(Debug, Clone)]
pub struct UploadedMaterial {
    pub material_type: MaterialType,
    /// Image URI (for ImageX uploads).
    pub uri: Option<String>,
    /// Video ID (for VOD uploads).
    pub vid: Option<String>,
    pub width: u32,
    pub height: u32,
    pub duration: u32,
    pub fps: u32,
    pub name: String,
}

/// Check if a model name is an image generation model.
pub fn is_image_model(model: &str) -> bool {
    model.starts_with("jimeng-")
}

/// Map user-facing image model names to internal jimeng model keys.
pub fn resolve_image_model(model: &str) -> &str {
    match model {
        "jimeng-5.0" => "high_aes_general_v50",
        _ => "high_aes_general_v50",
    }
}

/// Image resolution dimensions and ratio code.
#[derive(Debug, Clone, Copy)]
pub struct ImageResolution {
    pub width: u32,
    pub height: u32,
    pub ratio_code: u32,
}

/// Resolve image resolution from resolution string (1k/2k/4k) and ratio string.
pub fn resolve_image_resolution(resolution: &str, ratio: &str) -> Result<ImageResolution, String> {
    let table: HashMap<(&str, &str), (u32, u32, u32)> = HashMap::from([
        // 1k
        (("1k", "1:1"), (1024, 1024, 1)),
        (("1k", "4:3"), (768, 1024, 4)),
        (("1k", "3:4"), (1024, 768, 2)),
        (("1k", "16:9"), (1024, 576, 3)),
        (("1k", "9:16"), (576, 1024, 5)),
        (("1k", "3:2"), (1024, 682, 7)),
        (("1k", "2:3"), (682, 1024, 6)),
        (("1k", "21:9"), (1195, 512, 8)),
        // 2k
        (("2k", "1:1"), (2048, 2048, 1)),
        (("2k", "4:3"), (2304, 1728, 4)),
        (("2k", "3:4"), (1728, 2304, 2)),
        (("2k", "16:9"), (2560, 1440, 3)),
        (("2k", "9:16"), (1440, 2560, 5)),
        (("2k", "3:2"), (2496, 1664, 7)),
        (("2k", "2:3"), (1664, 2496, 6)),
        (("2k", "21:9"), (3024, 1296, 8)),
        // 4k
        (("4k", "1:1"), (4096, 4096, 101)),
        (("4k", "4:3"), (4608, 3456, 104)),
        (("4k", "3:4"), (3456, 4608, 102)),
        (("4k", "16:9"), (5120, 2880, 103)),
        (("4k", "9:16"), (2880, 5120, 105)),
        (("4k", "3:2"), (4992, 3328, 107)),
        (("4k", "2:3"), (3328, 4992, 106)),
        (("4k", "21:9"), (6048, 2592, 108)),
    ]);

    match table.get(&(resolution, ratio)) {
        Some(&(w, h, code)) => Ok(ImageResolution { width: w, height: h, ratio_code: code }),
        None => Err(format!("Unsupported image resolution/ratio: {resolution}/{ratio}")),
    }
}

/// Reverse-lookup: given pixel dimensions, find the exact (ratio, resolution_tier).
/// Returns None if the dimensions don't match any supported entry.
pub fn lookup_image_size(width: u32, height: u32) -> Option<(&'static str, &'static str)> {
    // (width, height) → (ratio, resolution_tier)
    let table: &[((u32, u32), &str, &str)] = &[
        // 1k
        ((1024, 1024), "1:1", "1k"),
        ((768, 1024), "4:3", "1k"),
        ((1024, 768), "3:4", "1k"),
        ((1024, 576), "16:9", "1k"),
        ((576, 1024), "9:16", "1k"),
        ((1024, 682), "3:2", "1k"),
        ((682, 1024), "2:3", "1k"),
        ((1195, 512), "21:9", "1k"),
        // 2k
        ((2048, 2048), "1:1", "2k"),
        ((2304, 1728), "4:3", "2k"),
        ((1728, 2304), "3:4", "2k"),
        ((2560, 1440), "16:9", "2k"),
        ((1440, 2560), "9:16", "2k"),
        ((2496, 1664), "3:2", "2k"),
        ((1664, 2496), "2:3", "2k"),
        ((3024, 1296), "21:9", "2k"),
        // 4k
        ((4096, 4096), "1:1", "4k"),
        ((4608, 3456), "4:3", "4k"),
        ((3456, 4608), "3:4", "4k"),
        ((5120, 2880), "16:9", "4k"),
        ((2880, 5120), "9:16", "4k"),
        ((4992, 3328), "3:2", "4k"),
        ((3328, 4992), "2:3", "4k"),
        ((6048, 2592), "21:9", "4k"),
    ];

    table.iter()
        .find(|((w, h), _, _)| *w == width && *h == height)
        .map(|(_, ratio, res)| (*ratio, *res))
}

/// Compute GCD of two numbers.
pub fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Compute aspect ratio string from width and height (e.g. "16:9").
pub fn aspect_ratio_str(width: u32, height: u32) -> String {
    let d = gcd(width, height);
    format!("{}:{}", width / d, height / d)
}

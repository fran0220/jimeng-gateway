//! Model name mappings, resolution tables, and material type definitions.

use std::collections::HashMap;

/// Map user-facing model names to internal jimeng model keys.
pub fn model_map() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("jimeng-video-3.5-pro", "dreamina_ic_generate_video_model_vgfm_3.5_pro"),
        ("jimeng-video-3.0-pro", "dreamina_ic_generate_video_model_vgfm_3.0_pro"),
        ("jimeng-video-3.0", "dreamina_ic_generate_video_model_vgfm_3.0"),
        ("jimeng-video-2.0", "dreamina_ic_generate_video_model_vgfm_lite"),
        ("jimeng-video-2.0-pro", "dreamina_ic_generate_video_model_vgfm1.0"),
        ("jimeng-video-seedance-2.0", "dreamina_seedance_40_pro"),
        ("seedance-2.0", "dreamina_seedance_40_pro"),
        ("seedance-2.0-pro", "dreamina_seedance_40_pro"),
        ("jimeng-video-seedance-2.0-fast", "dreamina_seedance_40"),
        ("seedance-2.0-fast", "dreamina_seedance_40"),
    ])
}

/// Map model names to their draft content version.
pub fn draft_version(model: &str) -> &'static str {
    match model {
        "jimeng-video-3.5-pro" => "3.3.4",
        "jimeng-video-3.0-pro" | "jimeng-video-3.0" | "jimeng-video-2.0" | "jimeng-video-2.0-pro" => "3.2.8",
        "jimeng-video-seedance-2.0" | "seedance-2.0" | "seedance-2.0-pro"
        | "jimeng-video-seedance-2.0-fast" | "seedance-2.0-fast" => "3.3.9",
        _ => "3.2.8",
    }
}

/// Map Seedance model names to their benefit_type.
pub fn seedance_benefit_type(model: &str) -> &'static str {
    match model {
        "jimeng-video-seedance-2.0" | "seedance-2.0" | "seedance-2.0-pro" => "dreamina_video_seedance_20_pro",
        "jimeng-video-seedance-2.0-fast" | "seedance-2.0-fast" => "dreamina_seedance_20_fast",
        _ => "dreamina_video_seedance_20_pro",
    }
}

/// Check if a model name is a Seedance model.
pub fn is_seedance_model(model: &str) -> bool {
    model.starts_with("seedance-") || model.starts_with("jimeng-video-seedance-")
}

/// Resolve user model name to internal model key.
pub fn resolve_model(model: &str) -> &str {
    let map = model_map();
    map.get(model).copied().unwrap_or("dreamina_ic_generate_video_model_vgfm_3.0")
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

/// Compute GCD of two numbers.
pub fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Compute aspect ratio string from width and height (e.g. "16:9").
pub fn aspect_ratio_str(width: u32, height: u32) -> String {
    let d = gcd(width, height);
    format!("{}:{}", width / d, height / d)
}

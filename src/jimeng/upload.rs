//! File upload to ByteDance ImageX and VOD services with AWS Signature V4.

use anyhow::{bail, Result};
use hmac::{Hmac, Mac};
use sha2::{Sha256, Digest};
use reqwest::Client;

use super::auth;
use super::models::MaterialType;

const JIMENG_BASE: &str = "https://jimeng.jianying.com";
const IMAGEX_HOST: &str = "https://imagex.bytedanceapi.com";
const VOD_HOST: &str = "https://vod.bytedanceapi.com";
const DEFAULT_SERVICE_ID: &str = "tb4s082cfz";
const DEFAULT_SPACE_NAME: &str = "dreamina";

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36";

type HmacSha256 = Hmac<Sha256>;

/// Compute CRC32 of a byte slice (hex string, zero-padded to 8 chars).
fn crc32_hex(data: &[u8]) -> String {
    let crc = crc32fast::hash(data);
    format!("{:08x}", crc)
}

/// Generate an AWS4-HMAC-SHA256 signature.
fn aws4_signature(
    method: &str,
    url: &str,
    headers_to_sign: &[(&str, &str)],
    access_key_id: &str,
    secret_access_key: &str,
    session_token: Option<&str>,
    payload: &str,
    region: &str,
    service: &str,
) -> Result<String> {
    let parsed = reqwest::Url::parse(url)?;
    let pathname = parsed.path();

    // Find x-amz-date from headers
    let timestamp = headers_to_sign.iter()
        .find(|(k, _)| *k == "x-amz-date")
        .map(|(_, v)| *v)
        .unwrap_or("");
    let date = &timestamp[..8];

    // Canonical query string (sorted)
    let mut query_pairs: Vec<(String, String)> = parsed.query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    query_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_query = query_pairs.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    // Build signed headers map
    let mut sign_headers: Vec<(String, String)> = headers_to_sign.iter()
        .map(|(k, v)| (k.to_lowercase(), v.to_string()))
        .collect();
    if let Some(st) = session_token {
        if !sign_headers.iter().any(|(k, _)| k == "x-amz-security-token") {
            sign_headers.push(("x-amz-security-token".to_string(), st.to_string()));
        }
    }

    let payload_hash = if method.to_uppercase() == "POST" && !payload.is_empty() {
        let hash = hex::encode(Sha256::digest(payload.as_bytes()));
        if !sign_headers.iter().any(|(k, _)| k == "x-amz-content-sha256") {
            sign_headers.push(("x-amz-content-sha256".to_string(), hash.clone()));
        }
        hash
    } else {
        hex::encode(Sha256::digest(b""))
    };

    sign_headers.sort_by(|a, b| a.0.cmp(&b.0));
    let signed_headers_str = sign_headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let canonical_headers = sign_headers.iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect::<String>();

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(), pathname, canonical_query, canonical_headers, signed_headers_str, payload_hash
    );

    let credential_scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        timestamp, credential_scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    // Derive signing key
    let k_date = hmac_sha256(format!("AWS4{secret_access_key}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    Ok(format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{credential_scope}, SignedHeaders={signed_headers_str}, Signature={signature}"
    ))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn aws_timestamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Get upload token from jimeng API.
async fn get_upload_token(
    client: &Client,
    session_token: &str,
    scene: u32,
) -> Result<serde_json::Value> {
    let uri = "/mweb/v1/get_upload_token";
    let headers = auth::build_headers(session_token, uri);
    let params = auth::standard_query_params();

    let resp = client
        .post(format!("{JIMENG_BASE}{uri}"))
        .headers(headers)
        .query(&params)
        .json(&serde_json::json!({ "scene": scene }))
        .send()
        .await?;

    let text = resp.text().await?;
    let val: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("get_upload_token parse error: {e}, body: {}", &text[..text.len().min(500)]))?;

    // Extract data from { ret: "0", data: {...} } format
    if let Some(data) = val.get("data") {
        Ok(data.clone())
    } else {
        Ok(val)
    }
}

/// Upload an image to ImageX and return the image URI.
///
/// Flow: get_upload_token(scene=2) → ApplyImageUpload → Upload binary → CommitImageUpload
pub async fn upload_image(
    client: &Client,
    session_token: &str,
    image_data: &[u8],
) -> Result<String> {
    let token_data = get_upload_token(client, session_token, 2).await?;
    let access_key = token_data["access_key_id"].as_str().unwrap_or("");
    let secret_key = token_data["secret_access_key"].as_str().unwrap_or("");
    let session_tok = token_data["session_token"].as_str().unwrap_or("");
    let service_id = token_data["service_id"].as_str().unwrap_or(DEFAULT_SERVICE_ID);

    if access_key.is_empty() || secret_key.is_empty() || session_tok.is_empty() {
        bail!("Failed to get ImageX upload token");
    }

    let file_size = image_data.len();
    let random_str: String = (0..10).map(|_| rand::random::<char>()).collect::<String>()
        .chars().filter(|c| c.is_alphanumeric()).take(10).collect();
    let random_str = if random_str.len() < 5 { uuid::Uuid::new_v4().to_string()[..10].to_string() } else { random_str };

    // Step 1: ApplyImageUpload
    let timestamp = aws_timestamp();
    let apply_url = format!(
        "{IMAGEX_HOST}/?Action=ApplyImageUpload&Version=2018-08-01&ServiceId={service_id}&FileSize={file_size}&s={random_str}"
    );

    let req_headers = vec![
        ("x-amz-date", timestamp.as_str()),
        ("x-amz-security-token", session_tok),
    ];
    let authorization = aws4_signature("GET", &apply_url, &req_headers, access_key, secret_key, Some(session_tok), "", "cn-north-1", "imagex")?;

    let apply_resp = client.get(&apply_url)
        .header("accept", "*/*")
        .header("authorization", &authorization)
        .header("origin", "https://jimeng.jianying.com")
        .header("referer", "https://jimeng.jianying.com/ai-tool/video/generate")
        .header("user-agent", USER_AGENT)
        .header("x-amz-date", &timestamp)
        .header("x-amz-security-token", session_tok)
        .send().await?;

    let apply_text = apply_resp.text().await?;
    let apply_result: serde_json::Value = serde_json::from_str(&apply_text)
        .map_err(|e| anyhow::anyhow!("ApplyImageUpload parse error: {e}"))?;

    if let Some(err) = apply_result.pointer("/ResponseMetadata/Error") {
        bail!("ApplyImageUpload failed: {err}");
    }

    let upload_address = apply_result.pointer("/Result/UploadAddress")
        .ok_or_else(|| anyhow::anyhow!("No UploadAddress in ApplyImageUpload response"))?;
    let store_info = upload_address.pointer("/StoreInfos/0")
        .ok_or_else(|| anyhow::anyhow!("No StoreInfos in upload address"))?;
    let upload_host = upload_address.pointer("/UploadHosts/0")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("No UploadHosts"))?;
    let store_uri = store_info["StoreUri"].as_str().unwrap_or("");
    let store_auth = store_info["Auth"].as_str().unwrap_or("");
    let session_key = upload_address["SessionKey"].as_str().unwrap_or("");

    // Step 2: Upload binary
    let upload_url = format!("https://{upload_host}/upload/v1/{store_uri}");
    let crc32 = crc32_hex(image_data);

    let upload_resp = client.post(&upload_url)
        .header("Authorization", store_auth)
        .header("Content-CRC32", &crc32)
        .header("Content-Disposition", "attachment; filename=\"undefined\"")
        .header("Content-Type", "application/octet-stream")
        .header("Origin", "https://jimeng.jianying.com")
        .header("User-Agent", USER_AGENT)
        .body(image_data.to_vec())
        .send().await?;

    if !upload_resp.status().is_success() {
        bail!("Image upload failed: HTTP {}", upload_resp.status());
    }

    // Step 3: CommitImageUpload
    let commit_url = format!("{IMAGEX_HOST}/?Action=CommitImageUpload&Version=2018-08-01&ServiceId={service_id}");
    let commit_timestamp = aws_timestamp();
    let commit_payload = serde_json::json!({
        "SessionKey": session_key,
        "SuccessActionStatus": "200"
    }).to_string();

    let payload_hash = hex::encode(Sha256::digest(commit_payload.as_bytes()));
    let commit_headers = vec![
        ("x-amz-date", commit_timestamp.as_str()),
        ("x-amz-security-token", session_tok),
        ("x-amz-content-sha256", payload_hash.as_str()),
    ];
    let commit_auth = aws4_signature("POST", &commit_url, &commit_headers, access_key, secret_key, Some(session_tok), &commit_payload, "cn-north-1", "imagex")?;

    let commit_resp = client.post(&commit_url)
        .header("authorization", &commit_auth)
        .header("content-type", "application/json")
        .header("origin", "https://jimeng.jianying.com")
        .header("user-agent", USER_AGENT)
        .header("x-amz-date", &commit_timestamp)
        .header("x-amz-security-token", session_tok)
        .header("x-amz-content-sha256", &payload_hash)
        .body(commit_payload)
        .send().await?;

    let commit_text = commit_resp.text().await?;
    let commit_result: serde_json::Value = serde_json::from_str(&commit_text)
        .map_err(|e| anyhow::anyhow!("CommitImageUpload parse error: {e}"))?;

    if let Some(err) = commit_result.pointer("/ResponseMetadata/Error") {
        bail!("CommitImageUpload failed: {err}");
    }

    // Try plugin result first, then direct result
    if let Some(uri) = commit_result.pointer("/Result/PluginResult/0/ImageUri").and_then(|v| v.as_str()) {
        return Ok(uri.to_string());
    }
    if let Some(uri) = commit_result.pointer("/Result/Results/0/Uri").and_then(|v| v.as_str()) {
        return Ok(uri.to_string());
    }

    bail!("CommitImageUpload: no URI in response: {commit_text}")
}

/// Upload result from VOD.
#[derive(Debug, Clone)]
pub struct VodUploadResult {
    pub vid: String,
    pub width: u32,
    pub height: u32,
    pub duration: u32,
    pub fps: u32,
}

/// Upload video/audio to ByteDance VOD and return the vid + metadata.
///
/// Flow: get_upload_token(scene=1) → ApplyUploadInner → Upload binary → CommitUploadInner
pub async fn upload_media(
    client: &Client,
    session_token: &str,
    data: &[u8],
    media_type: MaterialType,
) -> Result<VodUploadResult> {
    let token_data = get_upload_token(client, session_token, 1).await?;
    let access_key = token_data["access_key_id"].as_str().unwrap_or("");
    let secret_key = token_data["secret_access_key"].as_str().unwrap_or("");
    let session_tok = token_data["session_token"].as_str().unwrap_or("");
    let space_name = token_data["space_name"].as_str().unwrap_or(DEFAULT_SPACE_NAME);

    if access_key.is_empty() || secret_key.is_empty() || session_tok.is_empty() {
        bail!("Failed to get VOD upload token");
    }

    let file_size = data.len();
    let random_str = &uuid::Uuid::new_v4().to_string()[..10];

    // Step 1: ApplyUploadInner
    let timestamp = aws_timestamp();
    let apply_url = format!(
        "{VOD_HOST}/?Action=ApplyUploadInner&Version=2020-11-19&SpaceName={space_name}&FileType=video&IsInner=1&FileSize={file_size}&s={random_str}"
    );

    let req_headers = vec![
        ("x-amz-date", timestamp.as_str()),
        ("x-amz-security-token", session_tok),
    ];
    let authorization = aws4_signature("GET", &apply_url, &req_headers, access_key, secret_key, Some(session_tok), "", "cn-north-1", "vod")?;

    let apply_resp = client.get(&apply_url)
        .header("authorization", &authorization)
        .header("origin", "https://jimeng.jianying.com")
        .header("user-agent", USER_AGENT)
        .header("x-amz-date", &timestamp)
        .header("x-amz-security-token", session_tok)
        .send().await?;

    let apply_text = apply_resp.text().await?;
    let apply_result: serde_json::Value = serde_json::from_str(&apply_text)
        .map_err(|e| anyhow::anyhow!("ApplyUploadInner parse error: {e}"))?;

    if let Some(err) = apply_result.pointer("/ResponseMetadata/Error") {
        bail!("ApplyUploadInner failed: {err}");
    }

    let upload_node = apply_result.pointer("/Result/InnerUploadAddress/UploadNodes/0")
        .ok_or_else(|| anyhow::anyhow!("No upload nodes in VOD response"))?;
    let store_info = upload_node.pointer("/StoreInfos/0")
        .ok_or_else(|| anyhow::anyhow!("No StoreInfos in VOD upload node"))?;

    let upload_host = upload_node["UploadHost"].as_str().unwrap_or("");
    let store_uri = store_info["StoreUri"].as_str().unwrap_or("");
    let store_auth = store_info["Auth"].as_str().unwrap_or("");
    let session_key = upload_node["SessionKey"].as_str().unwrap_or("");
    let vid = upload_node["Vid"].as_str().unwrap_or("");

    // Step 2: Upload binary
    let upload_url = format!("https://{upload_host}/upload/v1/{store_uri}");
    let crc32 = crc32_hex(data);

    let upload_resp = client.post(&upload_url)
        .header("Authorization", store_auth)
        .header("Content-CRC32", &crc32)
        .header("Content-Type", "application/octet-stream")
        .header("Origin", "https://jimeng.jianying.com")
        .header("User-Agent", USER_AGENT)
        .body(data.to_vec())
        .send().await?;

    if !upload_resp.status().is_success() {
        bail!("VOD upload failed: HTTP {}", upload_resp.status());
    }

    // Step 3: CommitUploadInner
    let commit_url = format!("{VOD_HOST}/?Action=CommitUploadInner&Version=2020-11-19&SpaceName={space_name}");
    let commit_timestamp = aws_timestamp();
    let commit_payload = serde_json::json!({
        "SessionKey": session_key,
        "Functions": []
    }).to_string();

    let payload_hash = hex::encode(Sha256::digest(commit_payload.as_bytes()));
    let commit_headers = vec![
        ("x-amz-date", commit_timestamp.as_str()),
        ("x-amz-security-token", session_tok),
        ("x-amz-content-sha256", payload_hash.as_str()),
    ];
    let commit_auth = aws4_signature("POST", &commit_url, &commit_headers, access_key, secret_key, Some(session_tok), &commit_payload, "cn-north-1", "vod")?;

    let commit_resp = client.post(&commit_url)
        .header("authorization", &commit_auth)
        .header("content-type", "application/json")
        .header("origin", "https://jimeng.jianying.com")
        .header("user-agent", USER_AGENT)
        .header("x-amz-date", &commit_timestamp)
        .header("x-amz-security-token", session_tok)
        .header("x-amz-content-sha256", &payload_hash)
        .body(commit_payload)
        .send().await?;

    let commit_text = commit_resp.text().await?;
    let commit_result: serde_json::Value = serde_json::from_str(&commit_text)
        .map_err(|e| anyhow::anyhow!("CommitUploadInner parse error: {e}"))?;

    if let Some(err) = commit_result.pointer("/ResponseMetadata/Error") {
        bail!("CommitUploadInner failed: {err}");
    }

    let result = commit_result.pointer("/Result/Results/0")
        .ok_or_else(|| anyhow::anyhow!("No results in CommitUploadInner response"))?;

    let final_vid = result["Vid"].as_str().unwrap_or(vid).to_string();
    let video_meta = result.get("VideoMeta").cloned().unwrap_or_default();

    let mut duration_ms = video_meta.get("Duration")
        .and_then(|v| v.as_f64())
        .map(|d| (d * 1000.0) as u32)
        .unwrap_or(0);

    // Fallback: parse WAV duration locally for audio
    if duration_ms == 0 && media_type == MaterialType::Audio {
        duration_ms = parse_audio_duration(data);
    }

    Ok(VodUploadResult {
        vid: final_vid,
        width: video_meta.get("Width").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        height: video_meta.get("Height").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        duration: duration_ms,
        fps: video_meta.get("Fps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
    })
}

/// Parse WAV audio duration from header. Returns duration in milliseconds.
fn parse_audio_duration(data: &[u8]) -> u32 {
    if data.len() < 44 {
        return 0;
    }
    // Check RIFF...WAVE magic
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        // Not WAV, estimate at 128kbps
        return ((data.len() as f64) / (128.0 * 1000.0 / 8.0) * 1000.0) as u32;
    }
    let byte_rate = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
    if byte_rate == 0 {
        return 0;
    }
    // Find data chunk
    let mut offset = 12;
    while offset + 8 < data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]);
        if chunk_id == b"data" {
            return ((chunk_size as f64) / (byte_rate as f64) * 1000.0) as u32;
        }
        offset += 8 + chunk_size as usize;
    }
    // Fallback
    (((data.len() - 44) as f64) / (byte_rate as f64) * 1000.0) as u32
}

/// Download file from URL and return its bytes.
#[allow(dead_code)]
pub async fn download_file(client: &Client, url: &str) -> Result<Vec<u8>> {
    let resp = client.get(url)
        .timeout(std::time::Duration::from_secs(60))
        .send().await?;
    if !resp.status().is_success() {
        bail!("Download failed: HTTP {} for {url}", resp.status());
    }
    Ok(resp.bytes().await?.to_vec())
}

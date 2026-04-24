//! Task submission to jimeng.jianying.com API.
//! Seedance models use pure Rust a_bogus signing with browser fallback.
//! Image models use plain HTTP (no a_bogus needed).

use anyhow::{bail, Result};
use reqwest::Client;

use super::abogus;
use super::auth;
use super::models::{self, UploadedMaterial, MaterialType};
use super::browser::BrowserService;

const JIMENG_BASE: &str = "https://jimeng.jianying.com";

/// Result of a video generation submission.
#[derive(Debug, Clone)]
pub struct SubmitResult {
    pub history_record_id: String,
}

/// Submit a Seedance video generation task.
/// Tries pure Rust a_bogus signing first; falls back to browser proxy on failure.
pub async fn submit_seedance_video(
    client: &Client,
    browser: &BrowserService,
    session_token: &str,
    prompt: &str,
    model_name: &str,
    width: u32,
    height: u32,
    duration: u32,
    materials: &[UploadedMaterial],
    cookie_jar: Option<&str>,
) -> Result<SubmitResult> {
    let internal_model = models::resolve_model(model_name);
    let benefit_type = models::seedance_benefit_type(model_name);
    let draft_version = models::draft_version(model_name);
    let aspect_ratio = models::aspect_ratio_str(width, height);

    let has_video_material = materials.iter().any(|m| m.material_type == MaterialType::Video);
    let final_benefit_type = if has_video_material {
        format!("{benefit_type}_with_video")
    } else {
        benefit_type.to_string()
    };

    // Build material_list
    let material_list: Vec<serde_json::Value> = materials.iter().map(|mat| {
        let base_id = uuid::Uuid::new_v4().to_string();
        match mat.material_type {
            MaterialType::Image => serde_json::json!({
                "type": "", "id": base_id,
                "material_type": "image",
                "image_info": {
                    "type": "image", "id": uuid::Uuid::new_v4().to_string(),
                    "source_from": "upload", "platform_type": 1, "name": "",
                    "image_uri": mat.uri.as_deref().unwrap_or(""),
                    "aigc_image": { "type": "", "id": uuid::Uuid::new_v4().to_string() },
                    "width": mat.width, "height": mat.height,
                    "format": "", "uri": mat.uri.as_deref().unwrap_or(""),
                }
            }),
            MaterialType::Video => serde_json::json!({
                "type": "", "id": base_id,
                "material_type": "video",
                "video_info": {
                    "type": "video", "id": uuid::Uuid::new_v4().to_string(),
                    "source_from": "upload", "name": mat.name,
                    "vid": mat.vid.as_deref().unwrap_or(""),
                    "fps": mat.fps, "width": mat.width, "height": mat.height,
                    "duration": mat.duration,
                }
            }),
            MaterialType::Audio => serde_json::json!({
                "type": "", "id": base_id,
                "material_type": "audio",
                "audio_info": {
                    "type": "audio", "id": uuid::Uuid::new_v4().to_string(),
                    "source_from": "upload",
                    "vid": mat.vid.as_deref().unwrap_or(""),
                    "duration": mat.duration, "name": mat.name,
                }
            }),
        }
    }).collect();

    // Build meta_list from prompt placeholders
    let meta_list = build_meta_list(prompt, materials);

    let component_id = uuid::Uuid::new_v4().to_string();
    let submit_id = uuid::Uuid::new_v4().to_string();

    let material_type_codes: Vec<u32> = materials.iter()
        .map(|m| m.material_type.code())
        .collect::<std::collections::HashSet<_>>()
        .into_iter().collect();

    let metrics_extra = serde_json::json!({
        "isDefaultSeed": 1,
        "originSubmitId": submit_id,
        "isRegenerate": false,
        "enterFrom": "click",
        "position": "page_bottom_box",
        "functionMode": "omni_reference",
        "sceneOptions": serde_json::json!([{
            "type": "video",
            "scene": "BasicVideoGenerateButton",
            "modelReqKey": internal_model,
            "videoDuration": duration,
            "reportParams": {
                "enterSource": "generate",
                "vipSource": "generate",
                "extraVipFunctionKey": internal_model,
                "useVipFunctionDetailsReporterHoc": true
            },
            "materialTypes": material_type_codes
        }]).to_string()
    }).to_string();

    let draft_content = serde_json::json!({
        "type": "draft",
        "id": uuid::Uuid::new_v4().to_string(),
        "min_version": draft_version,
        "min_features": ["AIGC_Video_UnifiedEdit"],
        "is_from_tsn": true,
        "version": draft_version,
        "main_component_id": component_id,
        "component_list": [{
            "type": "video_base_component",
            "id": component_id,
            "min_version": "1.0.0",
            "aigc_mode": "workbench",
            "metadata": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "created_platform": 3, "created_platform_version": "",
                "created_time_in_ms": chrono::Utc::now().timestamp_millis().to_string(),
                "created_did": ""
            },
            "generate_type": "gen_video",
            "abilities": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "gen_video": {
                    "type": "", "id": uuid::Uuid::new_v4().to_string(),
                    "text_to_video_params": {
                        "type": "", "id": uuid::Uuid::new_v4().to_string(),
                        "video_gen_inputs": [{
                            "type": "", "id": uuid::Uuid::new_v4().to_string(),
                            "min_version": draft_version,
                            "prompt": "",
                            "video_mode": 2,
                            "fps": 24,
                            "duration_ms": duration * 1000,
                            "idip_meta_list": [],
                            "unified_edit_input": {
                                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                                "material_list": material_list,
                                "meta_list": meta_list,
                            }
                        }],
                        "video_aspect_ratio": aspect_ratio,
                        "seed": rand::random::<u32>() % 1000000000,
                        "model_req_key": internal_model,
                        "priority": 0
                    },
                    "video_task_extra": metrics_extra,
                }
            },
            "process_type": 1
        }]
    });

    let body = serde_json::json!({
        "extend": {
            "root_model": internal_model,
            "m_video_commerce_info": {
                "benefit_type": final_benefit_type,
                "resource_id": "generate_video",
                "resource_id_type": "str",
                "resource_sub_type": "aigc"
            },
            "m_video_commerce_info_list": [{
                "benefit_type": final_benefit_type,
                "resource_id": "generate_video",
                "resource_id_type": "str",
                "resource_sub_type": "aigc"
            }]
        },
        "submit_id": submit_id,
        "metrics_extra": metrics_extra,
        "draft_content": draft_content.to_string(),
        "http_common_info": {
            "aid": auth::DEFAULT_ASSISTANT_ID,
        },
    });

    // Build the full URL with query params (match browser's params exactly)
    let std_params = auth::standard_query_params();
    let query_string = std_params.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    let body_str = body.to_string();

    // Try pure Rust a_bogus signing first
    let use_browser = std::env::var("ABOGUS_MODE")
        .map(|v| v == "browser")
        .unwrap_or(false);

    let result = if use_browser {
        // Browser fallback mode
        let url = format!("{JIMENG_BASE}/mweb/v1/aigc_draft/generate?{query_string}");
        tracing::info!("Seedance: submitting via browser proxy");
        browser.fetch(session_token, &url, &body_str).await?
    } else {
        // Direct HTTP with full cookie jar (no a_bogus needed when cookies are complete)
        let url = if cookie_jar.is_some() {
            // With full cookie jar, a_bogus is not needed
            format!("{JIMENG_BASE}/mweb/v1/aigc_draft/generate?{query_string}")
        } else {
            // Fallback: try pure Rust a_bogus when no cookie jar
            let a_bogus = abogus::generate(&query_string, "POST");
            format!("{JIMENG_BASE}/mweb/v1/aigc_draft/generate?{query_string}&a_bogus={a_bogus}")
        };
        let uri = "/mweb/v1/aigc_draft/generate";
        let headers = auth::build_headers_with_cookies(session_token, uri, cookie_jar);

        tracing::info!(
            has_cookie_jar = cookie_jar.is_some(),
            cookie_jar_len = cookie_jar.map(|s| s.len()).unwrap_or(0),
            url_len = url.len(),
            "Seedance: submitting via direct HTTP"
        );

        let resp = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(resp) => {
                let status_code = resp.status();
                let text = resp.text().await?;
                tracing::info!(%status_code, body_preview = &text[..text.len().min(200)], "Seedance submit response");
                if !status_code.is_success() {
                    tracing::warn!(
                        "Seedance pure Rust a_bogus got HTTP {status_code}, falling back to browser"
                    );
                    let url = format!("{JIMENG_BASE}/mweb/v1/aigc_draft/generate?{query_string}");
                    browser.fetch(session_token, &url, &body_str).await?
                } else {
                    text
                }
            }
            Err(e) => {
                tracing::warn!("Seedance pure Rust a_bogus request failed: {e}, falling back to browser");
                let url = format!("{JIMENG_BASE}/mweb/v1/aigc_draft/generate?{query_string}");
                browser.fetch(session_token, &url, &body_str).await?
            }
        }
    };

    // Parse response
    let payload: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| anyhow::anyhow!("Seedance submit parse error: {e}. Body: {}", &result[..result.len().min(500)]))?;

    if let Some(ret) = payload.get("ret") {
        let ret_num = ret.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| ret.as_i64()).unwrap_or(0);
        if ret_num != 0 {
            let errmsg = payload.get("errmsg").and_then(|v| v.as_str()).unwrap_or("unknown");
            bail!("Seedance submit failed [ret={ret_num}]: {errmsg}");
        }
    }

    let history_id = payload.pointer("/data/aigc_data/history_record_id")
        .or_else(|| payload.pointer("/aigc_data/history_record_id"))
        .or_else(|| payload.pointer("/data/history_record_id"));

    let history_id = match history_id {
        Some(v) => {
            if let Some(s) = v.as_str() { s.to_string() }
            else if let Some(n) = v.as_i64() { n.to_string() }
            else { bail!("Unexpected history_record_id type in Seedance response") }
        }
        None => bail!("No history_record_id in Seedance submit response"),
    };

    Ok(SubmitResult { history_record_id: history_id })
}

/// Submit an image generation task via direct HTTP (no browser proxy needed).
///
/// When `reference_image_uris` is non-empty, uses blend mode (image-to-image)
/// instead of generate mode (text-to-image).
pub async fn submit_image_generation(
    client: &Client,
    session_token: &str,
    prompt: &str,
    model_name: &str,
    width: u32,
    height: u32,
    image_ratio: u32,
    resolution_type: &str,
    sample_strength: f64,
    negative_prompt: &str,
    reference_image_uris: &[String],
    cookie_jar: Option<&str>,
) -> Result<SubmitResult> {
    let internal_model = models::resolve_image_model(model_name);
    let is_blend = !reference_image_uris.is_empty();

    let component_id = uuid::Uuid::new_v4().to_string();
    let submit_id = uuid::Uuid::new_v4().to_string();
    let seed = rand::random::<u32>() % 100000000 + 2500000000;

    // Blend mode uses different versions
    let draft_version = if is_blend { "3.2.9" } else { models::draft_version(model_name) };
    let min_version = if is_blend { "3.2.9" } else { "3.0.2" };

    let ability_list_scene: Vec<serde_json::Value> = if is_blend {
        reference_image_uris.iter().map(|_| {
            serde_json::json!({
                "abilityName": "byte_edit",
                "strength": sample_strength,
                "source": {
                    "imageUrl": format!("blob:https://jimeng.jianying.com/{}", uuid::Uuid::new_v4())
                }
            })
        }).collect()
    } else {
        Vec::new()
    };

    let scene_option = serde_json::json!({
        "type": "image",
        "scene": "ImageBasicGenerate",
        "modelReqKey": model_name,
        "resolutionType": resolution_type,
        "abilityList": ability_list_scene,
        "reportParams": {
            "enterSource": "generate",
            "vipSource": "generate",
            "extraVipFunctionKey": format!("{model_name}-{resolution_type}"),
            "useVipFunctionDetailsReporterHoc": true
        }
    });

    let metrics_extra = serde_json::json!({
        "promptSource": "custom",
        "generateCount": 1,
        "enterFrom": "click",
        "sceneOptions": serde_json::json!([scene_option]).to_string(),
        "generateId": submit_id,
        "isRegenerate": false
    }).to_string();

    // Build abilities block: "generate" for text-to-image, "blend" for image-to-image
    let (generate_type, abilities) = if is_blend {
        let blend_prompt = format!("{}{}", "##".repeat(reference_image_uris.len()), prompt);

        let ability_list: Vec<serde_json::Value> = reference_image_uris.iter().map(|uri| {
            serde_json::json!({
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "name": "byte_edit",
                "image_uri_list": [uri],
                "image_list": [{
                    "type": "image", "id": uuid::Uuid::new_v4().to_string(),
                    "source_from": "upload", "platform_type": 1, "name": "",
                    "image_uri": uri, "width": 0, "height": 0,
                    "format": "", "uri": uri
                }],
                "strength": 0.5
            })
        }).collect();

        let placeholder_list: Vec<serde_json::Value> = (0..reference_image_uris.len()).map(|i| {
            serde_json::json!({
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "ability_index": i
            })
        }).collect();

        ("blend", serde_json::json!({
            "type": "", "id": uuid::Uuid::new_v4().to_string(),
            "blend": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "min_version": "3.2.9",
                "min_features": [],
                "core_param": {
                    "type": "", "id": uuid::Uuid::new_v4().to_string(),
                    "model": internal_model,
                    "prompt": blend_prompt,
                    "sample_strength": sample_strength,
                    "image_ratio": image_ratio,
                    "large_image_info": {
                        "type": "", "id": uuid::Uuid::new_v4().to_string(),
                        "height": height,
                        "width": width,
                        "resolution_type": resolution_type
                    },
                    "intelligent_ratio": false
                },
                "ability_list": ability_list,
                "prompt_placeholder_info_list": placeholder_list,
                "postedit_param": {
                    "type": "", "id": uuid::Uuid::new_v4().to_string(),
                    "generate_type": 0
                }
            }
        }))
    } else {
        ("generate", serde_json::json!({
            "type": "", "id": uuid::Uuid::new_v4().to_string(),
            "generate": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "core_param": {
                    "type": "", "id": uuid::Uuid::new_v4().to_string(),
                    "model": internal_model,
                    "prompt": prompt,
                    "negative_prompt": negative_prompt,
                    "seed": seed,
                    "sample_strength": sample_strength,
                    "image_ratio": image_ratio,
                    "large_image_info": {
                        "type": "", "id": uuid::Uuid::new_v4().to_string(),
                        "min_version": "3.0.2",
                        "height": height,
                        "width": width,
                        "resolution_type": resolution_type
                    },
                    "intelligent_ratio": false
                },
                "gen_option": {
                    "type": "", "id": uuid::Uuid::new_v4().to_string(),
                    "generate_all": false
                }
            }
        }))
    };

    let draft_content = serde_json::json!({
        "type": "draft",
        "id": uuid::Uuid::new_v4().to_string(),
        "min_version": min_version,
        "min_features": [],
        "is_from_tsn": true,
        "version": draft_version,
        "main_component_id": component_id,
        "component_list": [{
            "type": "image_base_component",
            "id": component_id,
            "min_version": "3.0.2",
            "aigc_mode": "workbench",
            "metadata": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "created_platform": 3,
                "created_platform_version": "",
                "created_time_in_ms": chrono::Utc::now().timestamp_millis().to_string(),
                "created_did": ""
            },
            "generate_type": generate_type,
            "abilities": abilities
        }]
    });

    let body = serde_json::json!({
        "extend": {
            "root_model": internal_model
        },
        "submit_id": submit_id,
        "metrics_extra": metrics_extra,
        "draft_content": draft_content.to_string(),
        "http_common_info": {
            "aid": auth::DEFAULT_ASSISTANT_ID
        }
    });

    let uri = "/mweb/v1/aigc_draft/generate";
    let headers = auth::build_headers_with_cookies(session_token, uri, cookie_jar);
    let params = auth::standard_query_params();

    let resp = client.post(format!("{JIMENG_BASE}{uri}"))
        .headers(headers)
        .query(&params)
        .json(&body)
        .send().await?;

    let status_code = resp.status();
    let text = resp.text().await?;

    if !status_code.is_success() {
        bail!("Image submit HTTP {status_code}: {}", &text[..text.len().min(500)]);
    }

    let payload: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Image submit parse error: {e}. Body: {}", &text[..text.len().min(500)]))?;

    if let Some(ret) = payload.get("ret") {
        let ret_num = ret.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| ret.as_i64()).unwrap_or(0);
        if ret_num != 0 {
            let errmsg = payload.get("errmsg").and_then(|v| v.as_str()).unwrap_or("unknown");
            bail!("Image submit failed [ret={ret_num}]: {errmsg}");
        }
    }

    let history_id = payload.pointer("/data/aigc_data/history_record_id")
        .or_else(|| payload.pointer("/aigc_data/history_record_id"))
        .or_else(|| payload.pointer("/data/history_record_id"));

    let history_id = match history_id {
        Some(v) => {
            if let Some(s) = v.as_str() { s.to_string() }
            else if let Some(n) = v.as_i64() { n.to_string() }
            else { bail!("Unexpected history_record_id type in image response") }
        }
        None => bail!("No history_record_id in image submit response"),
    };

    Ok(SubmitResult { history_record_id: history_id })
}

/// Build meta_list from prompt placeholders (@1, @2, @图1, @image1).
fn build_meta_list(prompt: &str, materials: &[UploadedMaterial]) -> Vec<serde_json::Value> {
    let mut meta_list = Vec::new();
    let material_count = materials.len();

    // Match @1, @2, @图1, @image1 etc.
    let re = regex::Regex::new(r"@(?:图|image)?(\d+)").unwrap();
    let mut last_end = 0;

    for cap in re.captures_iter(prompt) {
        let m = cap.get(0).unwrap();
        // Add text before placeholder
        if m.start() > last_end {
            let text = &prompt[last_end..m.start()];
            if !text.trim().is_empty() {
                meta_list.push(serde_json::json!({ "meta_type": "text", "text": text }));
            }
        }

        // Add material reference
        let idx: usize = cap[1].parse().unwrap_or(1);
        let material_idx = idx.saturating_sub(1);
        if material_idx < material_count {
            meta_list.push(serde_json::json!({
                "meta_type": materials[material_idx].material_type.as_str(),
                "text": "",
                "material_ref": { "material_idx": material_idx }
            }));
        }

        last_end = m.end();
    }

    // Remaining text
    if last_end < prompt.len() {
        let text = &prompt[last_end..];
        if !text.trim().is_empty() {
            meta_list.push(serde_json::json!({ "meta_type": "text", "text": text }));
        }
    }

    // If no placeholders found, build default references
    if meta_list.is_empty() {
        meta_list.push(serde_json::json!({ "meta_type": "text", "text": "使用" }));
        for (i, mat) in materials.iter().enumerate() {
            meta_list.push(serde_json::json!({
                "meta_type": mat.material_type.as_str(),
                "text": "",
                "material_ref": { "material_idx": i }
            }));
            if i < material_count - 1 {
                meta_list.push(serde_json::json!({ "meta_type": "text", "text": "和" }));
            }
        }
        if !prompt.trim().is_empty() {
            meta_list.push(serde_json::json!({ "meta_type": "text", "text": format!("素材，{prompt}") }));
        } else {
            meta_list.push(serde_json::json!({ "meta_type": "text", "text": "素材生成视频" }));
        }
    }

    meta_list
}

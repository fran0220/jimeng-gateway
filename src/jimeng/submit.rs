//! Task submission to jimeng.jianying.com API.
//! Regular videos use plain HTTP; Seedance models use browser proxy for a_bogus signing.

use anyhow::{bail, Result};
use reqwest::Client;

use super::auth;
use super::models::{self, UploadedMaterial, MaterialType};
use super::browser::BrowserService;

const JIMENG_BASE: &str = "https://jimeng.jianying.com";

/// Result of a video generation submission.
#[derive(Debug, Clone)]
pub struct SubmitResult {
    pub history_record_id: String,
}

/// Submit a regular (non-Seedance) video generation task.
pub async fn submit_regular_video(
    client: &Client,
    session_token: &str,
    prompt: &str,
    model_name: &str,
    width: u32,
    height: u32,
    duration: u32,
    resolution: &str,
    first_frame_uri: Option<&str>,
    end_frame_uri: Option<&str>,
) -> Result<SubmitResult> {
    let internal_model = models::resolve_model(model_name);
    let draft_version = models::draft_version(model_name);
    let aspect_ratio = models::aspect_ratio_str(width, height);

    let component_id = uuid::Uuid::new_v4().to_string();
    let submit_id = uuid::Uuid::new_v4().to_string();

    let metrics_extra = serde_json::json!({
        "enterFrom": "click",
        "isDefaultSeed": 1,
        "promptSource": "custom",
        "isRegenerate": false,
        "originSubmitId": submit_id,
    }).to_string();

    let first_frame = first_frame_uri.map(|uri| serde_json::json!({
        "format": "",
        "height": height,
        "id": uuid::Uuid::new_v4().to_string(),
        "image_uri": uri,
        "name": "",
        "platform_type": 1,
        "source_from": "upload",
        "type": "image",
        "uri": uri,
        "width": width,
    }));

    let end_frame = end_frame_uri.map(|uri| serde_json::json!({
        "format": "",
        "height": height,
        "id": uuid::Uuid::new_v4().to_string(),
        "image_uri": uri,
        "name": "",
        "platform_type": 1,
        "source_from": "upload",
        "type": "image",
        "uri": uri,
        "width": width,
    }));

    // If end_frame provided, use jimeng-video-3.0's internal model
    let root_model = if end_frame.is_some() {
        "dreamina_ic_generate_video_model_vgfm_3.0"
    } else {
        internal_model
    };

    let draft_content = serde_json::json!({
        "type": "draft",
        "id": uuid::Uuid::new_v4().to_string(),
        "min_version": "3.0.5",
        "is_from_tsn": true,
        "version": draft_version,
        "main_component_id": component_id,
        "component_list": [{
            "type": "video_base_component",
            "id": component_id,
            "min_version": "1.0.0",
            "metadata": {
                "type": "",
                "id": uuid::Uuid::new_v4().to_string(),
                "created_platform": 3,
                "created_platform_version": "",
                "created_time_in_ms": chrono::Utc::now().timestamp_millis(),
                "created_did": ""
            },
            "generate_type": "gen_video",
            "aigc_mode": "workbench",
            "abilities": {
                "type": "",
                "id": uuid::Uuid::new_v4().to_string(),
                "gen_video": {
                    "id": uuid::Uuid::new_v4().to_string(),
                    "type": "",
                    "text_to_video_params": {
                        "type": "",
                        "id": uuid::Uuid::new_v4().to_string(),
                        "model_req_key": internal_model,
                        "priority": 0,
                        "seed": rand::random::<u32>() % 100000000 + 2500000000,
                        "video_aspect_ratio": aspect_ratio,
                        "video_gen_inputs": [{
                            "duration_ms": duration * 1000,
                            "first_frame_image": first_frame,
                            "end_frame_image": end_frame,
                            "fps": 24,
                            "id": uuid::Uuid::new_v4().to_string(),
                            "min_version": "3.0.5",
                            "prompt": prompt,
                            "resolution": resolution,
                            "type": "",
                            "video_mode": 2
                        }]
                    },
                    "video_task_extra": metrics_extra,
                }
            }
        }],
    });

    let body = serde_json::json!({
        "extend": {
            "root_model": root_model,
            "m_video_commerce_info": {
                "benefit_type": "basic_video_operation_vgfm_v_three",
                "resource_id": "generate_video",
                "resource_id_type": "str",
                "resource_sub_type": "aigc"
            },
            "m_video_commerce_info_list": [{
                "benefit_type": "basic_video_operation_vgfm_v_three",
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

    let uri = "/mweb/v1/aigc_draft/generate";
    let headers = auth::build_headers(session_token, uri);
    let mut params = auth::standard_query_params();
    params.push(("da_version", draft_version.to_string()));

    let resp = client.post(format!("{JIMENG_BASE}{uri}"))
        .headers(headers)
        .query(&params)
        .json(&body)
        .send().await?;

    let _status = resp.status();
    let text = resp.text().await?;
    let payload: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Submit parse error: {e}. Body: {}", &text[..text.len().min(500)]))?;

    // Check for API-level error
    if let Some(ret) = payload.get("ret").and_then(|v| v.as_str().or_else(|| v.as_i64().map(|_| "")).and_then(|_| Some(v))) {
        let ret_num = ret.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| ret.as_i64()).unwrap_or(0);
        if ret_num != 0 {
            let errmsg = payload.get("errmsg").and_then(|v| v.as_str()).unwrap_or("unknown");
            bail!("Submit failed [ret={ret_num}]: {errmsg}");
        }
    }

    let history_id = payload.pointer("/data/aigc_data/history_record_id")
        .and_then(|v| v.as_str().or_else(|| v.as_i64().map(|_| "")).and_then(|_| Some(v)))
        .or_else(|| payload.pointer("/data/history_record_id"))
        .or_else(|| payload.pointer("/aigc_data/history_record_id"));

    let history_id = match history_id {
        Some(v) => {
            if let Some(s) = v.as_str() { s.to_string() }
            else if let Some(n) = v.as_i64() { n.to_string() }
            else { bail!("Unexpected history_record_id type: {v}") }
        }
        None => bail!("No history_record_id in submit response: {text}"),
    };

    Ok(SubmitResult { history_record_id: history_id })
}

/// Submit a Seedance video generation task via browser proxy.
pub async fn submit_seedance_video(
    _client: &Client,
    browser: &BrowserService,
    session_token: &str,
    prompt: &str,
    model_name: &str,
    width: u32,
    height: u32,
    duration: u32,
    materials: &[UploadedMaterial],
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

    // Build the full URL with query params
    let params: Vec<(String, String)> = vec![
        ("aid".into(), auth::DEFAULT_ASSISTANT_ID.to_string()),
        ("device_platform".into(), "web".into()),
        ("region".into(), "cn".into()),
        ("webId".into(), auth::standard_query_params().iter().find(|(k,_)| *k == "webId").map(|(_,v)| v.clone()).unwrap_or_default()),
        ("da_version".into(), draft_version.into()),
        ("web_component_open_flag".into(), "1".into()),
        ("web_version".into(), "7.5.0".into()),
        ("aigc_features".into(), "app_lip_sync".into()),
    ];
    let query_string = params.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("{JIMENG_BASE}/mweb/v1/aigc_draft/generate?{query_string}");

    // Send through browser proxy (for a_bogus injection)
    let result = browser.fetch(session_token, &url, &body.to_string()).await?;

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

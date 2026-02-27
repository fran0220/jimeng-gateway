//! Cookie generation and request signing for jimeng.jianying.com API.

use md5::{Md5, Digest};
use reqwest::header::{HeaderMap, HeaderValue};

/// Default assistant ID used in all API requests.
pub const DEFAULT_ASSISTANT_ID: u64 = 513695;
/// API version code.
const VERSION_CODE: &str = "8.4.0";
/// Platform code (web).
const PLATFORM_CODE: &str = "7";

lazy_static::lazy_static! {
    static ref DEVICE_ID: u64 = rand::random::<u64>() % 999999999999999999 + 7000000000000000000;
    static ref WEB_ID: u64 = rand::random::<u64>() % 999999999999999999 + 7000000000000000000;
    static ref USER_ID: String = uuid::Uuid::new_v4().to_string().replace("-", "");
}

/// Generate the Cookie header value for a given session token.
pub fn generate_cookie(session_token: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    format!(
        "_tea_web_id={web_id}; is_staff_user=false; store-region=cn-gd; store-region-src=uid; \
         sid_guard={token}%7C{ts}%7C5184000%7CMon%2C+03-Feb-2025+08%3A17%3A09+GMT; \
         uid_tt={uid}; uid_tt_ss={uid}; sid_tt={token}; sessionid={token}; sessionid_ss={token}",
        web_id = *WEB_ID,
        token = session_token,
        ts = now,
        uid = *USER_ID,
    )
}

/// Compute the Sign header: md5("9e2c|{uri_last7}|{platform}|{version}|{timestamp}||11ac")
pub fn compute_sign(uri: &str, timestamp: u64) -> String {
    let uri_suffix = if uri.len() >= 7 {
        &uri[uri.len() - 7..]
    } else {
        uri
    };

    let input = format!(
        "9e2c|{uri_suffix}|{platform}|{version}|{timestamp}||11ac",
        uri_suffix = uri_suffix,
        platform = PLATFORM_CODE,
        version = VERSION_CODE,
        timestamp = timestamp,
    );

    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Build the full set of fake browser headers for a jimeng API request.
pub fn build_headers(session_token: &str, uri: &str) -> HeaderMap {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let sign = compute_sign(uri, timestamp);
    let cookie = generate_cookie(session_token);

    let mut headers = HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_static("application/json, text/plain, */*"));
    headers.insert("Accept-Encoding", HeaderValue::from_static("gzip, deflate, br, zstd"));
    headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9"));
    headers.insert("App-Sdk-Version", HeaderValue::from_static("48.0.0"));
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
    headers.insert("Appid", HeaderValue::from_str(&DEFAULT_ASSISTANT_ID.to_string()).unwrap());
    headers.insert("Appvr", HeaderValue::from_static(VERSION_CODE));
    headers.insert("Lan", HeaderValue::from_static("zh-Hans"));
    headers.insert("Loc", HeaderValue::from_static("cn"));
    headers.insert("Origin", HeaderValue::from_static("https://jimeng.jianying.com"));
    headers.insert("Pragma", HeaderValue::from_static("no-cache"));
    headers.insert("Referer", HeaderValue::from_static("https://jimeng.jianying.com"));
    headers.insert("Pf", HeaderValue::from_static(PLATFORM_CODE));
    headers.insert("User-Agent", HeaderValue::from_static(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36"
    ));
    headers.insert("Cookie", HeaderValue::from_str(&cookie).unwrap());
    headers.insert("Device-Time", HeaderValue::from_str(&timestamp.to_string()).unwrap());
    headers.insert("Sign", HeaderValue::from_str(&sign).unwrap());
    headers.insert("Sign-Ver", HeaderValue::from_static("1"));

    headers
}

/// Standard query parameters appended to all jimeng API requests.
pub fn standard_query_params() -> Vec<(&'static str, String)> {
    vec![
        ("aid", DEFAULT_ASSISTANT_ID.to_string()),
        ("device_platform", "web".to_string()),
        ("region", "cn".to_string()),
        ("webId", WEB_ID.to_string()),
        ("da_version", "3.3.2".to_string()),
        ("web_component_open_flag", "1".to_string()),
        ("web_version", "7.5.0".to_string()),
        ("aigc_features", "app_lip_sync".to_string()),
    ]
}

/// Get cookies as (name, value, domain) tuples for browser context injection.
pub fn get_cookies_for_browser(session_token: &str) -> Vec<(&'static str, String, &'static str)> {
    let domain = ".jianying.com";
    vec![
        ("_tea_web_id", WEB_ID.to_string(), domain),
        ("is_staff_user", "false".to_string(), domain),
        ("store-region", "cn-gd".to_string(), domain),
        ("store-region-src", "uid".to_string(), domain),
        ("uid_tt", USER_ID.clone(), domain),
        ("uid_tt_ss", USER_ID.clone(), domain),
        ("sid_tt", session_token.to_string(), domain),
        ("sessionid", session_token.to_string(), domain),
        ("sessionid_ss", session_token.to_string(), domain),
    ]
}

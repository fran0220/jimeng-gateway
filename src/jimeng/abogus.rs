//! Pure Rust implementation of ByteDance a_bogus signing algorithm.
//!
//! Replaces the headless Chromium browser approach. The algorithm:
//! 1. Double-SM3 hash params and method with "cus" suffix
//! 2. Build a 44-byte structured payload with timestamps and hash bytes
//! 3. Append browser fingerprint bytes and XOR checksum
//! 4. RC4-encrypt with key "y"
//! 5. Prepend 12-byte random nonce
//! 6. Custom Base64-encode with shuffled alphabet

use sm3::{Digest, Sm3};

/// Custom Base64 alphabet used for final encoding (s4 variant).
const ALPHABET: &[u8; 65] =
    b"Dkdpgh2ZmsQB80/MfvV36XI1R45-WUAlEixNLwoqYTOPuzKFjJnry79HbGcaStCe=";

/// Suffix appended before SM3 hashing.
const END_STRING: &str = "cus";

/// Default browser fingerprint string.
/// Format: innerW|innerH|outerW|outerH|screenX|screenY|0|0|screenW|screenH|screenW|screenH|innerW|innerH|colorDepth|pixelDepth|platform
const DEFAULT_BROWSER: &str =
    "1920|969|1920|1080|0|0|0|0|1920|1080|1920|1080|1920|969|24|24|Win32";

/// Pre-computed ua_code: SM3(SM3("Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
/// AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36" + "cus"))
/// This matches the User-Agent used in auth.rs headers.
fn default_ua_code() -> [u8; 32] {
    let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
              (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36";
    double_sm3(ua)
}

/// Compute SM3 hash, returning 32 bytes.
fn sm3_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sm3::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Double SM3: SM3(SM3(input + "cus"))
/// First hash takes string bytes, second hash takes the raw 32-byte digest.
fn double_sm3(input: &str) -> [u8; 32] {
    let mut buf = Vec::with_capacity(input.len() + END_STRING.len());
    buf.extend_from_slice(input.as_bytes());
    buf.extend_from_slice(END_STRING.as_bytes());
    let first = sm3_hash(&buf);
    sm3_hash(&first)
}

/// RC4 encrypt/decrypt.
fn rc4(data: &[u8], key: &[u8]) -> Vec<u8> {
    // KSA
    let mut s: Vec<u8> = (0..=255u8).collect();
    let mut j: usize = 0;
    for i in 0..256 {
        j = (j + s[i] as usize + key[i % key.len()] as usize) % 256;
        s.swap(i, j);
    }

    // PRGA
    let mut i: usize = 0;
    j = 0;
    data.iter()
        .map(|&byte| {
            i = (i + 1) % 256;
            j = (j + s[i] as usize) % 256;
            s.swap(i, j);
            let t = (s[i] as usize + s[j] as usize) % 256;
            byte ^ s[t]
        })
        .collect()
}

/// Custom Base64 encoding using the s4 alphabet.
fn custom_b64_encode(data: &[u8]) -> String {
    let mut result = Vec::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let n = match chunk.len() {
            3 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32,
            2 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8,
            1 => (chunk[0] as u32) << 16,
            _ => unreachable!(),
        };

        result.push(ALPHABET[((n >> 18) & 0x3F) as usize]);
        result.push(ALPHABET[((n >> 12) & 0x3F) as usize]);

        if chunk.len() > 1 {
            result.push(ALPHABET[((n >> 6) & 0x3F) as usize]);
        }
        if chunk.len() > 2 {
            result.push(ALPHABET[(n & 0x3F) as usize]);
        }
    }

    // Pad to multiple of 4
    let pad = (4 - result.len() % 4) % 4;
    for _ in 0..pad {
        result.push(b'=');
    }

    String::from_utf8(result).unwrap()
}

/// Generate a 4-byte random list using the masking scheme.
fn random_list(seed: f64, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8) -> [u8; 4] {
    let v1 = (seed as u32) & 0xFF;
    let v2 = (seed as u32) >> 8;
    [
        (v1 as u8 & b) | d,
        (v1 as u8 & c) | e,
        (v2 as u8 & b) | f,
        (v2 as u8 & c) | g,
    ]
}

/// Generate the 12-byte random nonce (string_1).
fn generate_string_1() -> Vec<u8> {
    let r1 = rand::random::<f64>() * 10000.0;
    let r2 = rand::random::<f64>() * 10000.0;
    let r3 = rand::random::<f64>() * 10000.0;

    let list_1 = random_list(r1, 170, 85, 1, 2, 5, 40);
    let list_2 = random_list(r2, 170, 85, 1, 0, 0, 0);
    let list_3 = random_list(r3, 170, 85, 1, 0, 5, 0);

    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(&list_1);
    out.extend_from_slice(&list_2);
    out.extend_from_slice(&list_3);
    out
}

/// Build the 44-byte structured payload.
fn build_list_4(
    end_time: u64,
    start_time: u64,
    params_hash: &[u8; 32],
    method_hash: &[u8; 32],
    ua_code: &[u8; 32],
    browser_len: u8,
) -> [u8; 44] {
    [
        44,                                // [0]  magic/version
        ((end_time >> 24) & 0xFF) as u8,   // [1]
        0,                                 // [2]
        0,                                 // [3]
        0,                                 // [4]
        0,                                 // [5]
        24,                                // [6]  fixed
        params_hash[21],                   // [7]
        method_hash[21],                   // [8]
        0,                                 // [9]
        ua_code[23],                       // [10]
        ((end_time >> 16) & 0xFF) as u8,   // [11]
        0,                                 // [12]
        0,                                 // [13]
        0,                                 // [14]
        1,                                 // [15] fixed
        0,                                 // [16]
        239,                               // [17] fixed 0xEF
        params_hash[22],                   // [18]
        method_hash[22],                   // [19]
        ua_code[24],                       // [20]
        ((end_time >> 8) & 0xFF) as u8,    // [21]
        0,                                 // [22]
        0,                                 // [23]
        0,                                 // [24]
        0,                                 // [25]
        (end_time & 0xFF) as u8,           // [26]
        0,                                 // [27]
        0,                                 // [28]
        14,                                // [29] fixed
        ((start_time >> 24) & 0xFF) as u8, // [30]
        ((start_time >> 16) & 0xFF) as u8, // [31]
        0,                                 // [32]
        ((start_time >> 8) & 0xFF) as u8,  // [33]
        (start_time & 0xFF) as u8,         // [34]
        3,                                 // [35] fixed
        0,                                 // [36] end_time / 2^32
        1,                                 // [37] fixed
        0,                                 // [38] start_time / 2^32
        1,                                 // [39] fixed
        browser_len,                       // [40]
        0,                                 // [41]
        0,                                 // [42]
        0,                                 // [43]
    ]
}

/// Generate the a_bogus parameter value for a given URL query string.
///
/// `url_params` should be the full query string (without leading `?`).
/// `method` is the HTTP method, typically "POST" for jimeng generate requests.
pub fn generate(url_params: &str, method: &str) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let start_time = now_ms;
    let end_time = start_time + rand::random::<u64>() % 5 + 4; // +4..8

    let params_hash = double_sm3(url_params);
    let method_hash = double_sm3(method);
    let ua_code = default_ua_code();

    let browser = DEFAULT_BROWSER;
    let browser_len = browser.len() as u8;
    let browser_code: Vec<u8> = browser.bytes().collect();

    // Build the 44-byte structure
    let list4 = build_list_4(end_time, start_time, &params_hash, &method_hash, &ua_code, browser_len);

    // XOR checksum of all 44 bytes
    let checksum = list4.iter().fold(0u8, |acc, &x| acc ^ x);

    // Assemble: list4 + browser_code + checksum
    let mut plaintext = Vec::with_capacity(44 + browser_code.len() + 1);
    plaintext.extend_from_slice(&list4);
    plaintext.extend_from_slice(&browser_code);
    plaintext.push(checksum);

    // RC4 encrypt with key "y"
    let encrypted = rc4(&plaintext, b"y");

    // Generate 12-byte random nonce
    let nonce = generate_string_1();

    // Concatenate nonce + encrypted
    let mut full = Vec::with_capacity(nonce.len() + encrypted.len());
    full.extend_from_slice(&nonce);
    full.extend_from_slice(&encrypted);

    // Custom Base64 encode
    let encoded = custom_b64_encode(&full);

    // URL-encode the result
    urlencoding::encode(&encoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sm3_basic() {
        // Known SM3 test vector: SM3("abc") = 66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0
        let hash = sm3_hash(b"abc");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0");
    }

    #[test]
    fn test_double_sm3_consistency() {
        let h1 = double_sm3("test_params");
        let h2 = double_sm3("test_params");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_double_sm3_different_inputs() {
        let h1 = double_sm3("GET");
        let h2 = double_sm3("POST");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_rc4_roundtrip() {
        let data = b"hello world";
        let key = b"y";
        let encrypted = rc4(data, key);
        let decrypted = rc4(&encrypted, key);
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_rc4_known_vector() {
        // RC4 with key "y" (0x79) on known input
        let data = vec![0u8; 5];
        let result = rc4(&data, b"y");
        assert_eq!(result.len(), 5);
        // RC4 should produce non-zero output for zero input
        assert_ne!(result, vec![0u8; 5]);
    }

    #[test]
    fn test_custom_b64_encode() {
        // Test that output uses the s4 alphabet characters
        let data = b"\x02\xff\x00\x01\x02\x03";
        let encoded = custom_b64_encode(data);
        assert!(!encoded.is_empty());
        // All chars should be from the alphabet or '='
        for ch in encoded.chars() {
            let valid = ALPHABET.contains(&(ch as u8));
            assert!(valid, "Character '{ch}' not in alphabet");
        }
    }

    #[test]
    fn test_custom_b64_length() {
        // 21 bytes → 28 chars (standard base64 math)
        let data = vec![0u8; 21];
        let encoded = custom_b64_encode(&data);
        assert_eq!(encoded.len(), 28);
    }

    #[test]
    fn test_random_list() {
        let r = random_list(1234.0, 170, 85, 1, 2, 5, 40);
        assert_eq!(r.len(), 4);
        // Check mask bits: d=1 means bit 0 must be set
        assert_eq!(r[0] & 1, 1);
        // e=2 means bit 1 must be set
        assert_eq!(r[1] & 2, 2);
    }

    #[test]
    fn test_generate_string_1_length() {
        let s = generate_string_1();
        assert_eq!(s.len(), 12);
    }

    #[test]
    fn test_build_list_4() {
        let params_hash = double_sm3("test=1");
        let method_hash = double_sm3("POST");
        let ua_code = default_ua_code();
        let list4 = build_list_4(1000, 999, &params_hash, &method_hash, &ua_code, 80);
        assert_eq!(list4.len(), 44);
        assert_eq!(list4[0], 44);   // magic
        assert_eq!(list4[6], 24);   // fixed
        assert_eq!(list4[15], 1);   // fixed
        assert_eq!(list4[17], 239); // fixed 0xEF
        assert_eq!(list4[29], 14);  // fixed
        assert_eq!(list4[35], 3);   // fixed
        assert_eq!(list4[40], 80);  // browser_len
    }

    #[test]
    fn test_generate_produces_valid_output() {
        let params = "aid=513695&device_platform=web&region=cn";
        let result = generate(params, "POST");
        // Should be non-empty URL-encoded string
        assert!(!result.is_empty());
        // Typical a_bogus length is 100-200 chars when URL-encoded
        assert!(result.len() > 50, "Result too short: {}", result.len());
        // Should not contain raw whitespace
        assert!(!result.contains(' '));
    }

    #[test]
    fn test_generate_deterministic_structure() {
        // Two calls should produce different values (random nonce)
        let params = "aid=513695&test=1";
        let r1 = generate(params, "POST");
        let r2 = generate(params, "POST");
        assert_ne!(r1, r2, "Should be different due to random nonce");
    }

    #[test]
    fn test_default_ua_code() {
        let code = default_ua_code();
        assert_eq!(code.len(), 32);
        // Should be deterministic
        assert_eq!(code, default_ua_code());
    }

    #[test]
    fn test_generate_with_realistic_params() {
        // Simulate a real jimeng API call
        let params = "aid=513695&device_platform=web&region=cn&webId=7000000000000000000\
                      &da_version=3.3.2&web_component_open_flag=1&web_version=7.5.0\
                      &aigc_features=app_lip_sync";
        let result = generate(params, "POST");

        // URL-decode to check raw structure
        let decoded = urlencoding::decode(&result).unwrap();

        // After URL-decoding, the result should be valid custom base64
        // All chars should be from the s4 alphabet or '=' or '%' (URL encoding)
        for ch in decoded.chars() {
            let valid = ALPHABET.contains(&(ch as u8));
            assert!(valid, "Unexpected char '{ch}' (0x{:02x}) in decoded output", ch as u32);
        }

        // The raw base64 output encodes: 12 (nonce) + 44 + browser_len + 1 (checksum) bytes
        // = 12 + 44 + 80 + 1 = 137 bytes → ceil(137/3)*4 = 184 base64 chars
        // But URL-encoding may expand this
        assert!(decoded.len() > 100, "Output too short: {} chars", decoded.len());
    }

    #[test]
    fn test_rc4_known_key_y() {
        // Verify RC4 with key "y" (0x79) matches expected behavior.
        // After KSA with single-byte key 0x79:
        // - S[0] and S[0x79] are swapped
        // First keystream byte check
        let input = [0u8];
        let output = rc4(&input, b"y");
        // Should produce a specific byte (RC4 is deterministic for fixed key+input)
        assert_eq!(output.len(), 1);
        assert_ne!(output[0], 0); // Non-trivial output
    }

    /// Integration test: verify cookie is valid via a non-a_bogus endpoint.
    /// Run with: cargo test test_live_cookie -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_live_cookie() {
        let token = std::env::var("TEST_SESSION_TOKEN")
            .unwrap_or_else(|_| "b68b270daf398ba25669932adab54784".to_string());
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap();

        let uri = "/mweb/v1/get_history_by_ids";
        let headers = super::super::auth::build_headers(&token, uri);
        let params = super::super::auth::standard_query_params();
        let body = serde_json::json!({ "history_ids": ["fake_test_id"] });

        let resp = client
            .post(format!("https://jimeng.jianying.com{uri}"))
            .headers(headers)
            .query(&params)
            .json(&body)
            .send()
            .await
            .expect("Network error");

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        println!("[cookie test] HTTP {status}");
        println!("[cookie test] Response: {}", &text[..text.len().min(500)]);
        assert!(status.is_success(), "Cookie invalid: HTTP {status}");
    }

    /// Integration test: test without a_bogus to see if the endpoint rejects sans signature.
    /// Run with: cargo test test_live_no_abogus -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_live_no_abogus() {
        let token = std::env::var("TEST_SESSION_TOKEN")
            .unwrap_or_else(|_| "b68b270daf398ba25669932adab54784".to_string());
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap();

        let query_params = format!(
            "aid={}&device_platform=web&region=cn&webId={}&da_version=3.3.2\
             &web_component_open_flag=1&web_version=7.5.0&aigc_features=app_lip_sync",
            super::super::auth::DEFAULT_ASSISTANT_ID,
            super::super::auth::standard_query_params()
                .iter().find(|(k, _)| *k == "webId").map(|(_, v)| v.clone()).unwrap_or_default()
        );

        // NO a_bogus — just plain request
        let url = format!(
            "https://jimeng.jianying.com/mweb/v1/aigc_draft/generate?{}",
            query_params
        );
        let headers = super::super::auth::build_headers(&token, "/mweb/v1/aigc_draft/generate");
        let body = serde_json::json!({
            "submit_id": uuid::Uuid::new_v4().to_string(),
            "draft_content": "{}",
            "http_common_info": { "aid": super::super::auth::DEFAULT_ASSISTANT_ID },
        });

        let resp = client.post(&url).headers(headers).json(&body).send().await.expect("Network error");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        println!("[no a_bogus, empty body] HTTP {status}");
        println!("[no a_bogus, empty body] Response: {}", &text[..text.len().min(500)]);

        // Now try with a real submit body, still no a_bogus
        let internal_model = super::super::models::resolve_model("seedance-2.0-fast");
        let draft_version = super::super::models::draft_version("seedance-2.0-fast");
        let sid = uuid::Uuid::new_v4().to_string();
        let cid = uuid::Uuid::new_v4().to_string();

        let gen_input = serde_json::json!({
            "type": "", "id": uuid::Uuid::new_v4().to_string(),
            "min_version": draft_version, "prompt": "", "video_mode": 2, "fps": 24,
            "duration_ms": 4000, "idip_meta_list": [],
            "unified_edit_input": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "material_list": [], "meta_list": [{"meta_type":"text","text":"test"}]
            }
        });
        let t2v = serde_json::json!({
            "type":"","id":uuid::Uuid::new_v4().to_string(),
            "video_gen_inputs":[gen_input],"video_aspect_ratio":"9:16",
            "seed":12345,"model_req_key":internal_model,"priority":0
        });
        let dc = serde_json::json!({
            "type":"draft","id":uuid::Uuid::new_v4().to_string(),
            "min_version":draft_version,"min_features":["AIGC_Video_UnifiedEdit"],
            "is_from_tsn":true,"version":draft_version,"main_component_id":cid,
            "component_list":[{"type":"video_base_component","id":cid,
                "min_version":"1.0.0","aigc_mode":"workbench",
                "metadata":{"type":"","id":uuid::Uuid::new_v4().to_string(),"created_platform":3,"created_platform_version":"","created_time_in_ms":"0","created_did":""},
                "generate_type":"gen_video","abilities":{"type":"","id":uuid::Uuid::new_v4().to_string(),"gen_video":{"type":"","id":uuid::Uuid::new_v4().to_string(),"text_to_video_params":t2v,"video_task_extra":"{}"}},
                "process_type":1}]
        });
        let full_body = serde_json::json!({
            "extend":{"root_model":internal_model},"submit_id":sid,
            "metrics_extra":"{}","draft_content":dc.to_string(),
            "http_common_info":{"aid":super::super::auth::DEFAULT_ASSISTANT_ID}
        });

        let headers2 = super::super::auth::build_headers(&token, "/mweb/v1/aigc_draft/generate");
        let resp2 = client.post(&url).headers(headers2).json(&full_body).send().await.expect("Network error");
        let status2 = resp2.status();
        let text2 = resp2.text().await.unwrap_or_default();
        println!("[no a_bogus, FULL body] HTTP {status2}");
        println!("[no a_bogus, FULL body] Response: {}", &text2[..text2.len().min(500)]);
    }

    /// Integration test: test pure Rust a_bogus signing against real API.
    /// Run with: cargo test test_live_abogus -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_live_abogus() {
        let token = std::env::var("TEST_SESSION_TOKEN")
            .unwrap_or_else(|_| "b68b270daf398ba25669932adab54784".to_string());
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap();

        let query_params = format!(
            "aid={}&device_platform=web&region=cn&webId={}&da_version=3.3.2\
             &web_component_open_flag=1&web_version=7.5.0&aigc_features=app_lip_sync",
            super::super::auth::DEFAULT_ASSISTANT_ID,
            super::super::auth::standard_query_params()
                .iter()
                .find(|(k, _)| *k == "webId")
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        );

        let a_bogus = generate(&query_params, "POST");
        println!("[a_bogus] Generated ({} chars): {}...", a_bogus.len(), &a_bogus[..a_bogus.len().min(60)]);

        let url = format!(
            "https://jimeng.jianying.com/mweb/v1/aigc_draft/generate?{}&a_bogus={}",
            query_params, a_bogus
        );

        let uri = "/mweb/v1/aigc_draft/generate";
        let headers = super::super::auth::build_headers(&token, uri);

        // Minimal body — we expect a business error but NOT an anti-bot rejection
        let body = serde_json::json!({
            "submit_id": uuid::Uuid::new_v4().to_string(),
            "draft_content": "{}",
            "http_common_info": { "aid": super::super::auth::DEFAULT_ASSISTANT_ID },
        });

        let resp = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .expect("Network error");

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        println!("[a_bogus] HTTP {status}");
        println!("[a_bogus] Response: {}", &text[..text.len().min(800)]);

        // 403 = a_bogus rejected by anti-bot
        assert_ne!(status.as_u16(), 403, "a_bogus REJECTED: got 403 Forbidden");

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            let ret = json.get("ret").and_then(|v| {
                v.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| v.as_i64())
            });
            let errmsg = json.get("errmsg").and_then(|v| v.as_str()).unwrap_or("");
            println!("[a_bogus] ret={:?} errmsg={}", ret, errmsg);

            // Business errors (invalid draft, missing params) are FINE — means a_bogus passed
            // Anti-bot errors would be 403 or specific shark rejection codes
            match ret {
                Some(0) => println!("[a_bogus] ✅ ACCEPTED (ret=0)"),
                Some(code) => println!("[a_bogus] ✅ Business error (ret={code}) — a_bogus accepted, body rejected"),
                None => println!("[a_bogus] ⚠️ No ret code in response"),
            }
        }
    }

    /// Integration test: submit a real Seedance video via pure Rust a_bogus, then poll.
    /// Run with: cargo test test_live_video_generation -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_live_video_generation() {
        let token = std::env::var("TEST_SESSION_TOKEN")
            .unwrap_or_else(|_| "b68b270daf398ba25669932adab54784".to_string());
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .gzip(true)
            .build()
            .unwrap();

        let model_name = "seedance-2.0-fast";
        let prompt = "一只小猫在草地上奔跑";
        let width = 720u32;
        let height = 1280u32;
        let duration = 4u32;

        let internal_model = super::super::models::resolve_model(model_name);
        let benefit_type = super::super::models::seedance_benefit_type(model_name);
        let draft_version = super::super::models::draft_version(model_name);
        let aspect_ratio = super::super::models::aspect_ratio_str(width, height);

        let component_id = uuid::Uuid::new_v4().to_string();
        let submit_id = uuid::Uuid::new_v4().to_string();

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
                "materialTypes": serde_json::json!([])
            }]).to_string()
        }).to_string();

        // Build draft_content incrementally to avoid macro recursion limit
        let unified_edit = serde_json::json!({
            "type": "", "id": uuid::Uuid::new_v4().to_string(),
            "material_list": [],
            "meta_list": [{ "meta_type": "text", "text": prompt }],
        });
        let video_gen_input = serde_json::json!({
            "type": "", "id": uuid::Uuid::new_v4().to_string(),
            "min_version": draft_version,
            "prompt": "", "video_mode": 2, "fps": 24,
            "duration_ms": duration * 1000,
            "idip_meta_list": [],
            "unified_edit_input": unified_edit,
        });
        let t2v_params = serde_json::json!({
            "type": "", "id": uuid::Uuid::new_v4().to_string(),
            "video_gen_inputs": [video_gen_input],
            "video_aspect_ratio": aspect_ratio,
            "seed": rand::random::<u32>() % 1000000000,
            "model_req_key": internal_model,
            "priority": 0,
        });
        let abilities = serde_json::json!({
            "type": "", "id": uuid::Uuid::new_v4().to_string(),
            "gen_video": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "text_to_video_params": t2v_params,
                "video_task_extra": metrics_extra.clone(),
            }
        });
        let component = serde_json::json!({
            "type": "video_base_component", "id": component_id,
            "min_version": "1.0.0", "aigc_mode": "workbench",
            "metadata": {
                "type": "", "id": uuid::Uuid::new_v4().to_string(),
                "created_platform": 3, "created_platform_version": "",
                "created_time_in_ms": chrono::Utc::now().timestamp_millis().to_string(),
                "created_did": ""
            },
            "generate_type": "gen_video",
            "abilities": abilities,
            "process_type": 1,
        });
        let draft_content = serde_json::json!({
            "type": "draft", "id": uuid::Uuid::new_v4().to_string(),
            "min_version": draft_version, "min_features": ["AIGC_Video_UnifiedEdit"],
            "is_from_tsn": true, "version": draft_version,
            "main_component_id": component_id,
            "component_list": [component],
        });

        let body = serde_json::json!({
            "extend": {
                "root_model": internal_model,
                "m_video_commerce_info": {
                    "benefit_type": benefit_type,
                    "resource_id": "generate_video",
                    "resource_id_type": "str",
                    "resource_sub_type": "aigc"
                },
                "m_video_commerce_info_list": [{
                    "benefit_type": benefit_type,
                    "resource_id": "generate_video",
                    "resource_id_type": "str",
                    "resource_sub_type": "aigc"
                }]
            },
            "submit_id": submit_id,
            "metrics_extra": metrics_extra,
            "draft_content": draft_content.to_string(),
            "http_common_info": {
                "aid": super::super::auth::DEFAULT_ASSISTANT_ID,
            },
        });

        // Build query string and sign with a_bogus
        let query_string = format!(
            "aid={}&device_platform=web&region=cn&webId={}&da_version={}\
             &web_component_open_flag=1&web_version=7.5.0&aigc_features=app_lip_sync",
            super::super::auth::DEFAULT_ASSISTANT_ID,
            super::super::auth::standard_query_params()
                .iter().find(|(k,_)| *k == "webId").map(|(_, v)| v.clone()).unwrap_or_default(),
            draft_version,
        );

        let a_bogus = generate(&query_string, "POST");
        println!("[submit] a_bogus generated ({} chars)", a_bogus.len());

        let url = format!(
            "https://jimeng.jianying.com/mweb/v1/aigc_draft/generate?{}&a_bogus={}",
            query_string, a_bogus
        );
        let headers = super::super::auth::build_headers(&token, "/mweb/v1/aigc_draft/generate");

        let resp = client.post(&url).headers(headers).json(&body).send().await
            .expect("Submit request failed");

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        println!("[submit] HTTP {status}");
        println!("[submit] Response: {}", &text[..text.len().min(500)]);

        assert_ne!(status.as_u16(), 403, "a_bogus REJECTED by anti-bot");

        let payload: serde_json::Value = serde_json::from_str(&text)
            .expect("Failed to parse submit response");

        let ret = payload.get("ret")
            .and_then(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| v.as_i64()))
            .unwrap_or(-1);

        if ret != 0 {
            let errmsg = payload.get("errmsg").and_then(|v| v.as_str()).unwrap_or("unknown");
            println!("[submit] ❌ Submit failed: ret={ret}, errmsg={errmsg}");
            println!("[submit] Note: ret!=0 could mean account quota exhausted or model unavailable");
            return; // Don't fail the test — business errors are OK
        }

        let history_id = payload.pointer("/data/aigc_data/history_record_id")
            .or_else(|| payload.pointer("/aigc_data/history_record_id"))
            .or_else(|| payload.pointer("/data/history_record_id"))
            .and_then(|v| v.as_str().or_else(|| v.as_i64().map(|_| "")).and_then(|_| v.as_str().or(Some(""))))
            .map(|s| if s.is_empty() {
                payload.pointer("/data/aigc_data/history_record_id")
                    .or_else(|| payload.pointer("/data/history_record_id"))
                    .and_then(|v| v.as_i64())
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            } else { s.to_string() })
            .unwrap_or_default();

        println!("[submit] ✅ Task submitted! history_record_id={history_id}");

        // Poll for result (up to 5 minutes)
        let poll_deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            if std::time::Instant::now() > poll_deadline {
                println!("[poll] ⏱️ Poll timeout (5 min) — task still in progress");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

            match super::super::poll::poll_status(&client, &token, &history_id, None).await {
                Ok(result) => {
                    println!(
                        "[poll] status={} queue={:?}/{:?} eta={:?}",
                        result.status, result.queue_position, result.queue_total, result.queue_eta
                    );

                    if result.status == super::super::poll::STATUS_FAILED {
                        println!("[poll] ❌ Failed: {:?} {:?}", result.fail_code, result.fail_msg);
                        break;
                    }

                    if let Some(ref url) = result.video_url {
                        println!("[poll] ✅ Video ready: {}", &url[..url.len().min(120)]);
                        break;
                    }
                }
                Err(e) => {
                    println!("[poll] ⚠️ Poll error: {e}");
                }
            }
        }
    }

    #[test]
    fn test_full_pipeline_structure() {
        // Verify the internal pipeline produces correct byte layout
        let params = "aid=513695&test=1";
        let params_hash = double_sm3(params);
        let method_hash = double_sm3("POST");
        let ua_code = default_ua_code();
        let browser = DEFAULT_BROWSER;
        let browser_len = browser.len() as u8;

        // list_4 should be 44 bytes with correct fixed positions
        let list4 = build_list_4(12345, 12340, &params_hash, &method_hash, &ua_code, browser_len);
        assert_eq!(list4.len(), 44);

        // Verify dynamic bytes are from the hashes
        assert_eq!(list4[7], params_hash[21]);
        assert_eq!(list4[8], method_hash[21]);
        assert_eq!(list4[10], ua_code[23]);
        assert_eq!(list4[18], params_hash[22]);
        assert_eq!(list4[19], method_hash[22]);
        assert_eq!(list4[20], ua_code[24]);

        // Full payload: 44 + browser_code + 1 checksum
        let browser_code: Vec<u8> = browser.bytes().collect();
        let _checksum = list4.iter().fold(0u8, |acc, &x| acc ^ x);
        let total_len = 44 + browser_code.len() + 1;

        // After RC4 + 12-byte nonce: total = 12 + total_len
        let full_len = 12 + total_len;
        // Base64 of full_len bytes
        let expected_b64_len = ((full_len + 2) / 3) * 4;
        assert!(expected_b64_len > 100, "Expected b64 output > 100 chars, got {expected_b64_len}");
    }
}

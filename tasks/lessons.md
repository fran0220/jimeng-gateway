# Lessons

## 2026-04-24: ByteDance 4013 Risk Control (jimeng.jianying.com)

The 4013 "web_risk_control_message_reject_generation" error has THREE root causes:

1. **TLS fingerprint**: reqwest (rustls/hyper) has different JA3/JA4 than Chrome. Use system curl subprocess for jimeng API calls when full cookie jar is available. curl's OpenSSL has an accepted fingerprint.

2. **webId consistency**: The `webId` query parameter MUST match `_tea_web_id` from the cookie jar. Random webId triggers risk control. Use `standard_query_params_with_jar(cookie_jar)` to extract and match it.

3. **Model access control**: Using paid model keys (e.g., `dreamina_seedance_40_pro`) on accounts without paid subscriptions triggers 4013 from non-browser clients. Free model keys (e.g., `seedance_2_0_lite`) work fine. Always verify model → internal key mapping.

Related findings:
- ByteDance validates UA consistency with cookie fingerprint — don't send Chrome/132 UA with Chrome/147 cookies
- reqwest adds default `User-Agent: reqwest/x.y.z` — suppress with `.user_agent("")`
- Auto decompression (`.gzip(true)`) adds `Accept-Encoding` which conflicts with fingerprint
- The `ret` code in HTTP 200 responses is the real error; HTTP status is often 200 even on failures

## 2026-02-27

- 对外接入文档禁止包含任何管理员账户、密码、内部令牌等敏感凭据。
- 第三方文档仅保留调用方必需信息（Base URL、API Key 用法、请求示例、错误排查）。

---
name: jimeng-gateway-api
description: "Integrates with Jimeng Gateway for Seedance 2.0 video generation and Jimeng 5.0 image generation. Use when creating video/image generation tasks, polling task status, handling upload materials, managing API keys, or troubleshooting content moderation errors. Triggers on: jimeng, seedance, video generation, generate video, image generation, generate image."
---

# Jimeng Gateway API Integration

Jimeng Gateway proxies video and image generation requests to jimeng.jianying.com with async task queue, headless Chromium a_bogus signing (video only), and multi-layer content moderation awareness.

## Quick Reference

| Item | Value |
|------|-------|
| Base URL | `http://185.200.65.233:5100` |
| Auth | `Authorization: Bearer <API_KEY>` |
| Key format | `gw_` + 32 hex chars (35 total) |
| Models endpoint | `GET /v1/models` |
| Video mode | Async: submit → poll → get result |
| Image mode | Sync: submit → wait → get URLs (OpenAI compatible) |
| Video generation time | 2–5 minutes typical |
| Image generation time | 20–40 seconds typical |

## Available Models

| Model | Type | Speed | Quality |
|-------|------|-------|---------|
| `seedance-2.0` / `seedance-2.0-pro` | Video | Slower | High |
| `seedance-2.0-fast` | Video | Fast | Standard |
| `jimeng-5.0` | Image | ~30s | High |

Alias `jimeng-video-seedance-2.0` maps to `seedance-2.0`.

## Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/ping` | Health check (no auth) |
| `GET` | `/v1/models` | List available models |
| `POST` | `/v1/images/generations` | Create image (sync, OpenAI format) |
| `POST` | `/v1/videos/generations` | Create video task (async) |
| `GET` | `/api/v1/tasks` | List tasks (`?status=failed&limit=10`) |
| `GET` | `/api/v1/tasks/{id}` | Get single task |
| `POST` | `/api/v1/tasks/{id}/cancel` | Cancel (queued/submitting/polling only) |
| `POST` | `/api/v1/tasks/{id}/retry` | Retry with original params |
| `GET` | `/api/v1/stats` | Task statistics |

---

## Image Generation (OpenAI Compatible)

### `POST /v1/images/generations`

Synchronous endpoint — waits for generation to complete and returns image URLs directly. Fully compatible with OpenAI's image generation API format.

**Request (OpenAI standard):**

```bash
curl -X POST 'http://185.200.65.233:5100/v1/images/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -H 'Content-Type: application/json' \
  -d '{
    "prompt": "一只戴着墨镜的橘猫，坐在海边沙滩上，日落背景",
    "model": "jimeng-5.0",
    "size": "1024x1024"
  }'
```

**Request (extended control — ratio + resolution):**

```bash
curl -X POST 'http://185.200.65.233:5100/v1/images/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -H 'Content-Type: application/json' \
  -d '{
    "prompt": "赛博朋克风格的城市夜景，霓虹灯倒映在雨后的街道上",
    "model": "jimeng-5.0",
    "ratio": "16:9",
    "resolution": "2k"
  }'
```

**Request (image-to-image with reference image — multipart):**

```bash
curl -X POST 'http://185.200.65.233:5100/v1/images/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -F 'prompt=把这只猫放到下雪的街道上，保持猫的样子不变' \
  -F 'model=jimeng-5.0' \
  -F 'ratio=1:1' \
  -F 'resolution=2k' \
  -F 'files=@/path/to/reference.png'
```

**Request (multiple reference images):**

```bash
curl -X POST 'http://185.200.65.233:5100/v1/images/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -F 'prompt=把第一张图中的人物放到第二张图的场景中' \
  -F 'model=jimeng-5.0' \
  -F 'files=@/path/to/character.png' \
  -F 'files=@/path/to/background.jpg'
```

> When reference images are uploaded, the gateway automatically switches to **blend mode** (image-to-image). The generated images will incorporate the visual elements from reference images while following the prompt description. Subject appearance, pose, and key features are preserved.

**Response (OpenAI format, HTTP 200):**

```json
{
  "created": 1773733933,
  "data": [
    { "url": "https://p26-dreamina-sign.byteimg.com/...", "revised_prompt": "..." },
    { "url": "https://p3-dreamina-sign.byteimg.com/...", "revised_prompt": "..." },
    { "url": "https://p26-dreamina-sign.byteimg.com/...", "revised_prompt": "..." },
    { "url": "https://p26-dreamina-sign.byteimg.com/...", "revised_prompt": "..." }
  ]
}
```

> Each request generates 4 images by default.

### Image Parameters

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `prompt` | string | ✅ | — | Image description |
| `model` | string | No | `jimeng-5.0` | Model name |
| `size` | string | No | — | OpenAI format: `"WIDTHxHEIGHT"` (e.g. `"1024x1024"`) |
| `ratio` | string | No | `1:1` | Aspect ratio (overrides `size`) |
| `resolution` | string | No | `2k` | Resolution tier: `1k`, `2k`, `4k` (overrides `size`) |
| `files` | file[] | No | — | Reference images for blend mode (multipart only) |

> `ratio` + `resolution` take priority over `size` when both are provided.

### Generation Modes

| Mode | Trigger | Use case |
|------|---------|----------|
| **Text-to-image** | No files uploaded | Generate from prompt only |
| **Image-to-image (blend)** | One or more `files` uploaded | Transform/remix reference images with prompt guidance |

**Blend mode behavior:**
- Preserves visual elements (subject appearance, pose, features) from reference images
- Applies prompt-described transformations (scene change, style transfer, etc.)
- Supports multiple reference images (e.g. combine character from one image with scene from another)
- Automatically switches mode when files are detected — no extra parameters needed

### Supported Sizes

**Using `size` field (exact pixel match required):**

| Tier | 1:1 | 4:3 | 3:4 | 16:9 | 9:16 | 3:2 | 2:3 | 21:9 |
|------|-----|-----|-----|------|------|-----|-----|------|
| 1k | 1024x1024 | 768x1024 | 1024x768 | 1024x576 | 576x1024 | 1024x682 | 682x1024 | 1195x512 |
| 2k | 2048x2048 | 2304x1728 | 1728x2304 | 2560x1440 | 1440x2560 | 2496x1664 | 1664x2496 | 3024x1296 |
| 4k | 4096x4096 | 4608x3456 | 3456x4608 | 5120x2880 | 2880x5120 | 4992x3328 | 3328x4992 | 6048x2592 |

**Using `ratio` + `resolution` fields:**

- `ratio`: `1:1`, `4:3`, `3:4`, `16:9`, `9:16`, `3:2`, `2:3`, `21:9`
- `resolution`: `1k`, `2k`, `4k`

### Image Error Responses (OpenAI format)

```json
{
  "error": {
    "message": "Unsupported size: \"999x999\". Supported sizes: ...",
    "type": "invalid_request_error",
    "code": "invalid_size"
  }
}
```

| HTTP Code | `type` | Meaning |
|-----------|--------|---------|
| 400 | `invalid_request_error` | Bad params or content policy violation |
| 401 | `authentication_error` | Auth/account error |
| 429 | `rate_limit_exceeded` | Quota exceeded |
| 500 | `server_error` | Internal error |
| 504 | `timeout_error` | Generation timed out |

### Python Examples

**Text-to-image (OpenAI SDK):**

```python
from openai import OpenAI

client = OpenAI(
    api_key="gw_xxx",
    base_url="http://185.200.65.233:5100/v1"
)

response = client.images.generate(
    model="jimeng-5.0",
    prompt="一只戴着墨镜的橘猫，坐在海边沙滩上",
    size="1024x1024",
)

for img in response.data:
    print(img.url)
```

**Image-to-image with reference (requests):**

```python
import requests

BASE = "http://185.200.65.233:5100"
KEY = "gw_xxx"

response = requests.post(
    f"{BASE}/v1/images/generations",
    headers={"Authorization": f"Bearer {KEY}"},
    files=[("files", ("ref.png", open("reference.png", "rb"), "image/png"))],
    data={
        "prompt": "把这只猫放到雪景中，保持猫的样子不变",
        "model": "jimeng-5.0",
        "ratio": "1:1",
        "resolution": "2k",
    },
)

for img in response.json()["data"]:
    print(img["url"])
```

---

## Video Generation (Async)

### `POST /v1/videos/generations`

Asynchronous endpoint — returns task ID immediately, poll for results.

**JSON (text-to-video):**

```bash
curl -X POST 'http://185.200.65.233:5100/v1/videos/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -H 'Content-Type: application/json' \
  -d '{
    "prompt": "一只金色的柴犬在樱花树下奔跑，慢动作，电影感光影",
    "model": "seedance-2.0-fast",
    "duration": 5,
    "ratio": "16:9"
  }'
```

**Multipart (with material upload):**

```bash
curl -X POST 'http://185.200.65.233:5100/v1/videos/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -F 'prompt=让@1中的人物缓慢转头微笑，电影感光影' \
  -F 'model=seedance-2.0' \
  -F 'duration=5' \
  -F 'ratio=16:9' \
  -F 'files=@/path/to/image.png'
```

**Response (HTTP 202):**

```json
{
  "code": 0,
  "message": "Task queued",
  "task": {
    "id": "<task_id>",
    "status": "queued",
    "poll_url": "/api/v1/tasks/<task_id>"
  }
}
```

### Poll Status

```bash
curl 'http://185.200.65.233:5100/api/v1/tasks/<task_id>' \
  -H 'Authorization: Bearer <API_KEY>'
```

**Polling strategy:** Wait 3s initially, then poll every 5s, max 10 minutes.

**Status lifecycle:**

```
queued → submitting → polling → downloading → succeeded
              ↘            ↘                ↘
               → failed     → failed / cancelled → failed
```

| Status | Meaning |
|--------|---------|
| `queued` | Waiting in queue |
| `submitting` | Uploading materials + submitting to upstream |
| `polling` | Submitted, waiting for upstream generation |
| `downloading` | Getting HQ video URL |
| `succeeded` | Done — read `video_url` |
| `failed` | Error — read `error_message` + `error_kind` |
| `cancelled` | Cancelled by user |

During `polling`, queue progress fields are available: `queue_position`, `queue_total`, `queue_eta`.

### Video Parameters

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `prompt` | string | ✅ | — | Video description |
| `model` | string | No | `seedance-2.0` | Model name |
| `duration` | int | No | `4` | Video length in seconds (4, 5, 10) |
| `ratio` | string | No | `9:16` | Aspect ratio |
| `files` | file[] | No | — | Material files (multipart only) |

### Video Ratios (all at 720p)

| Ratio | Resolution | Use case |
|-------|-----------|----------|
| `1:1` | 720×720 | Square, social media |
| `4:3` | 960×720 | Traditional |
| `3:4` | 720×960 | Portrait moderate |
| `16:9` | 1280×720 | Landscape, YouTube |
| `9:16` | 720×1280 | Vertical, TikTok |

### Material Upload & References

| Type | Formats | Channel |
|------|---------|---------|
| Image | JPG, PNG, WebP, GIF, BMP | ImageX |
| Video | MP4, MOV, M4V | VOD |
| Audio | MP3, WAV | VOD |

Reference uploaded materials in prompt:
- `@1`, `@2` — by upload order
- `@图1`, `@image1` — alternative syntax

If no placeholders, system auto-generates default references.

### Python Example (Video)

```python
import time, requests

BASE = "http://185.200.65.233:5100"
KEY = "<API_KEY>"
HEADERS = {"Authorization": f"Bearer {KEY}"}

# 1. Create task
resp = requests.post(f"{BASE}/v1/videos/generations",
    headers={**HEADERS, "Content-Type": "application/json"},
    json={"prompt": "一只橘猫在阳光下打哈欠", "model": "seedance-2.0-fast",
          "duration": 5, "ratio": "16:9"})
task_id = resp.json()["task"]["id"]
print(f"Task: {task_id}")

# 2. Poll
time.sleep(3)
for _ in range(120):
    task = requests.get(f"{BASE}/api/v1/tasks/{task_id}", headers=HEADERS).json()["task"]
    status = task["status"]
    if status == "succeeded":
        print(f"Video: {task['video_url']}")
        break
    elif status == "failed":
        print(f"Error [{task['error_kind']}]: {task['error_message']}")
        break
    elif status == "cancelled":
        print("Cancelled"); break
    if task.get("queue_eta"):
        print(f"  Queue: {task['queue_position']}/{task['queue_total']} ETA: {task['queue_eta']}")
    time.sleep(5)
```

---

## Content Moderation (Three Layers)

> ⚠️ Upstream enforces server-side moderation. Gateway CANNOT bypass it.

```
Prompt + Materials
    ↓
[L1: Input Text] — keyword/name matching, instant block
    ↓ (pass)
[L2: Input Image] — AI face/copyright detection
    ↓ (pass)
[Generation]
    ↓
[L3: Output Check] — AI visual recognition on generated result
    ↓
Success or Block
```

### L1: Input Text Check
- Blocks real person names (Taylor Swift, 成龙), IP characters (Spider-Man)
- Error keys: `web_fail2generate_input_retry`, `web_text_violates_community_guidelines_toast`

### L2: Input Image Check
- Blocks recognizable faces, copyright content in uploads
- Error keys: `inputimagerisk`, `web_image_violates_community_guidelines_toast`

### L3: Output Check
- AI visual scan on generated content, catches indirect descriptions
- Realistic styles all trigger L3 for protected subjects
- Error keys: `ErrMessage_APP_OutputVideoRisk`, `outputvideorisk`, `outputimagerisk`

> Parameters like `safe_check: 0` do NOT work — moderation is entirely server-side.

## Error Handling

### HTTP errors

| Code | Meaning | Action |
|------|---------|--------|
| `401` | Auth failure | Check API key |
| `403` | Forbidden / key disabled | Contact admin |
| `429` | Rate limited | Wait for `X-RateLimit-Reset` seconds |
| `404` | Task not found | Check task_id |

### Task error_kind classification

| error_kind | Retryable | Action |
|-----------|-----------|--------|
| `content_risk` | ⚠️ Modify prompt | Don't blind retry |
| `timeout` | ✅ Direct retry | |
| `generation_failed` | ✅ Direct retry | |
| `network` | ✅ Wait 10s | |
| `quota` | ⏰ Next day | |
| `auth` | ❌ No | Wait for admin |
| `account_blocked` | ❌ No | Wait for admin |

## Rate Limiting & Quotas

Response headers on every request:

| Header | Description |
|--------|-------------|
| `X-RateLimit-Limit` | Requests per minute allowed |
| `X-RateLimit-Remaining` | Remaining requests |
| `X-RateLimit-Reset` | Seconds until reset |

Daily quota exceeded returns HTTP 429:

```json
{"error": {"message": "Daily quota exceeded", "type": "rate_limit_error", "code": "daily_quota_exceeded"}}
```

## Architecture Notes

- **a_bogus signing**: Video generation requires ByteDance `bdms` anti-crawl signature via headless Chromium. Image generation uses direct HTTP (no browser needed).
- **Session pool**: Multiple upstream sessions with LRU rotation and health checks. Unhealthy sessions are automatically marked.
- **Worker queue**: Configurable concurrency via `CONCURRENCY` env var. Tasks auto-requeue when no sessions available.
- **Upload channels**: Images → ImageX (bytedanceapi.com), Video/Audio → VOD (bytedanceapi.com). Both use AWS4-HMAC-SHA256 signing.

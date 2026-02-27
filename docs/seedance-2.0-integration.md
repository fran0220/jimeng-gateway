# Jimeng Gateway 第三方接入（Seedance 2.0 精简版）

本文档面向第三方调用方，仅包含对接所需的最小信息。

## 1. 接入信息

- Base URL：`http://185.200.65.233:5100`
- 鉴权方式：`Authorization: Bearer <API_KEY>`
- 由平台方提供：
  - `API_KEY`

## 2. 核心接口

- 生成任务：`POST /v1/videos/generations`
- 查询任务：`GET /api/v1/tasks/{task_id}`
- 健康检查：`GET /ping`

## 3. 图片生成视频（multipart 推荐）

Seedance 2.0 需要至少一个素材文件（图片/视频/音频）。

```bash
curl -X POST 'http://185.200.65.233:5100/v1/videos/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -F 'model=jimeng-video-seedance-2.0' \
  -F 'prompt=让@1中的主体缓慢转头微笑，电影感光影' \
  -F 'duration=5' \
  -F 'ratio=16:9' \
  -F 'files=@/absolute/path/to/image.png'
```

参数说明：

- `model`：推荐 `jimeng-video-seedance-2.0`（兼容 `seedance-2.0`）
- `prompt`：可用 `@1`、`@2` 引用素材
- `duration`：视频时长（秒）
- `ratio`：例如 `16:9`、`9:16`
- `files`：素材文件，可重复传多个

## 4. URL 素材（JSON 可选）

```bash
curl -X POST 'http://185.200.65.233:5100/v1/videos/generations' \
  -H 'Authorization: Bearer <API_KEY>' \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "jimeng-video-seedance-2.0",
    "prompt": "让图中人物挥手",
    "duration": 5,
    "ratio": "16:9",
    "file_paths": ["https://example.com/image1.jpg"]
  }'
```

## 5. 异步任务与轮询

创建后会返回 `202` 和 `task.id`：

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

轮询：

```bash
curl 'http://185.200.65.233:5100/api/v1/tasks/<task_id>'
```

典型状态：`queued -> submitting -> polling -> downloading -> succeeded`

成功时从 `task.video_url` 获取结果地址。

## 6. 常见错误

- `401 Invalid API key`
  - key 无效/禁用/请求头格式错误

- `submit failed [-2001] Seedance 2.0 需要至少一个文件`
  - 未提供素材文件或素材 URL 无效

- `submit 接口当前仅支持 Seedance 模型`
  - `model` 非 Seedance，请改为 `jimeng-video-seedance-2.0` 或 `seedance-2.0`

## 7. 联调最小检查

- `GET /ping` 返回 `pong`
- `POST /v1/videos/generations` 返回 `202` 与 `task.id`
- `GET /api/v1/tasks/{id}` 状态能进入 `polling`

如无法通过以上检查，请联系平台方排查 key 状态与后端 session 池可用性。

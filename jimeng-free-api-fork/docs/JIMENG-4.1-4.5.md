# 即梦 AI 4.1 和 4.5 模型支持

本文档介绍了 jimeng-free-api-all 项目对即梦 AI 新增的 4.1 和 4.5 模型的支持。

## 新增模型

### jimeng-4.5
- **内部模型名**: `high_aes_general_v45`
- **版本**: 3.2.9
- **特性**:
  - 支持文生图（Text-to-Image）
  - 支持图生图（Image-to-Image）
  - 支持多图连续生成
  - 最高支持 2048x2048 分辨率

### jimeng-4.1
- **内部模型名**: `high_aes_general_v41`
- **版本**: 3.2.9
- **特性**:
  - 支持文生图（Text-to-Image）
  - 支持图生图（Image-to-Image）
  - 支持多图连续生成
  - 最高支持 2048x2048 分辨率

## 与旧版本的差异

| 特性 | jimeng-4.5/4.1 | jimeng-4.0 | jimeng-3.1 |
|------|----------------|------------|------------|
| Draft版本 | 3.2.9 | 3.0.2 | 3.0.2 |
| 多图生成 | ✅ | ✅ | ❌ |
| 最大分辨率 | 2048x2048 | 2048x2048 | 1024x1024 |
| 采样强度范围 | 0.1-1.0 | 0.1-1.0 | 0.1-0.8 |

## API 使用

### 1. 文生图

```bash
curl -X POST http://localhost:8000/v1/images/generations \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_SESSION_ID" \
  -d '{
    "model": "jimeng-4.5",
    "prompt": "一只可爱的小猫在花园里玩耍",
    "width": 1024,
    "height": 1024,
    "sample_strength": 0.7
  }'
```

### 2. 多图连续生成

```bash
curl -X POST http://localhost:8000/v1/images/generations \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_SESSION_ID" \
  -d '{
    "model": "jimeng-4.5",
    "prompt": "生成4张连续场景的图片：春夏秋冬四季风景",
    "width": 1024,
    "height": 1024,
    "sample_strength": 0.6
  }'
```

### 3. 图生图

```bash
curl -X POST http://localhost:8000/v1/images/compositions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_SESSION_ID" \
  -d '{
    "model": "jimeng-4.1",
    "prompt": "将这些图片合成为一幅美丽的风景画",
    "images": [
      "https://example.com/image1.jpg",
      "https://example.com/image2.jpg"
    ],
    "width": 2560,
    "height": 1440,
    "sample_strength": 0.6
  }'
```

## 参数说明

### 支持的分辨率

- 512x512
- 768x768
- 1024x1024（默认）
- 1280x720
- 720x1280
- 1536x864
- 864x1536
- 2048x2048（仅 4.1 和 4.5）

### 采样强度（sample_strength）

- 范围：0.1 - 1.0
- 默认：0.5
- 说明：控制生成图片与提示词的契合度，值越高越贴近提示词

## 最佳实践

1. **使用提示词**
   - 4.5 和 4.1 版本对中文提示词支持更好
   - 建议使用详细的描述性提示词

2. **多图生成**
   - 使用 "连续"、"绘本"、"故事" 等关键词触发多图生成
   - 使用 "X张" 指定生成图片数量

3. **分辨率选择**
   - 普通场景使用 1024x1024
   - 需要高清细节时使用 2048x2048
   - 宽屏场景使用 1280x720

## 测试

项目提供了测试脚本 `test/test-new-models.js`：

```bash
# 安装依赖
npm install

# 修改脚本中的 SESSION_ID
vim test/test-new-models.js

# 运行测试
npm run test:models
# 或直接运行
node test/test-new-models.js
```

## 注意事项

1. 新模型需要更多的积分消耗
2. 生成时间可能比旧模型稍长
3. 建议在生产环境使用前充分测试
4. 遵守即梦 AI 的使用条款和限制

## 故障排除

### 常见错误

1. **参数验证失败**
   - 检查分辨率是否在支持列表中
   - 检查采样强度是否在 0.1-1.0 范围内

2. **模型不支持**
   - 确保使用的是正确的模型名称（jimeng-4.5 或 jimeng-4.1）
   - 查看模型列表确认可用性

3. **生成失败**
   - 检查积分是否充足
   - 检查提示词是否符合内容规范
   - 查看日志获取详细错误信息

## 更新日志

### v4.5 支持 (2024-12-07)
- 新增 jimeng-4.5 模型支持
- 支持最高 2048x2048 分辨率
- 优化了提示词理解能力

### v4.1 支持 (2024-12-07)
- 新增 jimeng-4.1 模型支持
- 改进了图像生成质量
- 增强了多图生成功能
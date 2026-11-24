# M1 感知模块 · API 说明

本文档是 `libxiaohong_perception.so` 的完整接口契约。所有入口都返回 **envelope**，
调用方先看 `ok`，再读 `data` 或 `error`。

## 1. 约定

### 1.1 Envelope

每个入口（包括业务失败）都 resolve 为一个 envelope JSON：

```jsonc
// 成功
{ "ok": true, "data": { ... } }

// 失败（业务错误或非法输入，不是 JS 异常）
{ "ok": false, "error": { "code": "ENGINE_NOT_INITIALIZED", "message": "..." } }
```

- **业务错误照常 resolve**，不会 reject Promise。
- **传输错误**（参数类型错误、请求超 32 MiB、含内嵌 NUL、native 崩溃/取消）会同步
  throw 或 reject Promise，错误码前缀 `ERR_M1_*`。

### 1.2 输入与限制

- 每个入口接收**一个** UTF-8 JSON 字符串。
- 最大 32 MiB；含内嵌 `\0` 会被拒绝（不静默截断）。
- 图片必须以 **base64** 随请求传入；本模块不编码图片。
- 只接受 **JPEG / PNG / WebP**。

## 2. ArkTS 接口

```ts
import perception from 'libxiaohong_perception.so';

await perception.init(requestJson: string): Promise<string>;
await perception.processPhoto(requestJson: string): Promise<string>;
await perception.correctEvidence(requestJson: string): Promise<string>;
```

## 3. 数据类型

### 3.1 InitOptions（`init` 入参）

```ts
interface InitOptions {
  detectionModel: string;   // 检测模型沙箱绝对路径（ocrs 兼容）
  recognitionModel: string; // 识别模型沙箱绝对路径（ocrs 兼容）
  language?: string;        // 后端语言提示，默认 en
  alphabet?: string;        // 必须匹配识别模型的字符顺序
}
```

- `detectionModel` 与 `recognitionModel` 是**应用沙箱内可读的绝对路径**。
- 模型必须是 ocrs 兼容结构，不是任意 ONNX。
- `language` 目前是提示位，默认 ocrs 后端不使用它。

### 3.2 PerceptionRequest（`processPhoto` 入参）

```ts
interface PerceptionRequest {
  photoId: string;          // 应用侧稳定照片标识，原图字节不出端
  imageBase64: string;      // 已有 JPEG/PNG/WebP 的 base64 文本
  rotationDegrees?: number; // 顺时针旋转，0/90/180/270，默认 0
  language?: string;        // 默认 zh-CN
  region?: BoundingBox;     // 原图像素坐标的兴趣区域，可选
}
```

### 3.3 BoundingBox（原图像素坐标）

```ts
interface BoundingBox { left: number; top: number; right: number; bottom: number; }
```

约束：`left < right`、`top < bottom`、不越界。

### 3.4 PerceptionResult（`processPhoto` 成功 `data`）

```ts
interface PerceptionResult {
  schemaVersion: number;        // 当前为 1
  photoId: string;
  originalWidth: number;        // 原始照片宽（旋转前）
  originalHeight: number;       // 原始照片高（旋转前）
  readingOrder: string[];       // 按阅读顺序排列的 evidenceId
  blocks: EvidenceBlock[];
  warnings: string[];
}
```

### 3.5 EvidenceBlock

```ts
interface EvidenceBlock {
  evidenceId: string;      // 稳定且对相同输入确定，形如 "<photoId>:e001"
  text: string;            // 识别文本（已 trim）
  kind: 'text' | 'formula' | 'diagramLabel' | 'handwriting';
  confidence: number;      // 0..1；ocrs 后端恒为 1.0
  originalBounds: BoundingBox; // 已映射回原始照片（旋转/ROI 逆变换）
  readingOrder: number;    // 从 1 开始
  needsReview: boolean;    // 低于置信阈值时 true（ocrs 路径不触发）
}
```

坐标语义：即使请求带了 `rotationDegrees` 或 `region`，`originalBounds` 也是**原始
未旋转照片**的像素坐标，可直接用于在界面高亮原图。

### 3.6 CorrectionRequest（`correctEvidence` 入参）

```ts
interface CorrectionRequest {
  evidence: EvidenceBlock;        // 原块，来自 PerceptionResult.blocks
  correctedText?: string;         // 替换文本；省略则保留原文本
  reviewCleared?: boolean;        // true 清除 needsReview
}
```

### 3.7 CorrectedEvidence（`correctEvidence` 成功 `data`）

```ts
interface CorrectedEvidence {
  evidenceId: string;
  text: string;
  kind: string;
  confidence: number;
  originalBounds: BoundingBox;
  readingOrder: number;
  needsReview: boolean;   // reviewCleared=false 且原块 needsReview 时仍为 true
  userCorrected: boolean; // 提供了 correctedText 或 reviewCleared 时为 true
}
```

## 4. 错误码

| code | 含义 |
|---|---|
| `INVALID_JSON` | 入参不是合法 JSON 或字段缺失/类型错误 |
| `IMAGE_DECODE` | 图片字节无法解码 |
| `IMAGE_TOO_LARGE` | 超过尺寸/像素上限（12000×12000 或 8000 万像素） |
| `EMPTY_RECOGNITION` | 识别结果没有任何非空文本 |
| `OCR_FAILED` | 模型加载失败或后端识别错误 |
| `ENGINE_NOT_INITIALIZED` | 未先 `init` 就调用 `processPhoto` |
| `BACKEND_UNAVAILABLE` | 未以 `--features ocrs-backend` 编译 |
| `ENGINE_LOCKED` | 引擎全局锁被破坏（进程级错误） |
| `NULL_INPUT` | C 边界收到空指针（防御性；ArkTS 层正常不会触发） |

## 5. 调用顺序

```
1. init(InitOptions)          → envelope{ ok }
2. processPhoto(Request)      → envelope{ ok, data: PerceptionResult }
3. （可选）correctEvidence    → envelope{ ok, data: CorrectedEvidence }
```

`processPhoto` 前必须先 `init` 成功，否则返回 `ENGINE_NOT_INITIALIZED`。

## 6. 示例

### 6.1 初始化

```ts
await perception.init(JSON.stringify({
  detectionModel: '/data/storage/el2/base/models/text-detection.onnx',
  recognitionModel: '/data/storage/el2/base/models/text-recognition.onnx',
  language: 'en'
}));
// → '{"ok":true,"data":null}'
```

### 6.2 处理照片

```ts
const raw = await perception.processPhoto(JSON.stringify({
  photoId: 'P1',
  imageBase64: '...',   // 已有编码文本
  rotationDegrees: 90
}));
const envelope = JSON.parse(raw);
// envelope.data = {
//   "schemaVersion":1, "photoId":"P1",
//   "originalWidth":4000, "originalHeight":3000,
//   "readingOrder":["P1:e001","P1:e002"],
//   "blocks":[
//     {"evidenceId":"P1:e001","text":"∫u dv = uv − ∫v du","kind":"text",
//      "confidence":1.0,"originalBounds":{...},"readingOrder":1,"needsReview":false}
//   ],
//   "warnings":["Bounds were mapped back to the original photo after right-angle rotation."]
// }
```

### 6.3 用户校正

```ts
const block = envelope.data.blocks[0];
await perception.correctEvidence(JSON.stringify({
  evidence: block,
  correctedText: '∫u dv = uv − ∫v du',
  reviewCleared: true
}));
// → data.userCorrected = true, data.needsReview = false
```

## 7. 稳定性说明

- `schemaVersion` 递增表示 `PerceptionResult` 结构不兼容变更。
- `evidenceId` 对相同请求内容确定；改变输入顺序或数量会改变编号。
- `readingOrder` 按「行带 + 列内从左到右」排序，不推断从右到左（RTL）版面。

# xiaohong-perception · M1 感知模块

小鸿助手的 **M1 感知子系统**：把一张黑板照片变成可追溯的「板书证据」，供上层编排
（M3）起草笔记、回链原文。核心用 Rust 编写，通过 OpenHarmony N-API 暴露给 ArkTS。

## 这个模块做什么

M1 不做完整笔记生成，只解决「这张照片的哪一块读到了什么」：

1. **输入校验与预处理**——只接受 JPEG/PNG/WebP；支持按 90° 旋转摆正，支持选定兴趣
   区域（ROI）后再识别。
2. **OCR 适配**——OCR 通过 `OcrEngine` trait 注入；开箱带一个纯 Rust 后端
   `ocrs-backend`，也允许接入平台能力或测试替身。
3. **阅读顺序**——把识别结果按「行带 + 列内从左到右」重排，输出稳定的阅读序。
4. **证据回链**——每个文本块带稳定 `evidenceId`、原图坐标（旋转/ROI 都映射回原始
   照片）、类别与是否需要人工复核。
5. **用户校正**——提供 `correctEvidence`，把用户校对后的文字与「已复核」标记写回，
   与原始识别结果区分。

## 目录结构

```
/
├── Cargo.toml              Rust crate 与 feature 定义
├── Cargo.lock
├── src/
│   ├── lib.rs              引擎状态、C ABI（m1_*）
│   ├── types.rs            JSON 数据契约、错误码
│   ├── pipeline.rs         预处理、阅读顺序、证据映射
│   └── ocrs_backend.rs     ocrs/rten OCR 后端（feature 门控）
├── include/m1_ffi.h        Rust 导出的 C 接口声明
├── napi/napi_init.cpp      异步 N-API 桥接（init/processPhoto/correctEvidence）
├── arkts/                  ArkTS 类型包与调用样例
├── scripts/build-ohos.sh   交叉构建脚本
├── CMakeLists.txt          与 Rust staticlib 链接为 libxiaohong_perception.so
└── tests/ffi_smoke.c       C 冒烟测试
```

## 数据流

```
ArkTS（已有图片的 base64）
   │  init(InitOptions) → 加载检测/识别模型
   │  processPhoto(PerceptionRequest)
   ▼
Rust 核心
   解码 → 尺寸上限 → 旋转/ROI → OCR → 阅读顺序 → 坐标回映原图
   ▼
PerceptionResult{ readingOrder, blocks:[EvidenceBlock{evidenceId, originalBounds,
kind, confidence, needsReview}] , warnings }
   │  correctEvidence(CorrectionRequest) → CorrectedEvidence{userCorrected}
   ▼
上层（M3/展示层）用 evidenceId 回链原图、按阅读序拼接、标记待校对项
```

## 快速开始（宿主机）

核心不依赖系统 OCR，测试使用注入的替身引擎：

```bash
cd code/M1
cargo test                      # 3 个单元测试：阅读顺序、旋转映射、低置信标记
```

启用真实 OCR 后端（纯 Rust，无需系统库）：

```bash
cargo check --features ocrs-backend
```

`ocrs-backend` 需要一个检测模型和一个识别模型（见下文「模型」）。

## 模型

`ocrs-backend` 基于开源 [robertknight/ocrs](https://github.com/robertknight/ocrs)
与 [rten](https://github.com/robertknight/rten)。它**不是任意 ONNX 运行时**，只接受
与 ocrs 模型结构兼容的模型。

- **官方示例模型**（英文打印文本，横向版面）：
  `text-detection.onnx`、`text-recognition.onnx`，来源见 ocrs 仓库的
  `examples/download-models.sh`。**长短期 URL 不保证稳定，正式发布请自托管模型。**
- **中文 / 手写数学公式**：官方模型不覆盖。需要训练或取得自定义识别模型，并在
  `InitOptions.alphabet` 中按模型字符顺序提供字母表，否则识别结果不可用。
- ocrs 不暴露逐行置信度，本模块把后端返回视为可信，`needsReview` 目前不会由 ocrs
  路径自动触发（其他适配器可自行提供置信度）。

## 集成到 OpenHarmony

### 1. 编译 Rust

```bash
# 先安装 OpenHarmony SDK/NDK，并安装对应 Rust 目标：
rustup target add aarch64-linux-ohos   # 或 x86_64-linux-ohos
```

### 2. 交叉构建

```bash
OHOS_NDK_HOME=/path/to/sdk scripts/build-ohos.sh arm64-v8a
```

脚本校验 `OHOS_NDK_HOME`、`cargo`、`rustup`、`cmake` 与 OpenHarmony CMake
toolchain；缺一即报错退出，不会下载 SDK。产物为 `libxiaohong_perception.so`。

### 3. ArkTS 侧

把 `arkts/` 作为类型包，`import perception from 'libxiaohong_perception.so'`，然后
`await perception.init(...)`、`await perception.processPhoto(...)`。调用样例见
`arkts/RustPerceptionExample.ets`。**本模块只处理已有图片的 base64，不负责拍照、
图片编码或平台 OCR。** 详细接口见 [`API.md`](API.md)。

## 限制与未验证项

- **未在真机/模拟器验证**：本机没有 OpenHarmony SDK 与交叉工具链，`build-ohos.sh`
  只在满足环境时执行；N-API 桥接按官方文档编写，但未运行在设备上。
- **中文识别**：默认 ocrs 模型不支持中文；需要自定义模型与字母表。
- **公式识别**：OCR 只产出文本块，`BlockKind::Formula` 是数据分类位，不等于公式
  结构解析。
- **置信度**：ocrs 无逐行置信度，低置信复核需由其他适配器或上层策略触发。

## 许可

MIT OR Apache-2.0。依赖 `ocrs`、`rten` 及其子 crate 遵循各自上游许可证
（MIT/Apache-2.0）。

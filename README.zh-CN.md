# CapGlyph

图像水印、载荷恢复与签名来源声明工具。

[English](README.md)

## 功能

CapGlyph 将水印嵌入 PNG/JPEG 图像，之后可测量存在性、提取接收者 ID 或检测密钥派生信号。结果依赖载体、图像、载荷及变换条件；单独的检测结果不能证明作者身份、所有权或来源声明的真实性。支持四种嵌入技术：

| 模式      | 技术                        | 构建方式             |
| --------- | --------------------------- | -------------------- |
| `alpha`   | 稀疏 alpha 通道像素         | 默认                 |
| `dct`     | 8×8 DCT 系数调制            | 默认                 |
| `dwt`     | Haar LH 频带调制            | 默认                 |
| `learned` | Adobe TrustMark CNN（ONNX） | `--features learned` |

## 安装

Linux、macOS 和 Windows 的预编译二进制（含 `learned` 与 `c2pa` 特性）随每个
[GitHub Release](https://github.com/CapGlyph/capglyph-cli/releases) 发布。

**macOS / Linux — Homebrew：**

```bash
brew tap CapGlyph/tap
brew install capglyph
```

**Windows — Scoop：**

```powershell
scoop bucket add capglyph https://github.com/CapGlyph/scoop-bucket
scoop install capglyph
```

**Arch Linux — AUR：**

```bash
yay -S capglyph-bin      # 预编译二进制（推荐）
# 或从源码构建：
yay -S capglyph
```

**Linux — deb / rpm / pkg.tar.zst：** 从
[最新 release](https://github.com/CapGlyph/capglyph-cli/releases/latest) 下载。

## 从源码构建

```bash
cargo build --release                    # alpha/dct/dwt
cargo build --release --features learned # + learned 模式（ONNX 运行时）
cargo build --release --features c2pa     # + C2PA 内容凭证
```

## 快速上手

```bash
# 嵌入特定接收者的水印
capglyph embed photo.png --mode dwt --recipient-id "alice001" --output photo_wm.png

# 验证
capglyph verify photo_wm.png --mode dwt; echo $?        # 0 = 存在

# 提取 ID（无需原图——泄露副本上即可提取）
capglyph extract leaked.png --mode dwt --id-length 8

# Keyed signal verification, not a general collusion guarantee
capglyph embed photo.png --mode dwt --recipient-id "bob" --key "mysecret" --output photo_keyed.png
capglyph verify photo_keyed.png --mode dwt --key "mysecret"

# Learned mode; robustness depends on the evaluated transformation profile
capglyph fetch-models                          # 下载 TrustMark ONNX（约 65MB）
capglyph embed photo.png --mode learned --recipient-id "carol"
capglyph extract leaked.png --mode learned
```

## 历史攻击观察

| 攻击               |     alpha     | dct |    dwt    | learned |
| ------------------ | :-----------: | :-: | :-------: | :-----: |
| JPEG q50           |       ✗       |  ✓  |     ✓     |    ✓    |
| JPEG q30           |       ✗       |  ✗  |     ✗     |  **✓**  |
| 模糊 σ2.0          |       ✗       |  ✗  | 验证✓/ID✗ |  **✓**  |
| 缩放 0.5×          |       ✗       |  ✗  |     ✗     |  **✓**  |
| 共谋（5 份中值）   |       —       |  —  | 密钥层 ✓  |    —    |
| 已知原图差分       | ✗（不可避免） |  ✗  |     ✗     |    ✗    |
| img2img 生成式重绘 |       ✗       |  ✗  |     ✗     |    ✗    |

这份历史定性表混合了存在性、ID 恢复和密钥信号结果，没有统一的试验分母或校准后的误报工作点。它不是发行保证，也不是完整认证 Credential 传输的对比；具体结论必须对应冻结的协议和结果记录，不能推广为所有图像、编码器或模型的通用阈值。

## 嵌入位置策略 (评估对比)

为了进行实证评估和基准对比，CapGlyph 支持三种块嵌入策略（通过 `--placement` 标志配置）：

- `skeleton` (默认)：沿着图像的几何拓扑路径（边缘和轮廓）嵌入水印。
- `edge`：一种竞争性基线策略，专门针对标准的高方差边缘块进行嵌入。
- `prng`：一种内部控制策略，将水印伪随机地均匀分布在整个图像中。

## 安全模型

- **公共层** —— 存在性检测（`verify`）
- **ID 层** —— 按接收者追踪（`extract`，无需几何文件）
- **密钥层** —— 密钥派生信号检测（`--key`），不能代替认证载荷校验，也不构成通用的防伪或抗共谋保证。

持有原图的攻击者可直接以未加水印的原图替换标记图像；生成式重绘也可能破坏水印，但 `0.3` 之类的去噪强度只属于特定模型和实验配置，并非通用临界值。提取的接收者 ID 是标签，不能独立证明持有人身份或法律归属。

## 兼容性

CapGlyph 已取代弃用的 Sigil 名称。现有集成仍保留以下兼容路径：

- Linux 软件包与 Homebrew 保留 `sigil` 命令别名。
- `SIGIL_MODEL_DIR` 与 `SIGIL_RECIPIENT_ID` 分别作为 `CAPGLYPH_MODEL_DIR`
  与 `CAPGLYPH_RECIPIENT_ID` 的后备环境变量。未显式指定模型目录或环境变量时，
  若旧 XDG 目录 `sigil/models` 存在且 `capglyph/models` 不存在，learned 模式仍使用旧目录。
- C2PA 签名仍同时写入 `com.sigil.watermark`，供旧版读取器使用。验证优先读取
  `com.capglyph.watermark`，找不到时再读取旧声明。现有证书与密钥文件仍可通过
  `--cert`/`--pkey` 显式传入。
- CLI 库保留 `sigil_core` 重导出，供下游 Rust 调用方兼容使用。

## 文档

- `docs/mvp-spec.md` — 完整规范
- `docs/product-roadmap.md` — 产品/B2B 方向
- `CHANGELOG.md` — 发布历史

## 许可证

Apache-2.0。learned 模式嵌入了 Adobe TrustMark 模型（MIT 许可，从
Adobe CDN 单独下载——不随 CapGlyph 分发）。

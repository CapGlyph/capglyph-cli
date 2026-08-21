# Sigil

面向图像的隐形结构水印 —— 来源证明、泄露追踪与篡改检测。

[English](README.md)

## 功能

Sigil 将亚感知水印嵌入 PNG/JPEG 图像，之后可验证其存在性、提取分发给特定接收者的 ID，或通过密钥证明归属。四种独立的嵌入技术：

| 模式      | 技术                        | 构建方式             |
| --------- | --------------------------- | -------------------- |
| `alpha`   | 稀疏 alpha 通道像素         | 默认                 |
| `dct`     | 8×8 DCT 系数调制            | 默认                 |
| `dwt`     | Haar LH 频带调制            | 默认                 |
| `learned` | Adobe TrustMark CNN（ONNX） | `--features learned` |

## 安装

Linux、macOS 和 Windows 的预编译二进制（含 `learned` 与 `c2pa` 特性）随每个
[GitHub Release](https://github.com/Xuepoo/sigil/releases) 发布。

**macOS / Linux — Homebrew：**

```bash
brew tap xuepoo/tap
brew install sigil
```

**Windows — Scoop：**

```powershell
scoop bucket add xuepoo https://github.com/Xuepoo/scoop-bucket
scoop install sigil
```

**Arch Linux — AUR：**

```bash
yay -S sigil-wm-bin      # 预编译二进制（推荐）
# 或从源码构建：
yay -S sigil-wm
```

**Linux — deb / rpm / pkg.tar.zst：** 从
[最新 release](https://github.com/Xuepoo/sigil/releases/latest) 下载。

## 从源码构建

```bash
cargo build --release                    # alpha/dct/dwt
cargo build --release --features learned # + learned 模式（ONNX 运行时）
cargo build --release --features c2pa     # + C2PA 内容凭证
```

## 快速上手

```bash
# 嵌入特定接收者的水印
sigil embed photo.png --mode dwt --recipient-id "alice001" --output photo_wm.png

# 验证
sigil verify photo_wm.png --mode dwt; echo $?        # 0 = 存在

# 提取 ID（无需原图——泄露副本上即可提取）
sigil extract leaked.png --mode dwt --id-length 8

# 密钥归属（可抵御共谋攻击）
sigil embed photo.png --mode dwt --recipient-id "bob" --key "mysecret"
sigil verify photo_wm.png --mode dwt --key "mysecret"   # + SECRET LAYER PRESENT

# learned 模式（激进编辑抵抗力：JPEG q30、模糊 σ2、缩放 0.5×）
sigil fetch-models                          # 下载 TrustMark ONNX（约 65MB）
sigil embed photo.png --mode learned --recipient-id "carol"
sigil extract leaked.png --mode learned
```

## 攻击矩阵（实测）

| 攻击               |     alpha     | dct |    dwt    | learned |
| ------------------ | :-----------: | :-: | :-------: | :-----: |
| JPEG q50           |       ✗       |  ✓  |     ✓     |    ✓    |
| JPEG q30           |       ✗       |  ✗  |     ✗     |  **✓**  |
| 模糊 σ2.0          |       ✗       |  ✗  | 验证✓/ID✗ |  **✓**  |
| 缩放 0.5×          |       ✗       |  ✗  |     ✗     |  **✓**  |
| 共谋（5 份中值）   |       —       |  —  | 密钥层 ✓  |    —    |
| 已知原图差分       | ✗（不可避免） |  ✗  |     ✗     |    ✗    |
| img2img 生成式重绘 |       ✗       |  ✗  |     ✗     |    ✗    |

## 嵌入位置策略 (评估对比)

为了进行实证评估和基准对比，Sigil 支持三种块嵌入策略（通过 `--placement` 标志配置）：
* `skeleton` (默认)：沿着图像的几何拓扑路径（边缘和轮廓）嵌入水印。
* `edge`：一种竞争性基线策略，专门针对标准的高方差边缘块进行嵌入。
* `prng`：一种内部控制策略，将水印伪随机地均匀分布在整个图像中。

## 安全模型

- **公共层** —— 存在性检测（`verify`）
- **ID 层** —— 按接收者追踪（`extract`，无需几何文件）
- **密钥层** —— HMAC 密钥归属（`--key`），可抵御共谋攻击、阻止伪造

所有像素水印共有的硬性限制：持有原图的攻击者总可通过差分移除水印；
生成式重绘（img2img）在去噪强度 ≥0.3 时即可破坏水印。

## 文档

- `docs/mvp-spec.md` — 完整规范
- `docs/product-roadmap.md` — 产品/B2B 方向
- `CHANGELOG.md` — 发布历史

## 许可证

Apache-2.0。learned 模式嵌入了 Adobe TrustMark 模型（MIT 许可，从
Adobe CDN 单独下载——不随 Sigil 分发）。

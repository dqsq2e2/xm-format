# XM Format Plugin

XM 格式支持插件，用于支持喜马拉雅xm格式文件。

## 功能特性

- **格式检测**: 自动识别 XM 格式的音频文件
- **文件解密**: 将 XM 格式文件解密为标准音频格式（MP3, M4A, FLAC, WAV） 
- **元数据提取**: 提取音频文件的标题、艺术家、专辑等信息
- **进度报告**: 解密过程中实时报告进度
- **缓存支持**: 配合核心缓存服务避免重复解密

## ✨ 真正的流式解密支持

**XM 格式只加密第一个数据块，其余部分为明文！**

这意味着我们可以实现真正的流式解密，内存占用极低：

1. **只解密第一块**：通常 < 10MB，内存占用极小
2. **直接流式传输明文部分**：无需解密，零内存开销
3. **边解密边播放**：播放可以立即开始，无需等待完整解密

**文件结构：**
```
[ID3 Header] [加密块] [明文音频数据]
     ↓          ↓            ↓
  元数据    需要解密    直接流式传输
```

**实际工作流程：**
```
用户请求播放 XM 文件
    ↓
核心缓存服务检查缓存
    ├─ 缓存命中 → 直接流式播放（低内存）
    └─ 缓存未命中 → 继续
    ↓
XM 插件流式解密（低内存）
    ├─ 解密第一块（< 10MB）
    └─ 流式传输明文部分（零开销）
    ↓
同时：缓存解密结果 + 开始播放
    ↓
音频流模块流式播放（低内存）
```

**性能优势：**
- ✅ 内存占用：只需第一块大小（通常 < 10MB）
- ✅ 播放延迟：极低，解密第一块后立即开始
- ✅ 大文件友好：100MB+ 文件也能流畅播放
- ✅ 配合缓存：首次解密后，后续播放零开销

## 技术实现

### 解密算法

XM 文件使用以下加密方案：

1. **ID3v2 标签**: 包含加密元数据
   - `TSIZ`: 加密数据大小
   - `TSRC`/`TENC`: 十六进制编码的 IV（初始化向量）
   - `TSSE`: Base64 编码前缀
   - `TRCK`: 轨道编号（用于 WASM 算法）

2. **AES-256-CBC 加密**: 
   - 密钥: `ximalayaximalayaximalayaximalaya`
   - IV: 从 TSRC 或 TENC 标签提取
   - 填充: PKCS7

3. **WASM 后处理**: 使用内嵌的 WASM 模块进行额外的数据转换

4. **Base64 解码**: 最终数据使用 Base64 编码

### 文件结构

```
[ID3v2 Header] [Encrypted First Chunk] [Plaintext Audio Data]
```

- **ID3v2 Header**: 包含元数据和加密参数（TSIZ, TSRC/TENC, TSSE, TRCK）
- **Encrypted First Chunk**: 仅第一个数据块使用 AES-256-CBC 加密
- **Plaintext Audio Data**: 其余音频数据为明文，可直接流式传输

**关键发现**：XM 格式只加密第一个数据块，这使得真正的流式解密成为可能！

## 使用方法

### 作为库使用

```rust
use xm_format::{XmFormatPlugin, PluginConfig};
use std::path::Path;

let plugin = XmFormatPlugin::new(PluginConfig::default());

// 检测文件格式
let is_xm = plugin.detect(Path::new("audio.xm"))?;

// 解密文件
plugin.decrypt_file(
    Path::new("audio.xm"),
    Path::new("audio.m4a"),
    Some(Box::new(|progress| {
        println!("Progress: {:.1}%", progress * 100.0);
    }))
)?;

// 提取元数据
let metadata = plugin.extract_metadata(Path::new("audio.xm"))?;
println!("Title: {:?}", metadata.title);
println!("Artist: {:?}", metadata.artist);
```

### 配置选项

```json
{
  "enable_streaming": true,
  "buffer_size": 8192
}
```

- `enable_streaming`: 启用流式解密（减少内存占用）
- `buffer_size`: 解密缓冲区大小（字节）

## 依赖项

- `aes`: AES 加密算法
- `cbc`: CBC 模式
- `hex`: 十六进制编解码
- `base64`: Base64 编解码
- `wasmer`: WASM 运行时
- `wasmer-compiler-cranelift`: WASM 编译器

## 构建

### 构建为 WASM 插件

```bash
cargo build --target wasm32-wasi --release
```

### 构建为原生库

```bash
cargo build --release
```

## 测试

```bash
cargo test
```

## 参考资料

- [喜马拉雅 XM 文件解密逆向分析](https://www.aynakeya.com/articles/ctf/xi-ma-la-ya-xm-wen-jian-jie-mi-ni-xiang-fen-xi/)
- 原始 Python 实现: [ximalaya-downloader](https://github.com/lx0758/ximalaya-downloader)

## 许可证

MIT License

## 注意事项

⚠️ **免责声明**: 本插件仅供学习和研究使用。请遵守喜马拉雅的服务条款和版权法律。未经授权下载和分发受版权保护的内容可能违法。

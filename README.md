# HTTP 代理服务器

一个基于 Rust 实现的高性能 HTTP 代理服务器，支持范围请求和智能缓存。

## 功能特性

- 支持 HTTP/HTTPS 代理
- 支持范围请求（Range Request）
- 智能缓存系统
  - 支持部分缓存和断点续传
  - 自动合并缓存片段
  - 缓存状态持久化
- 高性能流式处理
  - 异步 I/O
  - 流式数据传输
  - 内存使用优化
- 可配置选项
  - 自定义端口
  - 自定义缓存目录
  - 缓存清理策略

## 安装

### 前置要求

- Rust 1.70.0 或更高版本
- Cargo 包管理器

### 安装步骤

1. 克隆仓库：
```bash
git clone https://github.com/yourusername/proxy-server.git
cd proxy-server
```

2. 编译项目：
```bash
cargo build --release
```

## 使用方法

### 启动服务器

```bash
cargo run --release
```

默认配置：
- 监听端口：8080
- 缓存目录：./cache

### 自定义配置

可以通过环境变量或命令行参数配置：

```bash
# 自定义端口
PROXY_PORT=9090 cargo run --release

# 自定义缓存目录
CACHE_DIR=/tmp/proxy-cache cargo run --release
```

### 代理请求格式

支持两种请求格式：

1. 直接代理：
```
http://localhost:8080/proxy/{target_url}
```

2. Range 请求：
```
# 请求指定范围
curl -H "Range: bytes=0-1024" http://localhost:8080/proxy/http://example.com/file.mp4

# 默认从头开始请求
curl http://localhost:8080/proxy/http://example.com/file.mp4
```

## 核心功能说明

### 1. 范围请求处理

- 支持标准的 HTTP Range 请求
- 自动处理 Range 头部
- 支持多种范围格式（如 "bytes=0-100", "bytes=100-", "bytes=-100"）

### 2. 缓存系统

- 基于文件的持久化缓存
- 支持部分缓存和范围合并
- 缓存状态管理
  - 记录已缓存的数据范围
  - 自动合并相邻的缓存片段
  - JSON 格式的缓存元数据

### 3. 流式处理

- 高效的内存使用
- 支持大文件处理
- 实时数据传输

## 示例

### 基本使用

```rust
// 启动代理服务器
let server = ProxyServer::new(8080, "./cache");
server.start().await?;

// 发送代理请求
let url = "http://example.com/video.mp4";
let range = "bytes=0-1024";
let response = reqwest::Client::new()
    .get(&format!("http://localhost:8080/proxy/{}", url))
    .header("Range", range)
    .send()
    .await?;
```

### 混合源测试

```rust
// 运行混合源测试示例
cargo run --example mixed_source_test
```

## 性能优化

- 异步 I/O 操作
- 流式数据处理
- 智能缓存策略
- 内存使用优化

## 贡献指南

欢迎提交 Issue 和 Pull Request！

## 许可证

MIT License 
# Rust HTTP 代理服务器

一个基于 Rust 实现的高性能 HTTP 代理服务器，支持缓存、范围请求和流式传输。

## 功能特性

- 支持 HTTP/HTTPS 代理
- 智能缓存管理
  - 基于文件的持久化缓存
  - 支持部分缓存和范围请求
  - 自动缓存清理
- 高效的流式传输
  - 支持大文件传输
  - 内存使用优化
- 混合数据源处理
  - 智能合并缓存和网络数据
  - 自动选择最优数据源
- 完整的日志系统
  - 详细的操作日志
  - 错误追踪

## 系统要求

- Rust 1.70.0 或更高版本
- Cargo 包管理器
- 支持 Unix/Linux/macOS 系统

## 安装

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
cargo run
```

服务器默认在 `http://127.0.0.1:8080` 启动。

### 示例

项目提供了多个示例来展示不同的使用场景：

1. 视频代理请求示例：
```bash
cargo run --example video_request
```

2. HTTPS 请求示例：
```bash
cargo run --example https_request
```

### 配置说明

服务器配置位于 `config.toml` 文件中，主要配置项包括：

- 服务器监听地址和端口
- 缓存目录路径
- 缓存大小限制
- 日志级别设置

### API 使用

1. 基本代理请求：
```rust
let client = Client::new();
let req = Request::builder()
    .method("GET")
    .uri("http://127.0.0.1:8080")
    .header("X-Original-Url", "https://example.com/file.mp4")
    .body(Body::empty())?;
```

2. 范围请求：
```rust
let req = Request::builder()
    .method("GET")
    .uri("http://127.0.0.1:8080")
    .header("Range", "bytes=0-1024")
    .header("X-Original-Url", "https://example.com/file.mp4")
    .body(Body::empty())?;
```

## 架构设计

### 核心组件

1. **DataSourceManager**
   - 负责管理数据源
   - 处理请求路由
   - 协调缓存和网络数据

2. **CacheManager**
   - 管理缓存存储
   - 处理缓存策略
   - 自动清理过期缓存

3. **StreamProcessor**
   - 处理流式数据
   - 合并多个数据源
   - 优化内存使用

### 数据流

```
Client Request
    ↓
DataSourceManager
    ↓
┌─────────────────┐
│ FileSource      │
│ NetworkSource   │ ← CacheManager
│ MixedSource     │
└─────────────────┘
    ↓
StreamProcessor
    ↓
Client Response
```

## 性能优化

- 使用异步 I/O 处理请求
- 实现智能缓存策略
- 优化内存使用和数据流处理
- 支持并发请求处理

## 错误处理

系统实现了完整的错误处理机制：

- 网络错误处理
- 缓存访问错误处理
- 范围请求验证
- 并发冲突处理

## 贡献指南

欢迎提交 Pull Request 或创建 Issue。在提交代码前，请确保：

1. 代码符合项目的代码风格
2. 添加了适当的测试
3. 更新了相关文档
4. 所有测试都能通过

## 许可证

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE) 文件。 
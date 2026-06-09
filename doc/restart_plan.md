# Geello Restart Plan

> **状态**: 草案 v0.5 — 第四轮反馈修订
> **日期**: 2026-06-09
> **修订**: v0.4→v0.5: 确定多 GPU 请求轮询策略；字体 fallback 采用配置顺序；GeoTIFF 波段计算列入远期规划

---

## 一、现有项目分析 (As-Is)

### 1.1 项目定位

Geello 是一个基于 Rust 的 GIS 渲染工具，核心能力：

- **Library 模式**：将 `geo` 矢量几何数据通过 `vello` (GPU 2D 矢量渲染引擎) 绘制到纹理/缓冲区；支持 GeoTIFF 遥感影像渲染
- **Server 模式**：提供类 WMTS/WMS 的地图瓦片服务 + WebSocket 实时动画推送

### 1.2 现有架构概览

```
geello/
├── Cargo.toml              # 单一 crate，通过 features 区分 lib/server
├── Geello.toml             # 运行时配置 (TOML)
├── Dockerfile              # 单阶段构建，cargo install
├── src/
│   ├── main.rs             # entry：feature="server" → Rocket launch，else → hello
│   ├── lib.rs              # 核心渲染 API（render_to_texture / render_to_buffer）
│   ├── render_option.rs    # 渲染参数 (区域、像素、投影、颜色)
│   ├── rendered_geometry.rs# 几何数据包装层 (lazy calc lines/areas/center)
│   ├── magic_value.rs      # "魔法值"系统：支持从文件/URL/props 懒加载 + RON 序列化
│   ├── utils.rs            # EPSG:4326↔3857 坐标变换、Tile 索引计算
│   ├── server.rs           # Rocket 服务器 (单文件 ~1000行) + DataCache + WMTS/WMS/WS
│   └── renderer/
│       ├── mod.rs          # GeometryRenderer enum (Point/Line/Area + Filter)
│       ├── point_renderer.rs
│       ├── line_renderer.rs
│       └── area_renderer.rs
├── assets/
│   ├── test/               # 测试数据 (GeoJSON, RON 配置, 字体, 缓存)
│   └── web-map/            # Leaflet 前端示例 (index.html)
└── .github/workflows/
    └── docker-publish.yml  # GitHub Actions + Docker Buildx + Cosign 签名
```

### 1.3 现有依赖及版本

| 用途 | 依赖 | 当前版本 |
|------|------|----------|
| 地理计算 | `geo` | 0.30.0 |
| 地理类型 | `geo-types` | 0.7.16 |
| GPU 渲染 | `vello` | 0.5.0 |
| 画笔/颜色 | `peniko` | 0.4.0 |
| 字体 | `skrifa` | 0.31.3 |
| 序列化格式 | `ron` | 0.10.1 |
| 序列化框架 | `serde` / `serde_json` | 1.0.219 / 1.0.140 |
| 异步运行时 | `tokio` | 1.45.1 |
| Web 框架 | `rocket` | 0.5.1 |
| WebSocket | `rocket_ws` | 0.1.1 |
| 图片编解码 | `image` | 0.25.6 |
| GeoJSON 解析 | `geojson` | 0.24.2 |
| HTTP 客户端 | `reqwest` | 0.12.20 |
| 日志 | `log` | 0.4.27 |

### 1.4 关键问题识别

| 编号 | 问题 | 严重程度 | 说明 |
|------|------|----------|------|
| P1 | **RON 格式** | 高 | 配置/样式文件使用 RON，不如 TOML 通用；MagicValue 系统深度绑定 RON |
| P2 | **Rocket 框架** | 高 | Rocket 0.5 对 Rust 版本有特殊要求，生态不如 Axum 活跃 |
| P3 | **Docker 镜像** | 高 | 单阶段构建、`cargo install` 编译慢、镜像体积大、无分层缓存优化 |
| P4 | **单文件 Server** | 中 | `server.rs` ~1000行，路由/缓存/渲染逻辑耦合，难以维护 |
| P5 | **MagicValue 系统** | 中 | 概念有趣但实现复杂，与 RON 紧耦合，缺乏对 TOML 和异步加载的支持 |
| P6 | **block_on_wgpu** | 中 | 在 async 上下文中阻塞当前线程等待 GPU，可能与 tokio 调度冲突 |
| P7 | **GPU 硬依赖** | 中 | 启动时无 GPU 直接 panic，错误信息对用户不友好 |
| P8 | **vello Renderer !Sync** | 低 | 每次请求需新建 Renderer，纹理池设计受限于此 |
| P9 | **缺少测试** | 低 | 代码中几乎没有单元测试或集成测试 |
| P10 | **无热加载** | 中 | 配置/样式修改后需重启服务 |
| P11 | **无遥感影像支持** | 中 | 当前仅支持矢量数据，缺乏 GeoTIFF 栅格渲染能力 |

---

## 二、目标架构 (To-Be)

### 2.1 总体设计原则

1. **单 Crate + mod + features**：保持单 crate，用模块 (`mod`) 拆分功能域，用 `features` 条件编译 server / from_http / from_json / raster 等
2. **TOML First**：所有配置文件、样式文件统一使用 TOML；彻底移除 `ron`
3. **Axum 替代 Rocket**：使用纯 stable Rust 兼容的 Axum + tower 生态
4. **分层 mod 架构**：`core` → `render` → `server`，每层通过 feature gate 隔离
5. **多阶段 Docker 构建**：参考 cargo-chef + musl + alpine 方案
6. **thiserror 错误处理**：使用 `thiserror` 替代裸 `String` 错误
7. **配置热加载**：支持 watch 文件变化自动重载样式/配置，粒度为文件级
8. **支持字体/文字渲染**：保留字体能力，优先实现文字标注渲染器
9. **使用 proj 库**：投影变换从手写公式迁移到 `proj` crate（用户负责引入），后续扩展更多投影
10. **全面 breaking change**：不保持向后兼容，整体重新设计 API
11. **支持 TIF 遥感影像**：新增 Raster 渲染器，支持 GeoTIFF 加载与渲染
12. **混合缓存**：使用 Foyer 实现内存 + 磁盘双层缓存
13. **多 GPU**：Device 管理预留多 GPU 扩展能力

### 2.2 模块结构 (Mod Map)

```
geello/
├── Cargo.toml                  # 单一 crate，features 控制编译
├── Geello.toml                 # 运行时配置 (TOML)
├── Dockerfile                  # 多阶段构建 (cargo-chef + musl + alpine)
├── src/
│   ├── main.rs                 # entry: feature="server" 启动 Axum，否则打印 info
│   ├── lib.rs                  # 公开 API 入口 + 核心渲染函数
│   │
│   ├── core/                   # ---------- 核心层 (无 GPU 依赖) ----------
│   │   ├── mod.rs
│   │   ├── option.rs           # RenderOption, PixelOption, RenderRegion
│   │   ├── geometry.rs         # RenderedGeometry (lazy lines/areas/center)
│   │   ├── filter.rs           # RenderedGeometryFilter
│   │   ├── value.rs            # PropValue, LazyValue<T> 系统
│   │   └── proj.rs             # 投影/坐标变换 (未来迁移到 proj crate)
│   │
│   ├── render/                 # ---------- 渲染层 (依赖 vello/wgpu) ----------
│   │   ├── mod.rs              # GeometryRenderer enum + draw 调度
│   │   ├── point.rs            # PointRenderer
│   │   ├── line.rs             # LineRenderer + LineNodeRenderers
│   │   ├── area.rs             # AreaRenderer + AreaLineRenderers
│   │   ├── text.rs             # TextRenderer (文字标注)
│   │   └── raster.rs           # RasterRenderer (GeoTIFF 遥感影像)
│   │
│   ├── device.rs               # GPU 设备上下文管理 (wgpu device/queue 初始化，多 GPU 预留)
│   │
│   └── server/                 # ---------- 服务层 (feature="server") ----------
│       ├── mod.rs              # Axum Router 组装 + 启动入口
│       ├── main.rs             # #[tokio::main] 服务启动
│       ├── config.rs           # 配置管理 (TOML + env, 热加载)
│       ├── state.rs            # AppState (Device, Queue, TexturePool, Cache)
│       ├── error.rs            # 统一错误类型 (thiserror)
│       ├── routes/
│       │   ├── mod.rs
│       │   ├── wmts.rs         # /wmts/real-time, /wmts/cache
│       │   ├── wms.rs          # /wms
│       │   ├── websocket.rs    # /ws/anim
│       │   ├── map.rs          # /map/{*path} 静态文件
│       │   └── admin.rs        # /cache/stats, /example/{name}, /health
│       ├── cache/
│       │   ├── mod.rs          # CacheManager 总控
│       │   ├── layer.rs        # LayerCache
│       │   ├── style.rs        # StyleCache
│       │   └── tile.rs         # TileCache (内存+磁盘混合)
│       └── hot_reload.rs       # 配置/样式热加载 (notify/watch，文件粒度)
│
├── assets/
│   ├── test/                   # 测试数据 (GeoJSON, TOML 配置, 字体, TIF)
│   │   ├── china.geo.json
│   │   ├── line.geojson
│   │   ├── point.geojson
│   │   ├── polygon.geojson
│   │   ├── fsong_gb2312.ttf    # 字体文件
│   │   ├── sample.tif          # GeoTIFF 测试数据
│   │   └── styles/             # 样式配置目录
│   │       ├── 3857.toml
│   │       ├── 4326.toml
│   │       ├── area.toml
│   │       └── raster.toml     # 遥感影像渲染样式
│   └── web-map/                # Leaflet 前端示例（保留）
│       └── index.html
├── config/                     # 默认配置文件
│   └── default.toml
├── doc/
│   └── restart_plan.md
└── .github/workflows/
    └── docker-publish.yml
```

### 2.3 Feature Flags 设计

```toml
[features]
# 默认：纯 library 模式，最小依赖
default = []

# 启用 Axum 服务器
server = ["dep:axum", "dep:tokio", "dep:tower", "dep:tower-http",
         "dep:geojson", "dep:image", "dep:reqwest", "dep:serde_json",
         "dep:notify", "dep:tracing", "dep:tracing-subscriber", "dep:foyer"]

# 从 HTTP 加载数据/样式
from_http = ["dep:reqwest"]

# 从 JSON 加载数据
from_json = ["dep:serde_json"]

# GeoTIFF 遥感影像支持（地理参考由 tiff crate 解析）
raster = ["dep:tiff"]

# 全功能
full = ["server", "from_http", "from_json", "raster"]
```

### 2.4 各模块职责边界

#### `core/` — 无 GPU 依赖，纯数据结构与计算
- `RenderOption` / `PixelOption` / `RenderRegion`：渲染参数定义
- `RenderedGeometry`：几何数据包装层，惰性计算 lines/areas/center_point
- `RenderedGeometryFilter`：几何过滤（按 layer 名称）
- `LazyValue<T>` / `PropValue`：可延迟解析的值系统（固定值 / 文件 / URL / GeoJSON 属性）
- `proj.rs`：投影坐标变换工具函数（未来迁移到 `proj` crate，支持 EPSG:3857/4326 及后续更多投影）

#### `render/` — 依赖 vello/wgpu/peniko
- `GeometryRenderer` enum：Point / Line / Area / Text / Raster 五种渲染器调度
- 各具体渲染器实现
- Raster 渲染器负责 GeoTIFF 遥感影像的加载、投影变换与纹理贴图
- 场景构建 (`vello::Scene`) 和 vello 渲染调用
- 不依赖 HTTP / Web 框架

#### `device.rs` — wgpu 设备管理
- GPU 设备初始化、Queue 获取
- 纹理池管理 (Vec<Texture>)
- 架构预留多 GPU 支持（当前单 GPU，接口设计允许未来扩展为 `Vec<DeviceHandle>`）
- 可被 server 和 lib 模式共用

#### `server/` — feature="server" 时编译
- Axum Router 和所有 HTTP 路由
- 配置管理 + 热加载（文件粒度监听）
- 多层混合缓存系统（Foyer：内存 + 磁盘）
- AppState 管理
- 不包含渲染逻辑本身，调用 `render/` 和 `core/`

---

## 三、核心系统设计

### 3.1 LazyValue 系统重新设计

当前 `MagicValue<T>` 的核心价值：
- 值可以是 **固定值** (Fixed / inline)
- 可以从 **TOML 文件** 加载
- 可以从 **HTTP URL** 加载（需 feature from_http）
- 可以从 **GeoJSON properties** 动态读取（Map 属性映射）

重新设计目标：
- 与 TOML 格式原生适配
- 支持异步加载（文件/URL）
- 分离"解析阶段"和"使用阶段"
- 更清晰的类型系统，减少泛型传染

#### 3.1.1 核心类型

```rust
/// 值的来源描述
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ValueSource {
    /// 固定值（内联在 TOML 中）
    Inline,
    /// 从文件加载（相对路径或绝对路径）
    File {
        path: String,
    },
    /// 从 HTTP URL 加载
    #[cfg(feature = "from_http")]
    Url {
        url: String,
    },
    /// 从 GeoJSON feature 的 properties 字段读取
    Prop {
        field: String,
    },
}

/// 可延迟解析的值容器
///
/// T: 目标类型（如 Brush, Stroke, f64, String 等）
/// 序列化时：
///   - 如果 source 是 Inline，值直接内联在 TOML 中
///   - 如果 source 是 File/Url/Prop，value 为默认占位，渲染时 resolve
#[derive(Debug, Clone)]
pub struct LazyValue<T> {
    pub value: T,
    pub source: ValueSource,
}
```

#### 3.1.2 自定义 Serialize/Deserialize

为 `LazyValue<T>` 实现自定义 serde，使得 TOML 中：

```toml
# 简洁写法：直接用 T 的序列化形式，source 自动为 Inline
radius = 0.5
brush = { type = "Solid", color = [46, 139, 87, 255] }

# 完整写法：显式指定来源
radius = { value = 0.5, source = { prop = { field = "radius" } } }

# 引用外部文件
stroke = { source = { file = { path = "styles/thick_stroke.toml" } } }
```

**反序列化逻辑**：
1. 先尝试按 `{ value: T, source: ValueSource }` 结构反序列化
2. 如果失败，尝试直接按 `T` 反序列化，此时 source 默认 `Inline`

#### 3.1.3 Resolve 机制

```rust
impl<T: DeserializeOwned + Default> LazyValue<T> {
    pub async fn resolve(
        &mut self,
        base_path: Option<&Path>,
        http_client: Option<&reqwest::Client>,
    ) -> Result<(), ResolveError> {
        match &self.source {
            ValueSource::Inline => Ok(()),
            ValueSource::File { path } => {
                let full_path = resolve_path(base_path, path);
                let content = tokio::fs::read_to_string(&full_path).await?;
                self.value = toml::from_str(&content)?;
                Ok(())
            }
            #[cfg(feature = "from_http")]
            ValueSource::Url { url } => {
                let resp = http_client
                    .ok_or(ResolveError::NoHttpClient)?
                    .get(url)
                    .send().await?;
                let content = resp.text().await?;
                self.value = toml::from_str(&content)?;
                Ok(())
            }
            ValueSource::Prop { .. } => Ok(()), // defer to convert()
        }
    }
}
```

### 3.2 缓存系统设计（基于 Foyer）

> Foyer 是 Rust 生态的混合缓存库，原生支持内存缓存 + 磁盘缓存，
> 提供 WeightedCache、Admission/Eviction Policy 等高级特性。
> 相比 moka，Foyer 直接内置磁盘缓存，不需要额外拼接文件存储层。

#### 3.2.1 缓存层级（Foyer 一体化）

```
请求
  │
  ▼
┌──────────────────────────────────────────────┐
│ Foyer 混合缓存引擎                             │
│                                              │
│  ┌────────────────────────────────────┐      │
│  │ 内存缓存 (in-memory)                 │      │
│  │  - LayerCache: GeoJSON 解析后的数据   │      │
│  │  - StyleCache: 解析后的 RenderOption │      │
│  │  - TileCache: 渲染好的瓦片图片        │      │
│  │  策略: LFU + TTL + 权重淘汰          │      │
│  └────────────┬───────────────────────┘      │
│               │ 内存满 → 自动 spill          │
│  ┌────────────▼───────────────────────┐      │
│  │ 磁盘缓存 (on-disk)                   │      │
│  │  - 溢出的瓦片 / 冷数据               │      │
│  │  策略: 自动管理，应用无感知           │      │
│  └────────────────────────────────────┘      │
└──────────────────────┬───────────────────────┘
                       │ miss
                       ▼
              ┌─────────────────┐
              │ 数据源           │
              │  - 本地文件      │
              │  - HTTP URL     │
              └─────────────────┘
```

#### 3.2.2 为什么选择 Foyer

| 对比维度 | moka | Foyer |
|---------|------|-------|
| 内存缓存 | ✅ 成熟 | ✅ 成熟 |
| 磁盘缓存 | ❌ 需要自建 L2 | ✅ 内置 disk cache |
| 混合淘汰 | ❌ 需手动串联两层 | ✅ 自动 spill/restore |
| 缓存预热 | ✅ | ✅ + 磁盘持久化重启不丢失 |
| 内存控制 | LRU/TTL | LFU + 权重 + TTL |
| 依赖量 | 1 | 1（内部可能拆分，但对应用透明） |

**结论**：Foyer 的混合缓存能力非常适合 Geello 的使用场景——瓦片数据量大、需要磁盘持久化、冷热数据自动分层。避免自己实现 L1+L2 协调逻辑。

#### 3.2.3 数据结构

```rust
use foyer::{Cache, CacheBuilder, DirectFsDevice};
use std::sync::Arc;

pub struct CacheManager {
    /// GeoJSON 图层缓存 (内存 + 磁盘)
    layer_cache: Cache<String, Arc<CachedLayer>>,

    /// 样式缓存 (内存 + 磁盘)
    style_cache: Cache<String, Arc<CachedStyle>>,

    /// 渲染瓦片缓存 (内存权重 xxMB，溢出到磁盘)
    tile_cache: Cache<TileKey, Arc<Vec<u8>>>,
}

pub struct CachedLayer {
    pub geojson: GeoJson,
    pub source: DataSource,
    pub loaded_at: Instant,
    /// 文件修改时间，用于热加载检测
    pub file_mtime: Option<SystemTime>,
    /// 过期策略
    pub ttl: Option<Duration>,
}

pub struct CachedStyle {
    pub option: RenderOption,
    pub source: DataSource,
    pub loaded_at: Instant,
    pub file_mtime: Option<SystemTime>,
    pub ttl: Option<Duration>,
}

pub struct TileKey {
    pub layer_hash: u64,
    pub style_hash: u64,
    pub z: u32,
    pub x: u32,
    pub y: u32,
}

pub enum DataSource {
    File(PathBuf),
    Url(String),
}

impl CacheManager {
    pub fn new(config: &CacheConfig) -> Self {
        // Foyer 磁盘设备
        let disk_device = DirectFsDevice::new(config.disk_cache_path.clone());

        let layer_cache = CacheBuilder::new()
            .with_name("geello-layer")
            .with_device(disk_device.clone())
            .with_max_capacity(config.layer_cache_max_items)
            .build();

        let style_cache = CacheBuilder::new()
            .with_name("geello-style")
            .with_device(disk_device.clone())
            .with_max_capacity(config.style_cache_max_items)
            .build();

        let tile_cache = CacheBuilder::new()
            .with_name("geello-tile")
            .with_device(disk_device)
            .with_max_capacity(config.tile_cache_max_bytes as usize) // 权重 = 字节数
            .with_weighter(|_key: &TileKey, value: &Arc<Vec<u8>>| value.len())
            .build();

        Self { layer_cache, style_cache, tile_cache }
    }
}
```

#### 3.2.4 缓存容量控制

```toml
# Geello.toml 中的缓存配置
[cache]
# 内存缓存条目上限
layer_cache_max_items = 50
style_cache_max_items = 100

# 瓦片缓存（权重 = 实际字节数），内存中最多 512MB（可根据负载动态调整），超出 spill 到磁盘
tile_cache_max_bytes = 536870912   # 512MB，运行时支持动态调整

# 磁盘缓存
disk_cache_enabled = true
disk_cache_path = "cache"
disk_cache_max_bytes = 10737418240  # 10GB 上限

# 默认过期时间
default_ttl_secs = 86400            # 24 小时
```

#### 3.2.5 并发设计

```
                   ┌──────────────┐
                   │ Axum Handler │
                   └──────┬───────┘
                          │ get_or_load_layer(path)
                          ▼
              ┌───────────────────────┐
              │   CacheManager        │
              │   (Arc<CacheManager>) │
              └───────────┬───────────┘
                          │
          ┌───────────────┼───────────────┐
          ▼               ▼               ▼
   ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
   │ layer_cache │ │ style_cache │ │ tile_cache  │
   │   (Foyer)   │ │   (Foyer)   │ │   (Foyer)   │
   └─────────────┘ └─────────────┘ └─────────────┘
          │               │               │
          └───────────────┼───────────────┘
                          │
                   ┌──────▼──────┐
                   │  Disk Device│  (Foyer 内部管理，应用无感知)
                   └─────────────┘

- Foyer Cache 内部线程安全，无需外层 RwLock
- 对于大对象 (Vec<u8> 瓦片图片)，使用 Arc<Vec<u8>> 避免克隆
- 磁盘 I/O 由 Foyer 内部 tokio 异步线程池处理
```

#### 3.2.6 重启持久化

Foyer disk cache 在服务重启后自动恢复：
1. 磁盘上的瓦片数据不丢失
2. 冷启动时，Foyer 自动将热点数据从磁盘加载到内存
3. 无需手动预热脚本

### 3.3 配置热加载设计

#### 3.3.1 监听范围与粒度

- **粒度**：以文件为单位。一个样式文件变化 → 失效该文件对应的缓存条目
- **样式文件** (`*.toml` in data_path/styles/)：变化后自动重载对应 `RenderOption`
- **全局配置** (`Geello.toml`)：部分字段可热加载（如日志级别），部分需重启（如监听地址）
- **数据文件** (GeoJSON/TIF)：通过缓存 mtime 对比实现"访问时自动刷新"

#### 3.3.2 实现方案

```rust
use notify::{Event, RecursiveMode, Watcher, Config as NotifyConfig};
use tokio::sync::broadcast;

pub struct HotReloader {
    /// 文件变化事件广播
    tx: broadcast::Sender<FileChangeEvent>,
    /// 后台 watcher 任务 handle
    _watcher_task: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone)]
pub enum FileChangeEvent {
    /// 单个样式文件变化，携带文件路径
    StyleFileChanged { path: PathBuf },
    /// 单个数据文件变化
    DataFileChanged { path: PathBuf },
    /// 全局配置文件变化
    GlobalConfigChanged,
}

impl HotReloader {
    pub fn start(watch_paths: Vec<PathBuf>) -> Self {
        let (tx, _) = broadcast::channel::<FileChangeEvent>(256);
        let tx2 = tx.clone();
        let watcher_task = tokio::task::spawn_blocking(move || {
            let mut watcher = notify::recommended_watcher(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        for path in &event.paths {
                            let change = classify_event(path);
                            let _ = tx2.send(change);
                        }
                    }
                }
            ).unwrap();
            for path in &watch_paths {
                watcher.watch(path, RecursiveMode::Recursive).ok();
            }
            std::thread::park();
        });
        Self { tx, _watcher_task: watcher_task }
    }
}
```

#### 3.3.3 可热加载 vs 需重启

| 配置项 | 热加载 | 说明 |
|--------|--------|------|
| 日志级别 | ✅ | tracing subscriber 支持动态重载 filter |
| 样式文件 (单个 .toml) | ✅ | 文件粒度：变化文件对应的缓存失效，下次请求自动加载新版 |
| 数据文件 (GeoJSON/TIF) | ✅ | 通过 mtime 检测，访问时自动重载 |
| 缓存参数 (大小/TTL) | ✅ | 可重建 cache 实例 |
| 监听地址/端口 | ❌ | 需要重新绑定 socket |
| GPU 设备参数 | ❌ | 需要重新初始化 wgpu |
| shader_init_threads | ❌ | vello Renderer 初始化参数 |

### 3.4 GeoTIFF 遥感影像渲染设计

#### 3.4.1 渲染流程

```
GeoTIFF 文件
    │
    ▼
┌──────────────────────────┐
│ 1. 解析 TIFF 结构        │  ← tiff crate
│    - 图像数据 (像素矩阵)  │
│    - 波段数 (单波段/多波段)│
│    - 数据类型 (u8/u16/f32)│
└──────────┬───────────────┘
           ▼
┌──────────────────────────┐
│ 2. 解析地理参考标签       │  ← tiff crate (GeoKey 标签)
│    - 投影坐标系 (CRS)     │
│    - 仿射变换六参数       │
│    - 边界范围             │
└──────────┬───────────────┘
           ▼
┌──────────────────────────┐
│ 3. 投影变换 (如需要)      │  ← proj crate
│    - 源 CRS → 目标 CRS   │
│    - 像素重采样           │
└──────────┬───────────────┘
           ▼
┌──────────────────────────┐
│ 4. 生成 wgpu Texture     │
│    - 像素数据 → RGBA8    │
│    - 色带映射 (多波段)    │
└──────────┬───────────────┘
           ▼
┌──────────────────────────┐
│ 5. vello 渲染到场景      │
│    - 作为图片纹理贴图     │
│    - 应用 transform       │
└──────────────────────────┘
```

#### 3.4.2 RasterRenderer 配置

```toml
# styles/raster.toml
[[renderers]]
type = "Raster"
source = { source = { file = { path = "data/sample.tif" } } }
opacity = 1.0

# 多波段显示：指定 RGB 对应的波段索引 (1-based)
[raster.band_mapping]
red = 1
green = 2
blue = 3

# 或单波段灰度显示
# [raster]
# gray_band = 1
```

#### 3.4.3 RasterRenderer 结构

```rust
pub struct RasterRenderer {
    /// 影像数据源（文件路径或 URL）
    pub source: LazyValue<RasterSource>,
    /// 透明度 0.0 ~ 1.0
    pub opacity: MagicValue<f64>,
    /// 波段映射
    pub band_mapping: BandMapping,
}

pub enum BandMapping {
    /// RGB 三波段
    Rgb { red: usize, green: usize, blue: usize },
    /// 单波段灰度
    Gray { band: usize },
}
```

---

## 四、技术选型变更

### 4.1 依赖变更对照表

| 类别 | 旧 | 新 | 说明 |
|------|-----|-----|------|
| Web 框架 | `rocket` 0.5 | `axum` | 稳定 Rust，活跃生态 |
| WebSocket | `rocket_ws` | `axum` + `tokio-tungstenite` | axum 原生 ws 支持 |
| 序列化格式 | `ron` | `toml` | 更通用可读 |
| 错误处理 | 裸 `String` | `thiserror` | 结构化错误 |
| 日志 | `log` | `tracing` + `tracing-subscriber` | async 生态标准 |
| 配置管理 | `rocket::figment` | `figment` (独立) | 解耦 Rocket |
| 投影变换 | 手写公式 | `proj` crate | 专业 GIS 投影库（用户引入） |
| 缓存 | 自实现 HashMap | `foyer` | 内存+磁盘混合缓存 |
| 热加载 | 无 | `notify` | 文件系统监听 |
| 字体 | `skrifa` | 保留 `skrifa` | 文字渲染 |
| TIFF/GeoTIFF | 无 | `tiff` | 遥感影像支持，地理参考标签由 tiff 库解析（raster feature） |

> **注意**：用户自行负责依赖版本升级，本计划只确定引入/移除哪些依赖。

### 4.2 Axum 路由设计

```rust
use axum::{Router, routing::get};
use tower_http::services::ServeDir;

pub fn build_router(state: AppState) -> Router {
    let public_routes = Router::new()
        // WMTS
        .route("/wmts/real-time", get(wmts::real_time))
        .route("/wmts/cache", get(wmts::cache))
        // WMS
        .route("/wms", get(wms::real_time))
        // WebSocket (GET upgrade)
        .route("/ws/anim", get(websocket::anim_upgrade))
        // 静态前端 — Leaflet
        .nest_service("/map", ServeDir::new("assets/web-map"));

    let admin_routes = Router::new()
        .route("/cache/stats", get(admin::cache_stats))
        .route("/example/{name}", get(admin::render_option_example))
        .route("/health", get(admin::health_check));

    Router::new()
        .merge(public_routes)
        .merge(admin_routes)
        .with_state(state)
}
```

### 4.3 AppState 设计

```rust
/// 全局应用状态，Axum 中通过 `State<AppState>` 注入
pub struct AppState {
    /// GPU 设备句柄列表（当前单设备，预留多 GPU 扩展）
    pub devices: Vec<DeviceHandle>,
    /// 默认设备（等于 devices[0]）
    pub default_device: wgpu::Device,
    /// 默认命令队列
    pub default_queue: wgpu::Queue,
    /// WMTS 渲染用纹理池
    pub texture_pool: Arc<TexturePool>,
    /// 缓存管理器
    pub cache: Arc<CacheManager>,
    /// 配置（支持热加载部分字段）
    pub config: Arc<RwLock<ServerConfig>>,
    /// HTTP 客户端 (用于远程数据)
    pub http_client: reqwest::Client,
}

/// 单个 GPU 设备句柄
pub struct DeviceHandle {
    pub id: usize,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
}

/// 纹理池：多个预创建的 Texture 供 WMTS 请求复用
pub struct TexturePool {
    textures: Mutex<Vec<wgpu::Texture>>,
    semaphore: tokio::sync::Semaphore,
}
```

### 4.4 Docker 多阶段构建 (参考用户方案)

```dockerfile
# ============================================================
# Stage 1: 安装 cargo-chef（依赖分析工具）
# ============================================================
FROM rust:1.95-alpine AS chef

RUN apk add --no-cache musl-dev pkgconfig
RUN cargo install cargo-chef --locked

WORKDIR /geello

# ============================================================
# Stage 2: 生成 recipe.json（依赖配方文件）
# ============================================================
FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

# ============================================================
# Stage 3: 使用 recipe.json 构建依赖并编译项目
# ============================================================
FROM chef AS builder

COPY --from=planner /geello/recipe.json recipe.json

# 先构建依赖（此层被 Docker 缓存，只有 Cargo.toml/lock 变化时才重跑）
RUN cargo chef cook --release \
    --target x86_64-unknown-linux-musl \
    --recipe-path recipe.json \
    --features server

# 复制完整源码并构建
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets

RUN cargo build --release \
    --target x86_64-unknown-linux-musl \
    --features server

# ============================================================
# Stage 4: 极简运行环境
# ============================================================
FROM alpine:3.22

RUN apk add --no-cache \
    libgcc \
    vulkan-loader \
    ca-certificates

WORKDIR /geello

COPY --from=builder \
    /geello/target/x86_64-unknown-linux-musl/release/geello .

COPY assets ./assets
COPY config ./config

EXPOSE 8000

# GPU 环境需要 vulkan，参考 README 中 NVIDIA Container Toolkit 说明
VOLUME ["/geello/cache"]

ENTRYPOINT ["./geello"]
```

#### 额外考虑事项

- **Vulkan 依赖**：alpine 需要 `vulkan-loader`，运行容器时需要 GPU passthrough 或软件 Vulkan (如 `lavapipe`)
- **GPU 容器**：保留 README 中 NVIDIA Container Toolkit + xhost 的文档说明
- **CI 缓存**：GitHub Actions 中使用 `actions/cache` 缓存 `~/.cargo` 和 `target/` 目录
- **多架构构建**：后续可扩展 `--target aarch64-unknown-linux-musl`

---

## 五、实施路线图

### Phase 0：基础设施准备（不动功能）
- [ ] 引入 `thiserror`，定义 `GeelloError` 错误枚举
- [ ] 引入 `tracing` + `tracing-subscriber` 替代 `log`
- [ ] 整理 `rustfmt.toml` 和 `clippy.toml`

### Phase 1：模块重组 + RON → TOML
- [ ] 按 2.2 节 mod map 重组目录结构
- [ ] 移除 `ron` 依赖，引入 `toml` + `serde`
- [ ] 重写序列化/反序列化适配 TOML 格式
- [ ] 迁移 `assets/test/*.ron` 为 `assets/test/styles/*.toml`
- [ ] 重设计 `MagicValue` → `LazyValue<T>` 系统
- [ ] 用 `thiserror` 替换所有 `Result<_, String>` 为 `Result<_, GeelloError>`

### Phase 2：渲染层重构
- [ ] 清理 `device.rs`：统一 GPU 初始化流程，预留多 GPU 接口
- [ ] 重构 `GeometryRenderer`：添加 `TextRenderer`、`RasterRenderer` 变体
- [ ] 实现 `TextRenderer`（基于 skrifa + vello glyph）— 优先
- [ ] 实现 `RasterRenderer`（GeoTIFF 加载 + 纹理渲染）— feature="raster"
- [ ] 优化 `RenderedGeometry` 惰性计算逻辑
- [ ] 消除 `block_on_wgpu` 阻塞问题（考虑 spawn_blocking 或专用线程）
- [ ] 保证 library 模式 API 可用

### Phase 3：Server 用 Axum 重写
- [ ] 创建 `server/` 模块树
- [ ] 实现 `AppState` + Axum Router
- [ ] 重写 WMTS/WMS 路由
- [ ] 重写 WebSocket 实时动画路由
- [ ] 实现混合缓存系统（Foyer：内存 + 磁盘）
- [ ] 实现配置热加载（notify，文件粒度）
- [ ] 保留 Leaflet 静态前端文件服务

### Phase 4：Docker + CI/CD
- [ ] 编写多阶段 Dockerfile（cargo-chef + musl + alpine）
- [ ] 更新 `.github/workflows/docker-publish.yml`
- [ ] 验证 Docker 镜像构建和运行
- [ ] 更新 README 中的 Docker 运行说明

### Phase 5：Proj 集成 + 投影扩展（用户主导）
- [ ] 引入 `proj` crate，替换手写 EPSG:3857/4326 公式
- [ ] 扩展更多投影支持（Web Mercator、UTM 等）
- [ ] 投影性能优化

### Phase 6：质量与文档
- [ ] 添加单元测试（core 模块优先）
- [ ] 添加集成测试（server 请求测试）
- [ ] 完善 API 文档 (`cargo doc`)
- [ ] 编写使用指南和示例

---

## 六、待讨论问题

## 六、已确认决策（不再讨论）

以下事项经多轮反馈已确定：

| 决策 | 结论 |
|------|------|
| Foyer | 唯一缓存方案，不回退 moka |
| Leaflet 前端 | 保留，暂不改造 |
| 字体加载 | 运行时从 assets 加载，不嵌入 binary |
| 缓存容量 | 支持运行时动态调整 |
| API 鉴权 | 暂不实现 |
| TIFF 地理参考 | 统一由 `tiff` crate 解析 GeoKey 标签 |
| Edition | 保持 2024 |
| CPU 降级 | 不支持 |
| 向后兼容 | 全面 breaking change |
| 热加载粒度 | 以文件为单位 |
| 多 GPU 调度 | 默认**请求轮询**，预留 per-layer `gpu_id` 可选固定绑定 |
| 字体 fallback | 按**配置顺序**遍历字体列表，取首个覆盖所需 Unicode 码位的字体 |

## 七、远期规划（不做入当前 Phase）

| 功能 | 说明 |
|------|------|
| GeoTIFF 波段计算 | NDVI、NDWI 等假彩色合成 |
| GeoTIFF 拉伸算法 | 直方图拉伸、百分位拉伸等 |
| GeoTIFF γ 校正 | Gamma correction for display |
| 更多投影支持 | 当前仅 EPSG:3857/4326，后续扩展 UTM、Lambert 等 |

---

## 八、附录

### A. 现有 RON → TOML 迁移对照

**3857.ron (旧)**:
```ron
(
    region: All,
    pixel_option: (width: 256, height: 256, base_color: (...)),
    renderers: [
        Area(r#None, (brush: Solid((...)), line_renderers: {})),
        Line(r#None, (stroke: (...), brush: Solid((...)), node_renderers: {})),
        Point(Layer("china.geo.json"), (radius: 0.1, brush: Solid((...)))),
    ],
    tile_proj: EPSG3857,
    need_proj_geom: true,
)
```

**3857.toml (新)**:
```toml
tile_proj = "EPSG3857"
need_proj_geom = true

[pixel]
width = 256
height = 256
base_color = [0, 0, 0, 0]

[[renderers]]
type = "Area"
brush = { type = "Solid", color = [46, 139, 87, 255] }

[[renderers]]
type = "Line"
stroke = { width = 0.1, join = "Round", start_cap = "Round", end_cap = "Round" }
brush = { type = "Solid", color = [0, 255, 255, 255] }

[[renderers]]
type = "Point"
filter = { layer = "china.geo.json" }
radius = 0.1
brush = { type = "Solid", color = [255, 255, 224, 255] }
```

### B. 参考资源

- [Axum 文档](https://docs.rs/axum/latest/axum/)
- [TOML 规范](https://toml.io/)
- [vello GitHub](https://github.com/linebender/vello)
- [Foyer 混合缓存](https://github.com/MrCroxx/foyer)
- [notify (文件监听)](https://docs.rs/notify/latest/notify/)
- [cargo-chef (Docker 构建优化)](https://github.com/LukeMathWalker/cargo-chef)
- [tracing 文档](https://docs.rs/tracing/latest/tracing/)
- [proj Rust 绑定](https://docs.rs/proj/latest/proj/)
- [tiff crate](https://docs.rs/tiff/latest/tiff/)

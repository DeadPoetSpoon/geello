# Geello — Requirements Document

> 基于现有代码库 (v0.2.0) 整理的功能需求文档。

---

## 1. 项目目标

Geello 是使用 GPU 加速的地理数据渲染工具，支持两种模式：

- **REQ-001: Library 模式** — 作为 Rust 库使用，提供同步/异步渲染 API
- **REQ-002: Server 模式** — 作为 HTTP 服务运行，提供 WMTS/WMS 风格的地图瓦片接口

---

## 2. 数据输入

### REQ-102: Workspace/Layer 身份系统

系统必须支持 workspace:layer 两级命名空间逻辑标识图层，解耦数据来源。

- **REQ-102a**: `LayerId` 结构体 (workspace, name) 作为图层唯一标识
- **REQ-102b**: `LayerSource` 枚举支持 `FileSystem` / `Http` (feature: `from_http`) / `Samba` (feature: `from_samba`)
- **REQ-102c**: `WorkspaceRegistry` 维护 LayerId → LayerSource 的全局映射，对齐 foyer 缓存键设计
- **REQ-102d**: `RenderedLayer` 封装 LayerId + LayerSource + Vec<RenderedGeometry>，作为缓存的原子单元

### REQ-100: GeoJSON 数据加载

系统必须能够加载和解析 GeoJSON 格式的地理数据。

- **REQ-100a**: 支持从本地文件系统加载 `.geojson` / `.json` 文件
- **REQ-100b**: 当启用 `from_http` feature 时，支持从 HTTP URL 加载
- **REQ-100c**: 支持 `Feature`、`FeatureCollection`、`Geometry` 三种 GeoJSON 根类型
- **REQ-100d**: 支持 `,` 分隔的多图层加载，按顺序渲染

### REQ-101: GeoTIFF 数据加载

当启用 `raster` feature 时，系统必须能够加载 GeoTIFF 遥感影像。

- **REQ-101a**: 支持本地 `.tif` / `.tiff` 文件
- **REQ-101b**: 支持多波段 RGB 合成
- **REQ-101c**: 支持单波段灰度渲染
- **REQ-101d**: 支持 8-bit 和 16-bit 像素格式

---

## 3. 样式配置

### REQ-200: Style 样式文件

系统必须使用 TOML 格式定义渲染样式，采用三层结构 `Style → LayerStyle → RendererDef`。

- **REQ-200a**: `Style` — 顶层容器，含 `[[layers]]` 数组，每项为一个图层的完整渲染配置
- **REQ-200b**: `LayerStyle` — 单图层配置，含 `filter`（LayerFilter: workspace + name）和 `renderers` 列表
- **REQ-200c**: `RendererDef` — 单个渲染器定义，按类型 (point/line/area/text/raster) 携带对应 Config，可选 `geometry_filter`
- **REQ-200d**: 所有字段均有默认值，目标为增量配置：文件不存在时全默认，未设置项不序列化写入
- **REQ-200e**: `renderers` 按顺序分层绘制

### REQ-201: LazyValue 延迟解析系统

配置中的值必须支持从多种来源延迟加载。

- **REQ-201a**: `Inline` — 值直接内联在配置中
- **REQ-201b**: `File(path)` — 从 TOML 文件加载
- **REQ-201c**: `Toml(content)` — 从内联 TOML 字符串解析
- **REQ-201d**: `Prop(field, encoding)` — 从 GeoJSON feature properties 读取
- **REQ-201e**: `JsonFile(path)` 从 JSON 文件加载
- **REQ-201f**: `Url(url, encoding)` 从 HTTP URL 加载 (feature: `from_http`)

### REQ-202: 渲染器类型

系统必须支持以下渲染器类型：

- **REQ-202a**: `PointRenderer` — 点渲染，Circle 填充，可配置半径和颜色
- **REQ-202b**: `LineRenderer` — 线渲染，BezPath stroke，可配置线宽/join/cap/颜色
- **REQ-202c**: `AreaRenderer` — 面渲染，polygon fill，可配置填充色和边界线
- **REQ-202d**: `TextRenderer` — 文字标注，基于 skrifa 字体引擎，可配置字体/字号/颜色/锚点
- **REQ-202e**: `RasterRenderer` — 遥感影像渲染 (feature: `raster`)

### REQ-203: 几何过滤器

每个渲染器必须支持按几何属性或空间位置过滤。

- **REQ-203a**: `prop` — 按 feature property 筛选（name + Eq + value）
- **REQ-203b**: `bbox` — 按几何包围盒矩形筛选（min_x, min_y, max_x, max_y）
- **REQ-203c**: 两种条件可组合（AND），均未设置时渲染所有几何

---

## 4. 渲染引擎

### REQ-300: GPU 渲染管线

系统必须使用 vello + wgpu 进行 GPU 加速渲染。

- **REQ-300a**: 构建 `vello::Scene`，将几何转换为 GPU 路径
- **REQ-300b**: 使用 `vello::Renderer::render_to_texture` 渲染到 wgpu Texture
- **REQ-300c**: 支持从 Texture 读取 RGBA 像素到 CPU 缓冲区

### REQ-301: 同步渲染 API

必须提供阻塞式渲染函数供 Library 模式使用。

- **REQ-301a**: `render_to_texture()` — 渲染到已有纹理
- **REQ-301b**: `render_to_buffer()` — 渲染到 CPU 缓冲区 (使用 `block_on_wgpu`)
- **REQ-301c**: `render_to_buffer_with_new_texture()` — 创建新纹理并渲染到缓冲区

### REQ-302: 异步渲染 API

必须提供非阻塞渲染函数供 Server 模式使用。

- **REQ-302a**: `render_to_buffer_async()` — 使用 `spawn_blocking` 避免阻塞 async runtime
- **REQ-302b**: `render_to_buffer_with_new_texture_async()` — 异步版本含纹理创建

### REQ-303: 图像编码

渲染结果必须编码为标准图像格式。

- **REQ-303a**: PNG 编码
- **REQ-303b**: JPEG 编码
- **REQ-303c**: WebP 编码（保留接口）

### REQ-304: 投影变换

系统必须支持坐标投影变换。

- **REQ-304a**: EPSG:4326 (WGS84) ↔ EPSG:3857 (Web Mercator) 双向变换
- **REQ-304b**: WMTS 瓦片索引 (x, y, z) → 地理矩形坐标

---

## 5. HTTP 服务

当启用 `server` feature 时必须满足以下需求。

### REQ-400: WMTS 接口

- **REQ-400a**: `GET /wmts/real-time?layers=&styles=&x=&y=&z=&format=` — 实时渲染瓦片
- **REQ-400b**: `GET /wmts/cache?layers=&styles=&x=&y=&z=&format=` — 带磁盘缓存的瓦片
- **REQ-400c**: 参数: layers (`,` 分隔), styles, x, y, z, format (默认 png)

### REQ-401: WMS 接口

- **REQ-401a**: `GET /wms?layers=&styles=&width=&height=&bbox=&format=` — 按 bbox 渲染
- **REQ-401b**: bbox 格式: `minx,miny,maxx,maxy`

### REQ-402: WebSocket 动画

- **REQ-402a**: `WS /ws/anim?layers=&styles=&width=&height=&bbox=&format=` — 实时动画帧

### REQ-403: 管理接口

- **REQ-403a**: `GET /health` — 健康检查 (200 OK)
- **REQ-403b**: `GET /cache/stats` — 缓存统计 JSON
- **REQ-403c**: `GET /example/{name}` — 渲染器配置示例 TOML

### REQ-404: 静态文件服务

- **REQ-404a**: `GET /map/{*path}` — Leaflet 前端示例

---

## 6. 缓存系统

当启用 `server` feature 时必须满足以下需求。

### REQ-500: 分层缓存

- **REQ-500a**: Layer 缓存 — `LayerCache` (foyer 内存 LRU)，已提升为核心模块 (REQ ✓)
- **REQ-500b**: Style 缓存 — 缓存解析后的 TOML 样式 (内存)
- **REQ-500c**: Tile 缓存 — 缓存渲染后的瓦片图像 (内存 + 磁盘混合)

### REQ-501: 缓存新鲜度

- **REQ-501a**: 支持基于 TTL (秒) 的过期策略
- **REQ-501b**: 支持基于文件 mtime 的失效检测

### REQ-502: 缓存失效

- **REQ-502a**: 提供手动失效 API (`remove_layer`, `remove_style`)
- **REQ-502b**: 文件变更自动失效（热重载）

---

## 7. 配置管理

### REQ-600: 运行时配置

- **REQ-600a**: TOML 文件配置 (`Geello.toml`)
- **REQ-600b**: 环境变量覆盖 (`GEELLO_` 前缀)
- **REQ-600c**: 配置项: address, port, data_path, cache_path, wmts_texture_count

### REQ-601: 热重载

- **REQ-601a**: 监听数据文件目录的变更 (notify)
- **REQ-601b**: `.toml` 变更 → 样式缓存失效
- **REQ-601c**: `.geojson`/`.json`/`.tif` 变更 → 数据缓存失效

---

## 8. GPU 设备管理

### REQ-700: 设备初始化

- **REQ-700a**: 自动检测并初始化 wgpu 设备
- **REQ-700b**: 预留多 GPU 接口 (当前仅使用第一个设备)

### REQ-701: 纹理池

- **REQ-701a**: 预分配指定数量的可复用 Texture
- **REQ-701b**: Semaphore 限流并发请求
- **REQ-701c**: acquire/release 接口

---

## 9. 错误处理

### REQ-800: 统一错误类型

- **REQ-800a**: `GeelloError` 枚举覆盖所有错误分类 (14 种)
- **REQ-800b**: 公共 API 全部返回 `GeelloResult<T>`
- **REQ-800c**: Server 层 `ServerError` 自动转换为 HTTP 状态码

---

## 10. Feature Flags

### REQ-900: 条件编译

| Feature     | 需求                                          |
|-------------|-----------------------------------------------|
| `default`   | core (foyer/geojson/serde_json 已内置)        |
| `server`    | Axum HTTP 服务 + 热重载                       |
| `from_http` | LayerSource::Http + LayerCache HTTP client + LazyValue HTTP 源 |
| `from_samba` | LayerSource::Samba (预留)                     |
| `raster`    | GeoTIFF 加载与渲染                            |
| `full`      | 全功能 (`server` + `from_http` + `from_samba` + `raster`) |

---

## 11. 部署

### REQ-1000: Docker

- **REQ-1000a**: 多阶段构建 (cargo-chef + musl + alpine)
- **REQ-1000b**: 需要 NVIDIA Container Toolkit (GPU 访问)
- **REQ-1000c**: 暴露端口 8000

### REQ-1001: CI/CD

- **REQ-1001a**: tag push → Docker build + cosign sign → ghcr.io

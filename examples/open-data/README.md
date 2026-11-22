# 公开矢量与栅格测试数据

数据及输出位于被 Git 忽略的 `assets/test/open-data/`；不提交二进制下载。
下载地址、SHA-256、来源与许可记录于 [sources.json](sources.json)。
所有上传使用 `open:*` 命名空间，不覆盖既有 `demo:*` / `preview:*` 资源。

## 数据与许可

| 数据 | 类型 / 规模 | 许可与测试用途 |
| --- | --- | --- |
| Natural Earth 110m Admin 0 Countries | 177 个 Polygon / MultiPolygon 要素 | Public domain；面填充、多环国界、WFS 属性与分页 |
| Natural Earth 110m Populated Places | 243 个城市点 | Public domain；点符号、ASCII 城市名注记、瓦片碰撞 |
| Natural Earth 110m Rivers + Lake Centerlines | 13 个河流线要素 | Public domain；线渲染与多图层叠加 |
| Natural Earth I shaded relief，经 Cartopy 分发 | 720 × 360 RGB 全球地形底图 | Public domain；栅格采样、CRS84 / Web Mercator 重投影 |
| Rasterio `RGB.byte.tif`，源自 USGS Landsat 7 ETM | 791 × 718 RGB GeoTIFF，约 1.7 MB，EPSG:32618 | Rasterio 测试数据 README 明确采用 CC0 1.0；真实遥感影像、UTM 重投影、NoData |

Natural Earth 的 [使用条款](https://www.naturalearthdata.com/about/terms-of-use/)
明确覆盖其矢量和栅格数据，允许修改与再分发。
地形底图并非近期卫星观测影像；小型 Landsat 样本亦非当前影像或原始全分辨率场景，
适合功能验证，不适合高分辨率影像吞吐基准。
Rasterio 的许可依据是其 [测试数据说明](https://github.com/rasterio/rasterio/blob/main/tests/data/README.rst)，
不是仅从仓库代码许可证推断。
Cartopy 的 [images.json](https://github.com/SciTools/cartopy/blob/main/lib/cartopy/data/raster/natural_earth/images.json)
记录地形底图的 Natural Earth 来源及 Plate Carree 投影。
字体使用仓库中已有的 OFL 测试字体，只显示 `NAMEASCII` 字段。
这些国界仅作为测试输入，不代表权威边界表达。

## 可重复运行

Python 需安装 Pillow（当前机器已具备）。在仓库根目录运行。
如已经启动本项目，跳过服务启动命令，直接向现有服务发布。
新机器或独立测试可以使用本目录配置，其数据目录独立于现有本机目录：

```bash
cargo build --release --locked --features server-full
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/nvidia_icd.json \
  ./target/release/geello --config examples/open-data/server.toml
```

`VK_DRIVER_FILES` 是本机 NVIDIA Vulkan ICD 的路径；其他机器按实际驱动设置或省略。
`backend = "gpu"` 要求真实 GPU，不自动降级到 CPU。
在其他终端启动预览并发布、验证：

```bash
python -B tools/preview_server.py --upstream http://127.0.0.1:3000 --port 8080
python -B tools/publish_open_data_demo.py --require-gpu --preview http://127.0.0.1:8080
```

下载会校验固定 SHA-256；已有文件也会重新校验。源文件不做就地修改。
重复发布通过现有资源 ETag 条件更新。
如只需要准备数据或对已发布资源复测：

```bash
python -B tools/publish_open_data_demo.py --prepare-only
python -B tools/publish_open_data_demo.py --verify-only --require-gpu --preview http://127.0.0.1:8080
```

## 预览

- [纯矢量：面 / 线 / 点 / 城市注记](http://127.0.0.1:8080/?map=open%3Avector)
- [Natural Earth 地形底图](http://127.0.0.1:8080/?map=open%3Araster)
- [地形 + 国界 + 河流 + 城市注记](http://127.0.0.1:8080/?map=open%3Aoverlay)
- [真实 Landsat 影像](http://127.0.0.1:8080/?map=open%3Alandsat)
- [叠加图 WMTS 瓦片](http://127.0.0.1:8080/?map=open%3Aoverlay&protocol=wmts)
- [国界 WFS 属性与客户端矢量](http://127.0.0.1:8080/?map=open%3Aoverlay&protocol=wfs)

预览的图层列表也能切换以上地图；Landsat 自动定位到其加勒比海覆盖区域。
WFS 模式加载国界要素与属性，由 OpenLayers 客户端描绘边线，背景仍由服务器输出。
地图 / WMS / WMTS 模式则由服务器以 GPU 渲染。

## 派生处理与本机预算

- 原始 ZIP 保留不变。上传包只包含 SHP / SHX / DBF / PRJ / CPG，
  去除服务器不接受的 README / VERSION 成员。
- 国界原始数据有两个经度为 `180.00000000000006` 的顶点；派生包将此浮点越界归入 ±180°。
  南极极点顶点限制到 Web Mercator 的 ±85.0511287798066°，并同步调整 SHP / SHX 包围盒。
  保留全部 177 个要素及原属性；这不是通用 GIS 拓扑裁剪器，不适用于其他数据的自动修复。
- Cartopy 分发的 720 × 360 全球 PNG 包装为不压缩 RGB GeoTIFF，写入 EPSG:4326、
  PixelIsArea、像元大小和左上角锚点。没有把网页缩略图冒充原始高分辨率影像。
- Landsat 原始 GeoTIFF 原样上传，保留其 UTM 网格与 NoData，不经过降采样或重新编码。
- 当前本机 `assets/test/data-development/server.gpu.toml` 使用与示例相同的关键预算：
  `max_live_bytes = 1073741824`、`shard_features = 32`、`queued_per_class = 8`。
  复杂属性增加保守内存核算；缩小分片避免一次处理全部要素。
  队列为瓦片预热及前台请求保留余量，预览仍只同时转发两路请求。
  这些是本机测试设置，不修改部署配置，也不保证任意负载下不会出现 503。
- 本机服务的原始上传、派生数据、注册表与响应缓存统一位于
  `assets/test/data-development/`，与下载数据一起由现有 Git 忽略规则排除。

## 验证与输出

脚本验证四张地图在 CRS84 / EPSG:3857 的出图、WMS 1.3.0 经纬轴序及 JPEG、
两套矩阵的 WMTS PNG，检查图像可解码且区域出图不为单色。
通过 `--require-gpu` 同时核对健康检查和直接出图的 GPU 响应头。
三份矢量的 WFS 总数必须匹配 177 / 243 / 13，Features 首页必须返回 5 个要素。
传入 `--preview` 后增加八路并发 WMTS，使用新的时间参数避免命中之前的成图缓存；
预览代理不透传 GPU 响应头，该项只检查状态及 PNG，不单独声称验证了响应头。

输出为 `assets/test/open-data/results/*.png`、`*.jpg` 和 `report.json`。
报告记录实际后端、尺寸、响应字节数、请求时间及最终健康状态。
这是在线功能检查，不是冷磁盘、持续高并发或注记接缝的自动像素对比基准。

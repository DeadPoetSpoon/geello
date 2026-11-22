# 内置管理台使用与部署

管理台与 API、渲染服务运行在同一个 Geello 进程。入口为 `/admin/`，不需要启动
`tools/preview_server.py`。原独立预览示例仍保留，供独立调试使用。

## 启动

```sh
cargo build --release --features server-full
./target/release/geello --config examples/open-data/server.toml
```

打开 `http://127.0.0.1:3000/admin/`。已有部署可以直接使用自己的配置文件、数据目录与
CPU/GPU 选择，不需迁移目录或重新登记资源。GPU 是否真正启用，以概览中的实际后端为准。
离线 OpenLayers 10.7.0 的原发行文件、许可证和 SHA256 位于
`src/server/admin/assets/vendor/`；页面没有 CDN、外部底图、UI 库或 Node 构建步骤。

## 工作流

1. **数据集**：新建 `namespace:local` ID，选择 GeoJSON、Shapefile ZIP、GeoTIFF 或数据集
   TOML。可填写编码、源 CRS 与栅格样本解释。GeoTIFF 必须明确选择连续或分类解释。
2. **字体 / 样式**：上传 TTF、OTF 或集合；创建点、线、面、RGB 栅格或注记样式模板。
   注记模板中的字体 ID 和属性字段需替换为实际资源。可直接编辑完整 TOML。
3. **地图**：新建地图，检查并载入辅助表单，设置发布 CRS、矩阵集、标题与背景，添加已发布
   数据集和样式。上移、下移、拖放均移动完整图层对象；动画和几何绑定不被丢弃。
4. **保存并发布**：保存不是服务器草稿。上传百分比仅表示原文件传输；随后是独立的准备任务。
   `ready` 才是发布成功，失败请查看任务诊断。旧发布不会因新准备失败而被替换。
5. **预览**：从已发布地图进入 OL 预览。优先使用声明的 WMTS 矩阵集，否则使用 OGC Maps。
   可切换 WMS、CRS84/WebMercator、手动范围和显式时间；不进行连续动画播放。
   透明棋盘格不等于服务失败，源数据没有覆盖的区域可能正常返回透明图像。
   纯 Rust 投影下，局部投影栅格在世界范围内无法反投影的像素按 NoData 处理，
   不再使整张图像返回 422；非法 CRS、投影配置等错误仍会明确报错。
   输入覆盖影像的局部 bbox 后定位可放大影像；失败后显式定位或点击“重试预览”可重新请求。
   不会为某个示例影像硬编码默认视图，也不会自动重试失败请求。
6. **属性**：只对地图依赖中已公开的矢量集合启用 Features 高亮。每页最多 100 个，
   受服务端分页配置限制，每次检查最多 1000 个；有后续数据时明确提示截断。点击高亮查看属性。

配置检查只验证 TOML 及模型，不提前保证目录依赖、字体或渲染结果。未经表单转换的文本原样
提交。表单转换使用完整 TOML 值树，但注释、空白、键顺序可能重排；转换前需确认，并可在
当前会话恢复原文本。不能安全表示为浏览器 JSON 的配置只能保留原 TOML 编辑，不强行转换。

替换、下线、重新发布、删除均使用原编辑基线 ETag。遇到 409 保留本地输入及基线，并显示
当前版本，不悄悄覆盖。点击重新加载会明确放弃当前修改。浏览器不在 localStorage 或
sessionStorage 保存定义、服务器路径、文件或凭证；刷新不能恢复文件选择。

下线、删除记录和删除托管内容是三个不同动作，均不级联。删除记录保留原始托管文件；
删除托管内容要求输入完整 ID 且不可撤销；外部内容不能用该动作删除。
重新发布重新读取已登记源，保持原所有权，并再次检查外部根目录及最终 ETag。

## 反向代理与认证

管理台没有账号系统。必须用反向代理认证同时保护 `/admin/`、`/admin/assets/`、
`/admin/v1`，不能只保护 API 或只隐藏页面入口。服务默认监听回环地址；不要未经认证公开管理入口。
若管理路径不供公网使用，使用网络隔离、VPN 或单独的运维代理。

以下 Nginx 示例假设 `public_base_url = "https://maps.example.com/geello/"`，代理去掉
`/geello/` 后转发。认证文件需要部署方自行创建，TLS 设置也由部署方提供。

```nginx
location ^~ /geello/admin/ {
    auth_basic "Geello administration";
    auth_basic_user_file /etc/nginx/geello.htpasswd;
    proxy_pass http://127.0.0.1:3000/admin/;
    proxy_set_header Host $host;
    client_max_body_size 16m;
}
location = /geello/admin {
    return 307 /geello/admin/;
}
location /geello/ {
    proxy_pass http://127.0.0.1:3000/;
    proxy_set_header Host $host;
}
```

浏览器页面、API、图像均从当前管理页面推导同源服务前缀，不使用公开配置地址作为任意代理。
将 `public_base_url` 的协议、主机、端口配置为浏览器实际访问的公开来源，否则浏览器写请求
返回 403。`Sec-Fetch-Site: cross-site` 同样被拒绝。无 Origin 的现有 CLI 脚本保持兼容；
Origin 检查不能替代认证。不要用 CORS 放开管理写入。

页面返回 CSP、nosniff、防嵌入头；动态文本使用 DOM 文本节点。CSP 允许内联结构样式，
因为 OL 通过元素样式定位画布和控件，但不允许内联脚本。
仅提供编译期白名单静态资源，没有磁盘目录或 API 到 HTML 的兜底。

## 请求预算与排错

本轮性能实现、空间筛选回退条件、高分屏图像参数和 WebSocket 连接配置见
[代码审阅优化实施记录](performance-optimizations.md)。

共享调度器覆盖管理 fetch、原始文件 XHR 和 OL 图像。总并发最多 2，预览最多 1，待执行
最多 64。若服务端 `active_http = 1`，管理台保守串行。写操作优先于管理读取，管理读取
优先于预览；已发出的请求要等响应体实际完成才释放槽位，过期结果不会回写新视图。
不会自动重试写入或失败图像。任务每 2 秒更新，健康每 10 秒更新，隐藏标签页暂停，
读取失败退避 2/4/8/16/30 秒。

- **HTTP 503**：显示服务端 JSON/XML/文本及请求 ID，不推断不存在的精确故障原因。
  检查实际预算、其他客户端、后台准备和 GPU 日志；停止操作后手动重试，不增加并发形成重试风暴。
- **HTTP 409**：处理引用或版本冲突，比较本地文本和当前版本，不直接换新 ETag 强行提交。
- **HTTP 401/403 或登录 HTML**：检查代理认证和公开来源配置，重新登录后再查询任务状态。
- **准备失败**：输入仍保留在详情页，任务列表给出真实诊断；不是“上传成功即发布成功”。
- **其他 CRS**：页面列出发布 CRS；非 CRS84/3857 通过正式服务调用，不伪造客户端支持。

## 验证

```sh
cargo test --features server-full
node --test tests/admin_requests.test.mjs
python tools/generate_http_docs.py --check
cd src/server/admin/assets/vendor && sha256sum -c SHA256SUMS
```

浏览器检查脚本仅依赖现代 Node 的内置 fetch/WebSocket 和已安装的 Chromium，不引入 npm 包：

```sh
chromium --headless --disable-gpu --remote-debugging-port=9223 \
  --user-data-dir=/tmp/geello-admin-browser about:blank
node tools/verify_admin_ui.mjs
```

`--disable-gpu` 只关闭测试浏览器 GPU，不会关闭服务器 GPU。脚本上传临时点数据、字体，
创建样式、动画和带绑定地图，检查发布、表单语义保留、三种预览、属性、连续协议切换、
窄屏和并发；最后按依赖逆序删除自己创建的托管测试资源。任务历史保留。
截图、报告和临时数据默认位于已忽略的 `assets/test/admin-ui/results/`。
可用 `GEELLO_ADMIN_BASE`、`CHROME_DEBUG_URL`、`GEELLO_ADMIN_RESULTS` 指定服务前缀及输出。
应指向独立的测试服务，不要对现有业务服务运行会创建/删除资源的浏览器验收。
设置 `GEELLO_ADMIN_DEVICE_SCALE_FACTOR=2` 可验收高分屏；默认值为 1。
脚本检查实际画布的点标记像素宽度和请求比例，防止单图放置拉伸或像素样式漏缩放。
设置 `GEELLO_OPEN_DATA_MAPS=open:vector,open:raster,open:overlay,open:landsat` 可额外检查已发布开放数据。
检查不再豁免局部 UTM 影像的 422；先验证初始世界范围瓦片，再验证世界范围 OGC Maps/WMS
和局部影像显示。脚本还注入预览错误，验证停止自动请求以及显式定位/全图恢复。

契约测试覆盖静态资源、Origin、分页/筛选、配置语义、强 ETag、托管重新发布失败保护与外部
所有权。自动检查不等于完整的屏幕阅读器、真实认证设施或任意负载验证；详细状态见设计文档。

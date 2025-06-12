# Geello

Geello (Ge**O_V**ello) try to render geo data through [GEO](https://github.com/georust/geo) and [VELLO](https://github.com/linebender/vello)

> [!WARNING]
> Geello is a testing project in an alpha state.
>

### As a library

Geello can be used as a library to render geo data into a texture.

```rust
geello::render_to_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    transform: Affine,
    option: &RenderOption,
)
```
### As a server

Geello can be used as a server to provide map render (real-time/cache) services like WMTS/WMS.

```bash
cargo r --release --features server
```

WMTS LIKE
```url

/wmts/real-time?data=/path/to/json&style=/path/to/render_option&x={x}&y={y}&z={z}&format=png

/wmts/cache?data=/path/to/json&style=/path/to/render_option&x={x}&y={y}&z={z}

```

WMS LIKE
```url

/wms/real-time?data=/path/to/json&style=/path/to/render_option&format=png&width=360&height=1800

```

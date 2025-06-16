# Geello

Geello (Ge**O_V**ello) try to render geo data through [GEO](https://github.com/georust/geo) and [VELLO](https://github.com/linebender/vello)

> [!WARNING]
> Geello is a testing project in an alpha state.
>

### v0.2.0 ~ v0.3.0 Work

- [ ] Finish and Clean up the current code
- [x] Modify RenderedGeometry to split project and render
- [x] Exchange Geom order and Renderer order
- [x] Render Rect should stick to render
- [ ] Multi Layers
- [ ] Calc image size with x/y resolution or WMTS zoom
- [ ] Handle web geojson
- [ ] Make web map more flexible
- [ ] Server: add layers styles memory cache with expire time
- [ ] More Renderer, such as graph renderer and animation renderer
- [ ] Support read render value from props
- [ ] Provider docker package
- [ ] More README and docs

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

#### WMTS LIKE

```rust
// real-time
http://addr:port/wmts/real-time

// cache
http://addr:port/wmts/cache

//params
layers=${/path/to/json}
styles=${/path/to/render_option}
x=${x}
y=${y}
z=${z}
format=${format} // like image/png or png
```

#### WMS LIKE

```rust
http://addr:port/wms

// params
layers=${/path/to/json}
styles=${/path/to/render_option}
format=${format} // like image/png or png
width=${width}
height=${height}
bbox=${bbox}
```

#### What's More -> Animation Or Dynamic Data

Geello use web socket to handle real-time animation or dynamic data.

Such as a point that grows from small to large for representing importance.

Or a real-time route of a car.

```rust
ws://addr:port/ws/anim

// params
layers=${/path/to/json}
styles=${/path/to/render_option}
format=${format} // like image/png or png
width=${width}
height=${height}
bbox=${bbox}
```

### Contributing

Please!

### License

The code is available under the [MIT license](./LICENSE).

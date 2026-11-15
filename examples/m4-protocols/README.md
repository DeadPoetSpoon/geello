# M4 protocol and Unicode label demonstration

This client example uses the implemented application subset, without a server
viewer. Run from the repository root. Start the experimental server:

```text
cargo run --locked --features server-full -- --config examples/server.experimental.toml
```

Upload in the following order using `curl.exe` on Windows (`curl` elsewhere).
After **each** upload, poll `/admin/v1/operations/{operation_id}` from the returned
JSON until `status` is `ready`; stop on `failed`. These create requests require
absent IDs. For replacement, use the resource's current strong ETag in `If-Match`.

```text
curl.exe -X PUT -H "If-None-Match: *" -H "Content-Type: font/ttf" --data-binary @tests/fixtures/fonts/latin.ttf http://127.0.0.1:3000/admin/v1/resources/font/demo:latin/content
curl.exe -X PUT -H "If-None-Match: *" -H "Content-Type: font/otf" --data-binary @tests/fixtures/fonts/cjk.otf http://127.0.0.1:3000/admin/v1/resources/font/demo:cjk/content
curl.exe -X PUT -H "If-None-Match: *" -H "Content-Type: application/geo+json" --data-binary @examples/m4-protocols/data.geojson http://127.0.0.1:3000/admin/v1/resources/dataset/demo:cities/content
curl.exe -X PUT -H "If-None-Match: *" -H "Content-Type: application/toml" --data-binary @examples/m4-protocols/style.toml http://127.0.0.1:3000/admin/v1/resources/style/demo:labels/content
curl.exe -X PUT -H "If-None-Match: *" -H "Content-Type: application/toml" --data-binary @examples/m4-protocols/map.toml http://127.0.0.1:3000/admin/v1/resources/map/demo:labels/content
```

The fonts are explicitly selected renamed OFL subsets. Source URLs, versions,
SHA256 values and licenses are in [the fixture manifest](../../tests/fixtures/fonts/manifest.json).
They are test assets with intentionally limited character coverage.

```text
curl.exe "http://127.0.0.1:3000/maps/demo:labels/image?bbox=110,30,125,45&crs=OGC:CRS84&width=512&height=512" -o labels.png
curl.exe "http://127.0.0.1:3000/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=demo:labels&STYLES=&CRS=EPSG:4326&BBOX=30,110,45,125&WIDTH=512&HEIGHT=512&FORMAT=image/jpeg&BGCOLOR=0xffffff" -o labels.jpg
curl.exe "http://127.0.0.1:3000/wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile&LAYER=demo:labels&STYLE=default&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX=0&TILEROW=0&TILECOL=0&FORMAT=image/png" -o tile.png
curl.exe "http://127.0.0.1:3000/ogc/features/collections/demo:cities/items?bbox=110,30,125,45&limit=1"
curl.exe "http://127.0.0.1:3000/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=demo:cities&RESULTTYPE=hits"
curl.exe "http://127.0.0.1:3000/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=DescribeFeatureType&TYPENAMES=demo:cities"
curl.exe -H "Content-Type: application/xml" --data-binary @examples/m4-protocols/query.xml http://127.0.0.1:3000/wfs
python examples/m4-protocols/websocket-client.py --output frame.png
```

Features `rel=next` is an opaque fixed-snapshot link: follow it unchanged, even
after refresh or unpublish, until its absolute TTL expires. HTTP 410 means restart
the query. String `beijing` has typed token `s6265696a696e67`; numeric IDs use `n`.
Declared scalar queryables use aliases configured in dataset TOML, for example
`queryables=[{name='city',field='/name',type='string'}]`; unknown names fail.

WebSocket sends hello, then accepts init/update/retry. A complete playback group
can seek or pause: `{"type":"update","request_id":"seek","playback":{"time":1,"speed":1,"playing":false}}`.
Controls receive ordered acknowledgements; obsolete unstarted frames may be
discarded. Read exactly one binary image after each frame metadata message.
JPEG output requires a complete output group with explicit quality:
`{"format":"jpeg","jpeg_quality":90,"fps":30}`. Refresh preserves time and
emits `resource-changed`; unpublish emits `resource-unpublished` and closes after
any started pair. Reconnect with a complete init. Controls and sends are bounded
by the published hello/profile; a slow client eventually disconnects.

`python tools/verify_m4_demo.py` runs the uploads, image/query requests and real
WebSocket capture in a temporary CPU server, using only Python standard library.
The independent geometry/time example is [here](../geometry-animation/README.md).

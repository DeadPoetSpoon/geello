# Independent web map preview

Start Geello from the repository root:

```text
cargo run --release --locked --features server-full -- --config examples/server.experimental.toml
```

In another terminal, start the preview (Python standard library only):

```text
python tools/preview_server.py
```

Open <http://127.0.0.1:8080>. The preview discovers published maps through the
public OGC Maps collections API, including paginated results. OpenLayers 10.7.0
is bundled locally under `vendor/`; JavaScript and CSS load from the preview
server without a CDN. Select a map,
drag to pan, scroll to zoom, or use the zoom buttons and location presets.
The focused map also supports arrow keys and +/-. Switch between OGC Maps
WMTS 1.0.0 and WMS 1.3.0, choose a published CRS, set a geographic extent, or request a
specific animation time. The current request can be opened or copied.

The viewer supports EPSG:3857 and OGC:CRS84. WMTS uses OpenLayers' native WMTS
source with `WebMercatorQuad` or `WorldCRS84Quad`, respects published zoom limits,
and does not wrap across the matrix boundary. WMS uses the CRS:84 alias with
longitude/latitude axes. `views.json` supplies the initial geographic extent
for local examples, including `preview:jd`; other maps initially cover China.
The reset button returns to the selected example's extent. A `?map=preview%3Ajd`
query selects a published map directly. Transparent maps
show the viewer background; no external basemap is required.

Use Refresh after publishing, replacing or unpublishing maps. An empty catalog,
unavailable service, unsupported CRS and render errors have explicit messages.
This page does not upload or publish resources. To populate an empty server,
follow [the protocol demo](../m4-protocols/README.md).

For downloadable public-domain vector datasets, shaded relief and a real Landsat
GeoTIFF, see [the open-data demo](../open-data/README.md). Run
`python -B tools/publish_open_data_demo.py --require-gpu --preview http://127.0.0.1:8080`
against a running GPU server with the sample's resource budgets. Sources and
checksums are recorded; downloads stay under ignored `assets/test/open-data/`.
The preview's default `open:overlay` combines this relief with countries, rivers
and city labels; existing map-specific query URLs remain available.

To publish the owner-provided `assets/test/jd.tif` imagery, start Geello using
the experimental configuration above, then run:

```text
python tools/publish_imagery_demo.py
```

The script streams the original TIFF (about 1.1 GB, 16384×12544 pixels) and
creates or replaces the `preview:jd` dataset, RGB style and `jd.tif 影像` map.
It preserves the source alpha channel and explicitly uses EPSG:3857 for its
ESRI Web Mercator coordinates. The source stays untouched; no downsampled
proxy is uploaded. Resources are replaced using their current ETags.

The experimental profile allows uploads and sources up to 20 GiB
(21,474,836,480 bytes), with 20 GiB total managed storage, 40 GiB derived
storage and a 30 minute operation deadline. The profile selects the GPU
backend explicitly, accounts roughly 60% of the 64 GiB host memory, keeps an
8 GiB decoded-block cache, uses 2048-pixel native blocks and admits eight HTTP
plus four background workers. The nearest-neighbor jd style and larger blocks
reduce repeated strip decoding during this large TIFF's preparation. These are
local example capacities, not production defaults;
the development and small profiles retain their own settings. ZIP expansion
limits are separately configured.

Open <http://127.0.0.1:8080/?map=preview%3Ajd>. The image is automatically
framed at its geographic extent. Switch between OGC Maps, WMS and WMTS;
this imagery map publishes its native EPSG:3857 and `WebMercatorQuad` through
zoom 22. This allows overview selection during zoomed-out rendering. The
server currently uses base resolution for reprojection requests, so CRS84 is
not advertised for this large imagery example. Other maps can advertise both
matrix sets as before.
The server builds its derived overviews in the background on first use.
The level is selected from the requested bounds and image dimensions. The
coarsest compatible level no coarser than the target pixel footprint is read
only for the intersecting window; zooming in returns to finer levels. Nearest
styles use nearest pyramids and area-average styles use weighted pyramids.
An incompatible pyramid is rebuilt privately, while the existing source stays
available. For now, this optimization applies when source and view CRS match;
reprojection conservatively retains base resolution. The service uses its
checksummed derived pyramid, rather than reading arbitrary TIFF overview IFDs
on every request. Source TIFF overviews are not imported directly yet.
`jd.tif` is an ignored local asset and must be supplied separately on a new
checkout. The older `imagery/elevation.tif` remains a synthetic sample asset.

For a different server or preview port:

```text
python tools/preview_server.py --upstream http://127.0.0.1:3000 --port 8080
```

The upstream may include a deployment path prefix. The preview server binds
to loopback and proxies only read-only public discovery, image and protocol
requests to that fixed upstream. It keeps the page and requests on one origin
without changing Geello's CORS policy. The Geello binary retains its existing
API routes and does not host this independent example. Upstream JSON links
are preserved; the viewer maps discovery links through its proxy.
The proxy's default response timeout is 1800 seconds to accommodate the
first render of large imagery; override it with `--timeout SECONDS`.
The proxy forwards at most two upstream requests at a time and queues the rest,
so an eight-tile browser burst does not exhaust the development profile's five
HTTP admission slots. This limit is shared across preview clients, including
requests still running upstream after a browser cancels them. Override it with
`--upstream-concurrency COUNT` only when the upstream profile has enough capacity;
keep headroom for discovery and other clients. Queue waits use the same timeout.

Label candidates are selected and collision-tested before viewport culling, so
adjacent tiles retain the same placement even when a preferred label lies outside
one tile's padded view. Only the selected label is clipped; an offscreen label
does not trigger an alternative placement that would duplicate it across a seam.

## Imagery and Shapefile outlines, with WFS

After publishing `preview:jd`, run:

```text
python tools/publish_annotation_demo.py
```

This packages the owner-provided `assets/test/230800_1.shp` with its SHX, DBF,
PRJ and UTF-8 CPG, and publishes `preview:annotations`. The new map
`preview:jd-annotations` places this vector layer above the original jd imagery.
Its `LXMC` field supplies four classes (water, buildings, woodland and road).
All polygon rings are stroked, with no polygon fill. Existing jd maps stay
available.

Open <http://127.0.0.1:8080/?map=preview%3Ajd-annotations> for server-rendered
Maps, WMS or WMTS, or
<http://127.0.0.1:8080/?map=preview%3Ajd-annotations&protocol=wfs> for WFS.
The latter requests WFS 2.0 GetFeature GeoJSON, with a CRS84 bbox and bounded
COUNT/STARTINDEX pagination, and draws the resulting outlines in OpenLayers
above an imagery-only map request. Pan/zoom refreshes the current viewport.
Filter by LXMC or click an outline to inspect its original attributes. WFS
returns feature data; the imagery background continues to use the map service.
The preview proxy permits read-only `/wfs` and Features pagination requests.

On the local workstation, a 1080×698 jd view took 30.89 seconds with nearest
sampling forced to the base. After the nearest pyramid was ready, the same
extent selected factor 16 (1024×784) and new render requests took 2.77, 1.84
and 1.92 seconds. Animation time values differed to bypass completed-image
caches; decoded blocks and the operating-system cache could still be reused.
This is a local workload measurement, not a cold-disk throughput benchmark.

WMTS metatile prewarming, cache counters and measured GPU timings are recorded
in [the prewarming validation](wmts-prewarm-validation.md).

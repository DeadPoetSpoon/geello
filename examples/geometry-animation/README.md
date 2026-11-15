# Geometry animation demonstration

Run the experimental server as described in the
[service guide](../../docs/refactor/09-m3-service-and-evidence.md).
Run these commands from the repository root; on Windows use `curl.exe`.
The commands create absent resources. After **each** upload, poll
`GET /admin/v1/operations/{operation_id}` using the returned `operation_id`
until `status` is `ready` before issuing the next command.
Replace existing resources with their current strong `If-Match` ETag instead
of `If-None-Match: *`.

```sh
curl -X PUT -H "If-None-Match: *" -H "Content-Type: application/geo+json" --data-binary @examples/geometry-animation/points.geojson http://127.0.0.1:3000/admin/v1/resources/dataset/demo:motion-points/content
curl -X PUT -H "If-None-Match: *" -H "Content-Type: application/toml" --data-binary @examples/geometry-animation/style.toml http://127.0.0.1:3000/admin/v1/resources/style/demo:motion-style/content
curl -X PUT -H "If-None-Match: *" -H "Content-Type: application/toml" --data-binary @examples/geometry-animation/animation.toml http://127.0.0.1:3000/admin/v1/resources/animation/demo:motion/content
curl -X PUT -H "If-None-Match: *" -H "Content-Type: application/toml" --data-binary @examples/geometry-animation/map.toml http://127.0.0.1:3000/admin/v1/resources/map/demo:motion-map/content
curl --output motion-start.png "http://127.0.0.1:3000/maps/demo:motion-map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=256&height=256&geello_time=0"
curl --output motion-end.png "http://127.0.0.1:3000/maps/demo:motion-map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=256&height=256&geello_time=2"
curl --output motion-tile.png "http://127.0.0.1:3000/tiles/demo:motion-map/0/0/0?geello_time=2"
```

The source point at longitude 10 starts outside the requested bbox. At two
seconds its trajectory anchor reaches longitude zero, then translation moves
it to latitude 0.25. Rotation and scaling keep the explicit point anchor fixed;
they also work on whole line/polygon/collection geometry. Point-symbol diameter
is a separate style parameter and stays 12px. The source GeoJSON remains intact.
Request the times in any order: the same resources/time/view reproduce the
same bytes. `geello_time` is finite nonnegative seconds; omitted time is zero.

For explicit timestamp timing, replace `timing = "distance"` with
`timing = "timestamps", times = [0.0, 2.0]` in the trajectory inline table.
Distance timing uses the source coordinate plane, including degrees for CRS84.
All anchors and translation vectors use the same source units; rotation uses
raw degrees and scale uses nonnegative dimensionless XY factors.

See the [implemented geometry contract](../../docs/refactor/10-m4-animation-and-evidence.md#geometry-animation-increment)
for typed IDs, refresh, admission and diagnostic boundaries.

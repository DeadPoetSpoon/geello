"""Generate embedded OpenAPI 3.0 and offline HTML for implemented HTTP routes.

Run with Python 3: python tools/generate_http_docs.py [--check]. No dependencies.
Protocol implementations must extend this contract and its HTTP regressions.
"""
import argparse
import html
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "src/server/http"


def ref(name):
    return {"$ref": f"#/components/schemas/{name}"}


def obj(properties, required=None, **extra):
    return {"type": "object", "properties": properties,
            "required": list(properties) if required is None else required,
            "additionalProperties": False, **extra}


def array(items, **extra):
    return {"type": "array", "items": items, **extra}


STRING = {"type": "string"}
INTEGER = {"type": "integer", "minimum": 0}
NULL_STRING = {"type": "string", "nullable": True}
RESOURCE_ID = {"type": "string", "maxLength": 256,
               "pattern": r"^[A-Za-z_][A-Za-z0-9_.-]*:[A-Za-z_][A-Za-z0-9_.-]*$"}
KINDS = {"type": "string", "enum": ["dataset", "style", "map", "animation", "font"]}


def param(name, where, schema, description, required=False):
    return {"name": name, "in": where, "required": required,
            "description": description, "schema": schema}


def schema_response(description, schema, media="application/json", **extra):
    return {"description": description,
            "content": {media: {"schema": schema}}, **extra}


def error_response(description):
    return schema_response(description, ref("Problem"), "application/problem+json")


def operation(operation_id, summary, parameters, success, description="", write=False, **extra):
    responses = dict(success)
    responses.update({
        "400": error_response("Invalid path, query or request; unknown and duplicate query fields are rejected."),
        "404": error_response("Resource, operation or route not found; unpublished resources cannot render."),
        "405": {**error_response("Method not allowed. The Allow header lists supported methods."),
                "headers": {"Allow": {"schema": STRING, "description": "Methods accepted by the matched route."}}},
        "422": error_response("Resource or render validation failed."),
        "500": error_response("Storage operation failed; internal filesystem details are withheld."),
        "503": error_response("Admission exhausted, preparing/preparation failed, shutdown, deadline or backend failure. Retry is caller controlled."),
    })
    if write:
        responses.update({
            "403": error_response("Cross-origin browser administration writes are forbidden. No-Origin CLI requests remain supported."),
            "408": error_response("Upload or registration body timed out."),
            "409": error_response("Stale precondition, referenced resource or ownership conflict; authority is unchanged."),
            "413": error_response("Request or upload exceeds the configured profile."),
            "415": error_response("Unsupported upload media type."),
            "428": error_response("A create or mutation precondition is required."),
        })
    correlation = "Single nonempty visible ASCII value, at most 128 bytes, excluding quotes and commas. Invalid, missing or duplicate values are replaced by a generated request ID. Returned on every application response, including errors and 304; it does not affect image/cache identity."
    parameters = [p for p in parameters if (p["in"], p["name"]) != ("header", "X-Request-Id")] + [param("X-Request-Id", "header", {"type": "string", "minLength": 1, "maxLength": 128}, correlation)]
    # Copy response objects: callers reuse image/schema responses across routes.
    responses = {status: {**response, "headers": {
        **response.get("headers", {}),
        "X-Request-Id": {"schema": STRING, "description": correlation},
    }} for status, response in responses.items()}
    return {"operationId": operation_id, "summary": summary, "description": description,
            "parameters": parameters, "responses": responses, **extra}


def build():
    schemas = {
        "ResourceKey": obj({"kind": KINDS, "id": RESOURCE_ID}),
        "SourceSpec": obj({
            "path": {"type": "string", "description": "Explicit registered local path inside an allowed root; never a client URL."},
            "format": {"type": "string", "enum": ["geojson", "shapefile", "geotiff", "toml", "font"]},
            "encoding": NULL_STRING, "crs": NULL_STRING,
            "sample_kind": {"type": "string", "enum": ["continuous", "categorical", None], "nullable": True},
        }, ["path", "format"]),
        "Crs": obj({"definition": STRING, "space": {"type": "string", "enum": ["Planar", "GeographicXy"]}}),
        "ResourceRecord": obj({
            "key": ref("ResourceKey"), "ownership": {"type": "string", "enum": ["managed", "external"]},
            "version": STRING, "published": {"type": "boolean"}, "source": ref("SourceSpec"),
            "dependencies": array(ref("ResourceKey")), "data_version": STRING,
            "derived_version": NULL_STRING, "derived_path": NULL_STRING,
            "managed_directory": NULL_STRING, "class": NULL_STRING,
            "effective_crs": ref("Crs"), "definition": NULL_STRING,
            "type_bindings": array(array(ref("ResourceKey"), minItems=2, maxItems=2)),
        }, ["key", "ownership", "version", "published", "source", "dependencies", "data_version",
            "derived_version", "derived_path", "managed_directory", "class", "definition", "type_bindings"]),
        "Operation": obj({
            "operation_id": STRING, "resource": ref("ResourceKey"),
            "status": {"type": "string", "enum": ["staging", "preparing", "ready", "failed", "unpublished"]},
            "version": NULL_STRING, "diagnostics": array(STRING),
        }),
        "Problem": obj({
            "type": {"type": "string", "format": "uri", "example": "urn:geello:problem:invalid-request"},
            "title": STRING, "status": {"type": "integer", "minimum": 400, "maximum": 599},
            "references": array(ref("ResourceKey")),
        }, ["type", "title", "status"]),
        "CatalogStats": obj({"generation": INTEGER, "resources": INTEGER,
                             "pinned_versions": INTEGER, "detached_content": INTEGER}),
        "Ready": obj({"status": {"type": "string", "enum": ["ready"]}, "backend": STRING,
                      "backend_diagnostic": NULL_STRING, "catalog": ref("CatalogStats"),
                      "live_accounted_bytes": INTEGER, "cache_failures": INTEGER,
                      "response_cache": obj({
                          "memory_capacity_bytes": INTEGER, "disk_capacity_bytes": INTEGER,
                          "memory_used_bytes": INTEGER, "hits": INTEGER,
                          "area_renders": INTEGER, "tile_encodes": INTEGER,
                          "prewarm_completed": INTEGER, "prewarm_groups_active": INTEGER,
                      })}),
        "ResourcePage": obj({"generation": INTEGER, "total": INTEGER, "resources": array(ref("ResourceRecord"))}),
        "OperationPage": obj({"total": INTEGER, "operations": array(ref("Operation"))}),
        "ConfigurationParse": obj({"text": STRING}),
        "ConfigurationSerialize": obj({"document": {"type": "object", "additionalProperties": True}}),
        "ConfigurationResult": obj({"text": STRING, "document": {"type": "object", "additionalProperties": True}}),
    }
    key_params = [param("kind", "path", KINDS, "Resource kind.", True),
                  param("id", "path", RESOURCE_ID, "Case-sensitive namespace:local resource identity.", True)]
    preconditions = [
        param("If-Match", "header", STRING, "Exactly one strong current resource ETag, including quotes, to replace, unpublish or delete. No wildcard, weak or list form."),
        param("If-None-Match", "header", {"type": "string", "enum": ["*"]}, "Creation only. Supply exactly one of the two precondition headers; missing/both return 428."),
    ]
    etag_header = {"ETag": {"description": "Strong current resource version, quoted.", "schema": STRING}}
    time = param("geello_time", "query", {"type": "number", "minimum": 0, "default": 0},
                 "Finite nonnegative seconds. Case-sensitive. Omitted, 0 and -0 share zero cache identity. Independent of data version.")
    pixel_ratio = param("geello_pixel_ratio", "query", {"type": "number", "minimum": 0, "exclusiveMinimum": True, "maximum": 4, "default": 1},
                        "Physical output pixels per CSS pixel. Scales pixel-space symbols, strokes, labels and spacing; map-unit lengths are unchanged. Width/height are physical pixels. Partitions render caches; does not change resource revision.")
    revalidate = param("If-None-Match", "header", STRING,
                       "Encoded-byte ETag(s) or *. Current publication is checked before cached 200/304; weak tags currently do not match.")
    image_headers = {
        "ETag": {"description": "Strong hash of actual encoded bytes.", "schema": STRING},
        "Cache-Control": {"schema": {"type": "string", "enum": ["no-cache"]}},
        "X-Geello-Backend": {"schema": {"type": "string", "enum": ["cpu", "gpu"]}},
        "X-Geello-Render-Revision": {"schema": STRING, "description": "Pinned resource graph identity; time alone does not change it."},
        "X-Geello-Incomplete": {"schema": {"type": "string", "enum": ["true", "false"]}, "description": "Bounded preparation diagnostics report skipped incompatible components or missing animation IDs. Preserved in caches."},
    }
    images = {"200": schema_response("Rendered PNG; no content is a successful transparent image.",
                                      {"type": "string", "format": "binary"}, "image/png", headers=image_headers),
              "304": {"description": "Current published encoded bytes match If-None-Match; no response body.", "headers": image_headers}}
    resource_success = {"200": schema_response("Current resource record.", ref("ResourceRecord"), headers=etag_header)}
    accepted = {"202": schema_response("Upload/registration accepted; poll /admin/v1/operations/{operation_id} until ready or failed. 202 does not imply successful publication.", ref("Operation"))}
    paths = {
        "/admin": {"get": operation("redirectAdmin", "Redirect to the integrated administration page", [], {"307": {"description": "Relative ./admin/ redirect preserves reverse-proxy prefixes."}})},
        "/admin/": {"get": operation("getAdmin", "Open the embedded administration interface", [], {"200": schema_response("Offline Chinese administration UI. Protect both /admin/ and /admin/v1 using proxy authentication.", STRING, "text/html"), "304": {"description": "HTML ETag matched."}})},
        "/admin/assets/{file}": {"get": operation("getAdminAsset", "Get a whitelisted embedded administration asset", [param("file", "path", STRING, "Whitelisted JS, CSS or pinned OpenLayers assets; nested vendor paths supported. No arbitrary files.", True)], {"200": schema_response("Static asset with MIME, strong ETag and browser security headers.", STRING, "application/octet-stream"), "304": {"description": "Asset ETag matched."}})},
        "/api": {"get": operation("getOpenApi", "Get this OpenAPI 3.0 contract", [],
                                  {"200": schema_response("OpenAPI 3.0.3 document. servers uses only configured public_base_url; x-geello-enabled describes this binary/profile.", {"type": "object", "additionalProperties": True})})},
        "/api.html": {"get": operation("getApiHtml", "Read offline static API documentation", [],
                                       {"200": schema_response("Standalone English HTML; no JavaScript, CDN or server-hosted map viewer.", STRING, "text/html")})},
        "/health/ready": {"get": operation("getReadiness", "Inspect initialized server readiness", [],
                                           {"200": schema_response("Initialized backend and bounded service counters.", ref("Ready"))})},
        "/health/live": {"get": operation("getLiveness", "Inspect process liveness", [],
                                           {"200": schema_response("Process is accepting health checks.", obj({"status": {"type": "string", "enum": ["alive"]}}))})},
        "/admin/v1/resources": {"get": operation("listResources", "List registered resources", [
            param("offset", "query", {**INTEGER, "default": 0}, "Administrative offset in the current ordered catalog; not a fixed-snapshot feature cursor."),
            param("limit", "query", {"type": "integer", "minimum": 1}, "Defaults to profile.max_list_items; cannot exceed that limit."),
            param("kind", "query", KINDS, "Optional resource kind filter."),
            param("q", "query", {"type": "string", "maxLength": 256}, "ASCII case-insensitive ID substring, at most 256 UTF-8 bytes."),
            param("published", "query", {"type": "boolean"}, "Optional exact publication-state filter."),
        ], {"200": schema_response("Page and the exact catalog generation used for this page.", ref("ResourcePage"))})},
        "/admin/v1/resources/{kind}/{id}": {
            "get": operation("getResource", "Read a registered resource", key_params, resource_success),
            "delete": operation("removeResource", "Remove the resource record and retain original content", key_params + preconditions,
                                {"204": {"description": "Record removed; managed/external original content retained."}}, write=True),
        },
        "/admin/v1/resources/{kind}/{id}:unpublish": {
            "post": operation("unpublishResource", "Unpublish a resource atomically", key_params + preconditions, resource_success,
                              "Current If-Match required. Stop new use; existing pins may finish. Referenced resources return 409. No cascading unpublish.", write=True),
        },
        "/admin/v1/resources/{kind}/{id}:publish": {
            "post": operation("publishRegisteredResource", "Prepare and publish the registered source", key_params + preconditions, accepted,
                              "Requires If-Match and an empty body. Revalidates external roots and preserves managed/external ownership. Can refresh an already-published resource. Preparation failure leaves old publication and source intact. No automatic retry.", write=True),
        },
        "/admin/v1/operations": {"get": operation("listOperations", "List resource operations in reverse operation ID order", [
            param("offset", "query", INTEGER, "Page offset."),
            param("limit", "query", {"type": "integer", "minimum": 1}, "At most profile.max_list_items."),
            param("status", "query", {"type": "string", "enum": ["staging", "preparing", "ready", "failed", "unpublished"]}, "Exact status filter."),
            param("kind", "query", KINDS, "Exact resource kind filter."),
            param("resource_id", "query", STRING, "Exact resource ID filter."),
        ], {"200": schema_response("Filtered total and bounded operation page; no fabricated timestamps or progress.", ref("OperationPage"))})},
        "/admin/v1/resources/{kind}/{id}/source": {"put": operation(
            "registerExternalSource", "Create or replace an external source registration", key_params + preconditions, accepted,
            "Deployment proxy protects administration. Registered regular files must be inside configured external roots. Original files remain externally owned.", write=True,
            requestBody={"required": True, "content": {"application/json": {"schema": ref("SourceSpec")}}})},
        "/admin/v1/resources/{kind}/{id}/content": {
            "put": operation("uploadResource", "Create or replace managed resource content", key_params + preconditions + [
                param("X-Geello-CRS", "header", STRING, "Explicit source CRS interpretation; overrides format metadata."),
                param("X-Geello-Encoding", "header", STRING, "Explicit Shapefile/DBF encoding."),
                param("X-Geello-Sample-Kind", "header", {"type": "string", "enum": ["continuous", "categorical"]}, "Required interpretation for GeoTIFF raster sampling."),
            ], accepted, "Raw bounded body; not multipart. Dataset: GeoJSON/TIFF/Shapefile ZIP/TOML; style/map/animation: TOML; font: explicit bytes. Reader support depends on the compiled binary (x-geello-enabled). Font upload validates container signature only; full typography validation is pending. Preparation failures retain the previous publication.", write=True,
                requestBody={"required": True, "content": {media: {"schema": {"type": "string", "format": "binary"}} for media in [
                    "application/geo+json", "application/geojson", "image/tiff", "image/geotiff", "application/zip", "application/toml", "text/toml", "font/ttf", "font/otf", "font/collection", "application/octet-stream"]}}),
            "delete": operation("deleteManagedContent", "Delete managed content and its resource record", key_params + preconditions,
                                {"204": {"description": "Managed content and record deleted using the recovery journal."}},
                                "Current If-Match required. External ownership or dependencies/in-flight pins return 409. No cascade.", write=True),
        },
        "/admin/v1/operations/{id}": {"get": operation("getOperation", "Poll a resource operation", [param("id", "path", STRING, "Operation ID returned by an accepted write.", True)],
                                                                     {"200": schema_response("Current persisted operation state. Retention may expire terminal operations.", ref("Operation"))})},
        "/maps/{id}/image": {"get": operation("renderMap", "Render a published map at explicit time", [
            param("id", "path", RESOURCE_ID, "Published map identity.", True),
            param("bbox", "query", STRING, "Exactly four finite comma-separated XY values: min-x,min-y,max-x,max-y; strictly positive spans. CRS84/EPSG:4326 use longitude,latitude here.", True),
            param("crs", "query", STRING, "Explicit output CRS allowed by this map; pure Rust projection. Basic route uses XY axes.", True),
            param("width", "query", {"type": "integer", "minimum": 1}, "Pixel width; bounded by profile.max_dimension and max_image_bytes.", True),
            param("height", "query", {"type": "integer", "minimum": 1}, "Pixel height; bounded by profile.max_dimension and max_image_bytes.", True), time, pixel_ratio, revalidate,
        ], images, "PNG, stretch aspect policy. Immutable pinned data/styles/animations. Source-plane geometry animation precedes projection; originals remain unchanged. This is the basic application route; no WMS/OGC conformance claim.")},
        "/tiles/{id}/{z}/{x}/{y}": {"get": operation("renderTile", "Render a WebMercatorQuad tile at explicit time", [
            param("id", "path", RESOURCE_ID, "Published map identity allowing EPSG:3857.", True),
            param("z", "path", {"type": "integer", "minimum": 0, "maximum": 30}, "Zoom level.", True),
            param("x", "path", INTEGER, "Column; must be less than 2^z. No wrapping.", True),
            param("y", "path", INTEGER, "Row downward from top-left; must be less than 2^z.", True), time, revalidate,
        ], images, "256px PNG in EPSG:3857; configured aligned metatile/buffer, deduplicated area render and encoding only requested tiles. This basic route does not implement WMTS or OGC Tiles discovery.")},
    }
    for action, schema in [("parse", "ConfigurationParse"), ("serialize", "ConfigurationSerialize")]:
        paths[f"/admin/v1/configurations/{{kind}}/{action}"] = {"post": operation(
            f"{action}Configuration", f"{action.title()} a TOML configuration without publication",
            [param("kind", "path", {"type": "string", "enum": ["style", "map", "animation"]}, "Configuration model.", True)],
            {"200": schema_response("Validated full tree and TOML text. Unsafe browser integers, datetimes and nonfinite values return 422; keep original TOML in that case.", ref("ConfigurationResult"))},
            "Bounded by max_registry_bytes and normal memory/work admission. Does not write a catalog or start preparation. Forms may reorder comments/formatting but must preserve the full semantic tree.", write=True,
            requestBody={"required": True, "content": {"application/json": {"schema": ref(schema)}}})}
    schemas.update({
        "Link": obj({"href": STRING, "rel": STRING, "type": STRING, "templated": {"type": "boolean"}}, ["href", "rel"]),
        "OgcLanding": obj({"title": STRING, "description": STRING, "links": array(ref("Link"))}),
        "OgcConformance": obj({"conformsTo": array(STRING, maxItems=0),
            "x-geello-tested-subset": {"type": "string", "enum": ["maps", "tiles"]},
            "x-geello-status": {"type": "string", "enum": ["tested subset; not an official conformance certification"]}}),
        "MapCollection": obj({"id": RESOURCE_ID, "title": STRING, "itemType": {"type": "string", "enum": ["map"]},
            "crs": array(STRING), "links": array(ref("Link")), "x-geello-tile-matrix-sets": array(STRING),
            "x-geello-min-zoom": INTEGER, "x-geello-max-zoom": INTEGER}),
        "MapCollections": obj({"collections": array(ref("MapCollection")), "links": array(ref("Link")), "x-geello-generation": INTEGER}),
        "TileMatrixSetSummary": obj({"id": STRING, "crs": STRING, "links": array(ref("Link"))}),
        "TileMatrixSets": obj({"tileMatrixSets": array(ref("TileMatrixSetSummary"))}),
        "TileMatrix": obj({"id": STRING, "scaleDenominator": {"type": "number"}, "cellSize": {"type": "number"},
            "pointOfOrigin": array({"type": "number"}, minItems=2, maxItems=2),
            "cornerOfOrigin": {"type": "string", "enum": ["topLeft"]},
            "tileWidth": {"type": "integer", "enum": [256]}, "tileHeight": {"type": "integer", "enum": [256]},
            "matrixWidth": INTEGER, "matrixHeight": INTEGER}),
        "TileMatrixSet": obj({"id": STRING, "crs": STRING, "orderedAxes": array(STRING, minItems=2, maxItems=2),
            "tileMatrices": array(ref("TileMatrix"), minItems=31, maxItems=31)}),
        "TileLimits": obj({"tileMatrix": STRING, "minTileRow": INTEGER, "maxTileRow": INTEGER,
            "minTileCol": INTEGER, "maxTileCol": INTEGER}),
        "MapTileset": obj({"dataType": {"type": "string", "enum": ["map"]}, "crs": STRING,
            "tileMatrixSetId": STRING, "tileMatrixSetURI": STRING,
            "tileMatrixSetLimits": array(ref("TileLimits")), "links": array(ref("Link"))}),
        "MapTilesets": obj({"tilesets": array(ref("MapTileset"))}),
    })
    map_id = [param("id", "path", RESOURCE_ID, "Published map identity; datasets are not public collections here.", True)]
    grid = param("grid", "path", {"type": "string", "enum": ["WebMercatorQuad", "WorldCRS84Quad"]},
                 "Case-sensitive tile matrix set published by this map.", True)
    subset = "Experimental tested OGC Maps/Tiles subset. No complete OGC conformance class is claimed; conformsTo is empty. Unknown/duplicate query fields return 400."
    for kind in ["maps", "tiles"]:
        root = f"/ogc/{kind}"
        prefix = f"ogc{kind.title()}"
        def read(suffix, name, summary, schema, parameters=None, description=subset):
            paths[root + suffix] = {"get": operation(prefix + name, summary, parameters or [],
                {"200": schema_response(summary, ref(schema))}, description)}
        read("", "Landing", f"Discover the {kind} subset", "OgcLanding")
        read("/", "LandingSlash", f"Discover the {kind} subset (trailing slash)", "OgcLanding")
        read("/conformance", "Conformance", "Inspect tested subset status without claiming a complete class", "OgcConformance")
        paths[root + "/api"] = {"get": operation(prefix + "OpenApi", "Get the shared OpenAPI document", [], paths["/api"]["get"]["responses"], subset)}
        paths[root + "/api.html"] = {"get": operation(prefix + "ApiHtml", "Read the shared offline guide", [], paths["/api.html"]["get"]["responses"], subset)}
        read("/collections", "Collections", "List published map collections", "MapCollections", [
            param("offset", "query", {**INTEGER, "default": 0}, "Offset in currently published maps, ordered by resource ID. No retained feature snapshot cursor."),
            param("limit", "query", {"type": "integer", "minimum": 1}, "Defaults to profile.max_list_items; cannot exceed that ceiling. Follow links with rel=next."),
        ])
        read("/collections/{id}", "Collection", "Read a published map and its configured CRS/grid metadata", "MapCollection", map_id)
        read("/tileMatrixSets", "MatrixSets", "List the two fixed tile matrix sets", "TileMatrixSets")
        read("/tileMatrixSets/{id}", "MatrixSet", "Inspect matrix geometry for zooms 0 through 30", "TileMatrixSet", [
            param("id", "path", {"type": "string", "enum": ["WebMercatorQuad", "WorldCRS84Quad"]}, "Tile matrix set identity.", True)])
        read("/collections/{id}/map/tiles", "Tilesets", "List the map's published tilesets", "MapTilesets", map_id)
        read("/collections/{id}/map/tiles/{grid}", "Tileset", "Inspect published zoom and row/column limits", "MapTileset", map_id + [grid])
        paths[root + "/collections/{id}/map/tiles/{grid}/{z}/{row}/{col}"] = {"get": operation(
            prefix + "Tile", "Render a tile from a published fixed matrix", map_id + [grid,
                param("z", "path", {"type": "integer", "minimum": 0, "maximum": 30}, "Zoom within this map's published range (default 0-22).", True),
                param("row", "path", INTEGER, "Downward from the top-left; less than 2^z.", True),
                param("col", "path", INTEGER, "Less than 2^z for WebMercatorQuad or 2^(z+1) for WorldCRS84Quad. No wrapping.", True), time, revalidate],
            images, subset + " 256px PNG; z0 matrices are 1x1 and 2x1 respectively. Metatile buffers are clipped at matrix edges. Unpublished grid/zoom returns 404; coordinates outside the matrix return 400. Shares pinned rendering, ETags, cache and incomplete diagnostics with basic routes.")}
        if kind == "maps":
            paths[root + "/collections/{id}/map"] = {"get": operation(prefix + "Image", "Render a published map image", paths["/maps/{id}/image"]["get"]["parameters"], images,
                subset + " Explicit bbox, crs, width and height are required; XY axes and stretch policy. geello_time is independent of data time.")}
    protocol_subset = "Strict KVP query subset with published capabilities. Parameter names are case-insensitive and duplicates are rejected after canonicalization. WMS composes maps in request order on one canvas with global labels; WMTS uses fixed published grids. Protocol failures use sanitized WMS ServiceExceptionReport or OWS ExceptionReport XML; outer admission failures use problem+json. No official conformance certification is claimed."
    paths["/wms"] = {"get": operation("wmsKvp", "Serve the tested WMS 1.3.0 KVP subset", [
        param("SERVICE", "query", {"type": "string", "enum": ["WMS"]}, "Must be WMS.", True),
        param("VERSION", "query", {"type": "string", "enum": ["1.3.0"]}, "Supported WMS version.", True),
        param("REQUEST", "query", {"type": "string", "enum": ["GetCapabilities", "GetMap"]}, "Capabilities or published map composition.", True),
        param("LAYERS", "query", STRING, "Comma-separated published map IDs in drawing order."), param("STYLES", "query", STRING, "Empty style entries only."),
        param("CRS", "query", STRING, "Published CRS; CRS:84 is XY and EPSG:4326 uses latitude,longitude bbox order."), param("BBOX", "query", STRING, "Four ordered coordinates in protocol axis order."),
        param("WIDTH", "query", {"type": "integer", "minimum": 1}, "Output width."), param("HEIGHT", "query", {"type": "integer", "minimum": 1}, "Output height."),
        param("FORMAT", "query", {"type": "string", "enum": ["image/png", "image/jpeg"]}, "Final image format; JPEG quality 90."),
        param("TRANSPARENT", "query", {"type":"boolean"}, "Default false; PNG may retain transparency."),
        param("BGCOLOR", "query", STRING, "0xRRGGBB, default white; JPEG flattens once against this color."),
        param("TIME", "query", {"type":"number", "minimum":0}, "Animation seconds alias; cannot accompany geello_time."), time, pixel_ratio],
        {"200": {"description":"XML capabilities or encoded map image.","content": {"text/xml":{"schema":STRING},"image/png":{"schema":{"type":"string","format":"binary"}},"image/jpeg":{"schema":{"type":"string","format":"binary"}}}}}, protocol_subset)}
    paths["/wmts"] = {"get": operation("wmtsKvp", "Serve the tested WMTS 1.0.0 KVP subset", [
        param("SERVICE", "query", {"type": "string", "enum": ["WMTS"]}, "Must be WMTS.", True), param("VERSION", "query", {"type": "string", "enum": ["1.0.0"]}, "Supported WMTS version.", True),
        param("REQUEST", "query", {"type": "string", "enum": ["GetCapabilities", "GetTile"]}, "Capabilities or one tile.", True), param("LAYER", "query", RESOURCE_ID, "Published map ID for GetTile."),
        param("STYLE", "query", {"type": "string", "enum": ["default"]}, "Default style only."), param("TILEMATRIXSET", "query", {"type": "string", "enum": ["WebMercatorQuad", "WorldCRS84Quad"]}, "Fixed published grid."),
        param("TILEMATRIX", "query", INTEGER, "Zoom."), param("TILEROW", "query", INTEGER, "Downward row."), param("TILECOL", "query", INTEGER, "Column without wrapping."),
        param("FORMAT", "query", {"type": "string", "enum": ["image/png"]}, "PNG only."), time],
        {"200": schema_response("XML capabilities or rendered PNG depending on REQUEST.", {"oneOf": [{"type": "string", "format": "binary"}, {"type": "string"}]}, "text/xml")}, protocol_subset)}
    feature_subset = "Published vector datasets as CRS84 XY GeoJSON. Exact geometry/bbox intersection returns complete original geometries; null geometry matches bbox for Features, empty geometry does not. Declared queryable aliases accept typed scalar equalities combined with AND; typed IDs combine with OR. numberMatched is exact within operation budgets. Data is timeless: valid RFC3339 datetime selections match all records. Fixed-snapshot v2 next cursors bind the complete query and source version across refresh/unpublish, expire at absolute profile TTL, and are lost on restart. No complete conformance class is claimed."
    feature_id = param("id", "path", RESOURCE_ID, "Published vector dataset identity.", True)
    feature_collection_id = param("id", "path", RESOURCE_ID, "Published vector dataset identity.", True)
    paths["/ogc/features"] = {"get": operation("ogcFeaturesLanding", "Discover the tested Features subset", [], {"200": schema_response("Features landing document.", {"type":"object", "additionalProperties":True})}, feature_subset)}
    paths["/ogc/features/"] = {"get": operation("ogcFeaturesLandingSlash", "Discover the tested Features subset (trailing slash)", [], {"200": schema_response("Features landing document.", {"type":"object", "additionalProperties":True})}, feature_subset)}
    paths["/ogc/features/conformance"] = {"get": operation("ogcFeaturesConformance", "Inspect tested Features subset status", [], {"200": schema_response("Non-certifying tested subset response.", {"type":"object", "additionalProperties":True})}, feature_subset)}
    paths["/ogc/features/collections"] = {"get": operation("ogcFeaturesCollections", "List published vector collections", [param("offset", "query", INTEGER, "Zero-based collection offset; default 0."), param("limit", "query", {"type":"integer", "minimum":1}, "Page size bounded by profile.max_list_items. Responses include self/next links and generation.")], {"200": schema_response("Published vector collection page.", {"type":"object", "additionalProperties":True})}, feature_subset)}
    paths["/ogc/features/collections/{id}"] = {"get": operation("ogcFeaturesCollection", "Read vector collection metadata", [feature_collection_id], {"200": schema_response("Collection metadata.", {"type":"object", "additionalProperties":True})}, feature_subset)}
    feature_query = [feature_id, param("bbox", "query", STRING, "Four finite ordered CRS84 coordinates."), param("offset", "query", INTEGER, "Zero-based source-order offset; cannot accompany cursor."), param("limit", "query", {"type":"integer", "minimum":1}, "Bounded page size."), param("cursor", "query", STRING, "Opaque authenticated v2 token returned by rel=next; must be the only query parameter."),param("datetime","query",STRING,"RFC3339 instant or ordered interval; open endpoints use '..'. Timeless records match valid selections."),param("ids","query",STRING,"Comma-separated typed IDs: n followed by integer, or s followed by UTF-8 hex.")]
    paths["/ogc/features/collections/{id}/items"] = {"get": operation("ogcFeaturesItems", "Query a published vector collection as GeoJSON", feature_query, {"200": schema_response("GeoJSON FeatureCollection with exact numberMatched.", {"type":"object", "additionalProperties":True}, "application/geo+json"), "410": error_response("Snapshot expired, token invalid, or server restarted; restart the query.")}, feature_subset)}
    paths["/ogc/features/collections/{id}/items/{feature_id}"] = {"get": operation("ogcFeaturesItem", "Read one published feature by ID", [feature_id, param("feature_id", "path", STRING, "Typed reversible token: n42 for numeric 42, s3432 for string '42'.", True)], {"200": schema_response("GeoJSON Feature.", {"type":"object", "additionalProperties":True}, "application/geo+json")}, feature_subset)}
    wfs_subset = "WFS 2.0.0 GeoJSON query subset: GetCapabilities, version-wide DescribeFeatureType and GetFeature via KVP or bounded namespace-aware XML. One Query supports FES 2.0 PropertyIsEqualTo/And, ResourceId OR or GML 3.2 CRS84 BBOX. Declared queryable aliases are typed. RESULTTYPE=hits returns exact numberMatched and no features. WFS bbox excludes null/empty geometry. XML rejects DTDs, external entities, unknown namespaces, excess depth/nodes and unsupported expressions. XSD uses reversible UTF-8 hex property names and honest version-wide types; it does not validate GeoJSON. No GML output or full WFS certification. Protocol errors use OWS XML; outer admission errors use problem+json."
    wfs_params = [param("SERVICE", "query", {"type":"string", "enum":["WFS"]}, "Must be WFS.", True), param("VERSION", "query", {"type":"string", "enum":["2.0.0"]}, "Supported version.", True), param("REQUEST", "query", {"type":"string", "enum":["GetCapabilities","DescribeFeatureType","GetFeature"]}, "WFS operation.", True)]
    wfs_post_body = {"required": True, "content": {
        "application/xml": {"schema": {"type": "string"}},
        "text/xml": {"schema": {"type": "string"}},
    }}
    wfs_post = operation(
        "wfsXml", "Serve the WFS XML query subset", [],
        {"200": schema_response("GeoJSON GetFeature result.", {"type":"object", "additionalProperties":True}, "application/geo+json")},
        wfs_subset, requestBody=wfs_post_body)
    paths["/wfs"] = {
        "get": operation("wfsKvp", "Serve the tested WFS KVP subset", wfs_params + [param("TYPENAMES", "query", RESOURCE_ID, "Published vector dataset identity."), param("OUTPUTFORMAT", "query", {"type":"string", "enum":["application/json"]}, "GeoJSON only."), param("COUNT", "query", INTEGER, "Bounded page size."), param("STARTINDEX", "query", INTEGER, "Bounded offset."), param("BBOX", "query", STRING, "Four finite ordered CRS84 coordinates."),param("RESOURCEID","query",STRING,"Comma-separated typed public IDs."),param("RESULTTYPE","query",{"type":"string","enum":["results","hits"]},"Exact hits or source-order page.")], {"200": {"description":"XML capabilities/schema or GeoJSON GetFeature result.","content":{"text/xml":{"schema":STRING},"application/geo+json":{"schema":{"type":"object","additionalProperties":True}}}}}, wfs_subset),
        "post": wfs_post,
    }
    ws_subset = "One-map WebSocket protocol 1. No URL query. Hello precedes init; init/update/retry use ASCII request IDs up to 128 bytes and atomic complete view/variant/playback/output groups. Variant is {id:'default'}. Output supports png or jpeg (explicit jpeg_quality 1-100), default 30 fps and profile ceiling. Accepted controls receive ack in receive order; superseded unstarted frames are discarded, so each ack does not promise a frame. One pending render and one serial writer bound backpressure; a started frame metadata/image pair completes before any later control/event. Metadata carries frame_id, control revision, resource render_revision, time, content_type and bytes. Playing uses a monotonic speed-scaled clock; paused updates redraw. Refresh emits resource-changed and redraws paused state; unpublish finishes any started pair then emits resource-unpublished and closes. Render failure suspends automatic frames until valid update/retry succeeds. Animation work bypasses HTTP caches. Init and each send have operation deadlines; timeout closes. Reconnect is client-owned with complete init against current resources."
    ws_subset += " Independent profile.max_websocket_sessions admission applies for the entire connection; full capacity returns 503. Session buffers retain memory reservations, growing by outgoing image high-water mark. Inbound control frames/messages are limited to 64 KiB; outgoing images use max_image_bytes. Ping runs every websocket_idle_timeout_ms / 3; only received messages/Pong reset the idle timer, and idle expiry sends Close. Missing profile fields default to 64 sessions and 120000 ms idle (minimum 100 ms)."
    paths["/ws/maps/{id}"] = {"get": operation("websocketMap", "Upgrade to the tested WebSocket map subset", [param("id", "path", RESOURCE_ID, "Published map identity bound to this connection.", True)], {"101": {"description":"WebSocket upgrade; hello is sent first, followed by ack/frame pairs after valid controls."}}, ws_subset)}
    for path in ["/wms", "/wmts", "/wfs"]:
        for op in paths[path].values():
            for status, response in op["responses"].items():
                if int(status) >= 400:
                    response["description"] += " Legacy adapter failures use sanitized exception XML; outer routing/admission may return problem+json."
                    response.setdefault("content", {})["text/xml"] = {"schema": STRING}
    paths["/wfs"]["post"]["responses"]["200"]["content"]["text/xml"] = {"schema": STRING}
    return {
        "openapi": "3.0.3", "info": {"title": "Geello experimental HTTP API", "version": "0.2.0",
    "description": "Implemented administration, maps/tiles, OGC Features with fixed snapshots, WMS multi-map PNG/JPEG, WMTS KVP, WFS GeoJSON KVP/XML queries and bounded WebSocket lifecycle. Typed queries are case-sensitive; legacy KVP names are case-insensitive. Unknown/duplicate fields are rejected. X-Request-Id correlates responses. Conditional writes and HEAD are supported. Errors use RFC9457 problem+json or protocol-specific legacy XML. Proxy supplies TLS/admin protection. Conformance classes remain empty; no official OGC or full WFS certification is claimed. Profile limits are experimental; platform/performance acceptance belongs to M5."},
        "servers": [{"url": "/", "description": "The served document replaces this URL with configured public_base_url, including its proxy prefix."}],
        "paths": paths, "components": {"schemas": schemas},
        "externalDocs": {"description": "Offline API guide", "url": "api.html"},
    }


def render(document):
    esc = lambda value: html.escape(str(value), quote=True)
    sections = []
    for path, methods in document["paths"].items():
        for method, op in methods.items():
            parameters = "".join(f"<tr><td><code>{esc(p['name'])}</code></td><td>{esc(p['in'])}{' (required)' if p['required'] else ''}</td><td>{esc(p['description'])}</td></tr>" for p in op["parameters"])
            responses = "".join(f"<tr><td><code>{status}</code></td><td>{esc(value['description'])}</td></tr>" for status, value in op["responses"].items())
            body = op.get("requestBody")
            body_html = "" if body is None else "<p>Required request body: " + ", ".join(f"<code>{esc(media)}</code>" for media in body["content"]) + ". See JSON contract for schemas.</p>"
            sections.append(f"<section id='{esc(op['operationId'])}'><h2><span>{method.upper()}</span> <code>{esc(path)}</code></h2><h3>{esc(op['summary'])}</h3><p>{esc(op['description'])}</p>{body_html}" +
                            (f"<table><caption>Parameters</caption><thead><tr><th>Name</th><th>Location</th><th>Contract</th></tr></thead><tbody>{parameters}</tbody></table>" if parameters else "") +
                            f"<table><caption>Responses</caption><thead><tr><th>Status</th><th>Meaning</th></tr></thead><tbody>{responses}</tbody></table></section>")
    return """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Geello HTTP API</title><style>
body{max-width:1100px;margin:2rem auto;padding:0 1rem;font:16px/1.6 system-ui,sans-serif;color:#17212b;background:#f8fafc}
a{color:#1459a6}code{overflow-wrap:anywhere}h1,h2,h3{line-height:1.3}h2{font-size:1.2rem}h2 span{font-size:.85rem;color:#1459a6}
section{background:white;border:1px solid #d8e0e8;border-radius:8px;margin:2rem 0;padding:1.25rem}table{border-collapse:collapse;width:100%;margin:1rem 0}th,td{text-align:left;vertical-align:top;border-bottom:1px solid #d8e0e8;padding:.5rem}caption{text-align:left;font-weight:600}nav{display:flex;gap:1.5rem;flex-wrap:wrap}
</style></head><body><header><h1>Geello HTTP API</h1><p>Experimental application contract · OpenAPI 3.0.3</p>
<nav><a href="api">Download the JSON contract</a><a href="#renderMap">Map images</a><a href="#renderTile">Tiles</a><a href="#uploadResource">Resource uploads</a></nav>
<p>Requests below are relative to the configured public base URL, including any reverse proxy prefix. The JSON document reports that URL and this binary's enabled readers, backend and selected profile limits. Forwarded headers and Host do not override it.</p>
<p>Writes require exactly one conditional header: <code>If-None-Match: *</code> for absent resources or a current strong <code>If-Match</code> ETag for replacement/deletion/unpublish. Poll accepted operations until <code>ready</code> or <code>failed</code>. Deployments protect administration and TLS through their reverse proxy.</p>
<p>Typed query names are case-sensitive; legacy KVP names are case-insensitive. Unknown and duplicate fields fail. GET routes also support HEAD. Errors use RFC 9457 <code>application/problem+json</code> or legacy protocol exception XML. Features supports fixed snapshots; WFS supports the bounded KVP/XML GeoJSON query subset; WebSocket supports latest-state updates, refresh, playback and PNG/JPEG pairs. No official OGC certification is claimed. This guide is static and works without JavaScript or external assets.</p></header><main>
""" + "\n".join(sections) + "\n</main></body></html>\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    document = build()
    files = {"openapi.json": json.dumps(document, indent=2, ensure_ascii=False) + "\n",
             "api.html": render(document)}
    for name, text in files.items():
        path = OUTPUT / name
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                raise SystemExit(f"Stale generated HTTP document: {path}")
        else:
            OUTPUT.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
    print(f"{'Checked' if args.check else 'Generated'} {len(files)} HTTP documents, {len(document['paths'])} paths")


if __name__ == "__main__":
    main()

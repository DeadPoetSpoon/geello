//! Implemented OGC API Maps/Tiles discovery and render subset.
//!
//! This module deliberately advertises only the common landing, collections,
//! map/tile and tile-matrix operations implemented here. Features, WMS, WMTS,
//! WFS and WebSocket adapters are kept in separate modules because their
//! current implementations are deliberately bounded, tested subsets.
use super::api::TileQuery;
use super::http_extract::{CheckedPath as Path, CheckedQuery as Query};
use super::{api, tile_grid::TileGrid, *};
use crate::{AspectPolicy, Bounds, View};
use axum::{Router, extract::State, http::HeaderMap, response::Response, routing::get};
use serde::Deserialize;
use std::sync::Arc;

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/ogc/maps", get(landing_maps))
        .route("/ogc/maps/", get(landing_maps))
        .route("/ogc/maps/conformance", get(conformance_maps))
        .route("/ogc/maps/api", get(super::http_docs::openapi))
        .route("/ogc/maps/api.html", get(super::http_docs::html))
        .route("/ogc/maps/collections", get(collections))
        .route("/ogc/maps/collections/{id}", get(collection))
        .route("/ogc/maps/collections/{id}/map", get(map_image))
        .route("/ogc/maps/tileMatrixSets", get(tile_matrix_sets))
        .route("/ogc/maps/tileMatrixSets/{id}", get(tile_matrix_set))
        .route("/ogc/maps/collections/{id}/map/tiles", get(tilesets))
        .route("/ogc/maps/collections/{id}/map/tiles/{grid}", get(tileset))
        .route(
            "/ogc/maps/collections/{id}/map/tiles/{grid}/{z}/{row}/{col}",
            get(map_tile),
        )
        .route("/ogc/tiles", get(landing_tiles))
        .route("/ogc/tiles/", get(landing_tiles))
        .route("/ogc/tiles/conformance", get(conformance_tiles))
        .route("/ogc/tiles/api", get(super::http_docs::openapi))
        .route("/ogc/tiles/api.html", get(super::http_docs::html))
        .route("/ogc/tiles/collections", get(collections))
        .route("/ogc/tiles/collections/{id}", get(collection))
        .route("/ogc/tiles/tileMatrixSets", get(tile_matrix_sets))
        .route("/ogc/tiles/tileMatrixSets/{id}", get(tile_matrix_set))
        .route("/ogc/tiles/collections/{id}/map/tiles", get(tilesets))
        .route("/ogc/tiles/collections/{id}/map/tiles/{grid}", get(tileset))
        .route(
            "/ogc/tiles/collections/{id}/map/tiles/{grid}/{z}/{row}/{col}",
            get(map_tile),
        )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapQuery {
    bbox: String,
    crs: String,
    width: u32,
    height: u32,
    geello_time: Option<f64>,
    geello_pixel_ratio: Option<f64>,
}
fn base(s: &ServerState) -> String {
    s.config.public_base_url.trim_end_matches('/').into()
}
fn link(href: String, rel: &str, media: Option<&str>) -> serde_json::Value {
    let mut value = serde_json::json!({"href":href,"rel":rel});
    if let Some(media) = media {
        value["type"] = media.into();
    }
    value
}
fn landing(s: &ServerState, root: &str, desc: &str) -> serde_json::Value {
    let b = base(s);
    serde_json::json!({"title":"Geello","description":desc,
        "links":[link(format!("{b}{root}"),"self",Some("application/json")),
                 link(format!("{b}{root}/conformance"),"conformance",Some("application/json")),
                 link(format!("{b}{root}/collections"),"data",Some("application/json")),
                 link(format!("{b}/api"),"service-desc",Some("application/vnd.oai.openapi+json;version=3.0")),
                 link(format!("{b}/api.html"),"service-doc",Some("text/html"))]})
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyQuery {}

fn workspace(s: &ServerState, bytes: usize) -> Result<crate::operation::Reservation> {
    Ok(s.scheduler
        .memory_context()
        .reserve(memory::envelope(bytes)?)?)
}
async fn landing_maps(
    State(s): State<Arc<ServerState>>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    let _hold = workspace(&s, s.config.public_base_url.len() * 8 + 2048)?;
    api::json_response(
        &s,
        &landing(
            &s,
            "/ogc/maps",
            "Implemented OGC API Maps discovery and PNG rendering subset.",
        ),
    )
}
async fn landing_tiles(
    State(s): State<Arc<ServerState>>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    let _hold = workspace(&s, s.config.public_base_url.len() * 8 + 2048)?;
    api::json_response(
        &s,
        &landing(
            &s,
            "/ogc/tiles",
            "Implemented OGC API Tiles discovery and PNG tile subset.",
        ),
    )
}
async fn conformance_maps(
    State(s): State<Arc<ServerState>>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    conformance(&s, "maps")
}
async fn conformance_tiles(
    State(s): State<Arc<ServerState>>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    conformance(&s, "tiles")
}
fn conformance(s: &ServerState, subset: &str) -> Result<Response> {
    let _hold = workspace(s, 1024)?;
    // Passing local contract tests does not establish an entire OGC class.
    api::json_response(
        s,
        &serde_json::json!({"conformsTo":[],"x-geello-tested-subset":subset,"x-geello-status":"tested subset; not an official conformance certification"}),
    )
}
fn root(uri: &axum::http::Uri) -> &'static str {
    if uri.path().starts_with("/ogc/tiles") {
        "/ogc/tiles"
    } else {
        "/ogc/maps"
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionsQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}

fn map_definition(
    s: &ServerState,
    record: &ResourceRecord,
) -> Result<crate::Accounted<crate::config::MapDefinition>> {
    let text = record
        .definition
        .as_deref()
        .ok_or_else(|| ServerError::io("missing map definition"))?;
    let hold = workspace(s, text.len())?;
    let map = crate::config::parse_map(text)?;
    Ok(crate::Accounted::new(map, vec![hold]))
}
fn collection_value(
    s: &ServerState,
    record: &ResourceRecord,
    root: &str,
) -> Result<serde_json::Value> {
    let map = map_definition(s, record)?;
    let (grids, min, max) = super::tile_grid::publication(&map);
    let b = base(s);
    let id = &record.key.id;
    let crs = map
        .output_crs
        .iter()
        .map(|value| match value.as_str() {
            "OGC:CRS84" => "http://www.opengis.net/def/crs/OGC/1.3/CRS84".to_owned(),
            "EPSG:3857" => "http://www.opengis.net/def/crs/EPSG/0/3857".to_owned(),
            other => other.to_owned(),
        })
        .collect::<Vec<_>>();
    let mut links = vec![link(
        format!("{b}{root}/collections/{id}"),
        "self",
        Some("application/json"),
    )];
    links.push(link(
        format!("{b}{root}/collections/{id}/map"),
        "map",
        Some("image/png"),
    ));
    if !grids.is_empty() {
        links.push(link(
            format!("{b}{root}/collections/{id}/map/tiles"),
            "tilesets-map",
            Some("application/json"),
        ));
    }
    Ok(
        serde_json::json!({"id":id,"title":map.title.as_deref().unwrap_or(id),"itemType":"map","crs":crs,"links":links,"x-geello-tile-matrix-sets":grids.iter().map(|g|g.id()).collect::<Vec<_>>(),"x-geello-min-zoom":min,"x-geello-max-zoom":max}),
    )
}
async fn collections(
    State(s): State<Arc<ServerState>>,
    uri: axum::http::Uri,
    Query(q): Query<CollectionsQuery>,
) -> Result<Response> {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(s.config.profile.max_list_items);
    let (generation, records, more) = s.catalog.public_maps(offset, limit)?;
    let bytes =
        records
            .iter()
            .try_fold(2048 + s.config.public_base_url.len() * 8, |bytes, r| {
                bytes
                    .checked_add(
                        r.definition.as_ref().map_or(0, String::len)
                            + r.key.id.len() * 8
                            + 2048
                            + s.config.public_base_url.len() * 8,
                    )
                    .ok_or_else(|| ServerError::from(crate::GeelloError::SizeOverflow))
            })?;
    let _hold = workspace(&s, bytes)?;
    let root = root(&uri);
    let b = base(&s);
    let collections = records
        .iter()
        .map(|r| collection_value(&s, r, root))
        .collect::<Result<Vec<_>>>()?;
    let mut links = vec![link(
        format!("{b}{root}/collections?offset={offset}&limit={limit}"),
        "self",
        Some("application/json"),
    )];
    if more {
        links.push(link(
            format!(
                "{b}{root}/collections?offset={}&limit={limit}",
                offset
                    .checked_add(records.len())
                    .ok_or(crate::GeelloError::SizeOverflow)?
            ),
            "next",
            Some("application/json"),
        ));
    }
    api::json_response(
        &s,
        &serde_json::json!({"collections":collections,"links":links,"x-geello-generation":generation}),
    )
}
async fn collection(
    State(s): State<Arc<ServerState>>,
    uri: axum::http::Uri,
    Path((id,)): Path<(String,)>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    let key = ResourceKey::new(ResourceKind::Map, id.clone())?;
    let record = s.catalog.get_accounted(&key)?;
    if !record.published {
        return Err(ServerError::new(
            404,
            "resource-unpublished",
            "Collection is not published",
        ));
    }
    let _hold = workspace(
        &s,
        record.definition.as_ref().map_or(0, String::len)
            + s.config.public_base_url.len() * 8
            + 4096,
    )?;
    api::json_response(&s, &collection_value(&s, &record, root(&uri))?)
}
fn parse_bbox(q: &MapQuery) -> Result<View> {
    let mut it = q.bbox.split(',');
    let mut n = [0.; 4];
    for x in &mut n {
        *x = it
            .next()
            .and_then(|v| v.parse().ok())
            .filter(|v: &f64| v.is_finite())
            .ok_or_else(|| ServerError::invalid("bbox requires four finite XY coordinates"))?;
    }
    if it.next().is_some() {
        return Err(ServerError::invalid("bbox requires four coordinates"));
    }
    let view = View {
        crs: super::runtime::parse_crs(&q.crs)?,
        bounds: Bounds {
            min_x: n[0],
            min_y: n[1],
            max_x: n[2],
            max_y: n[3],
        },
        width: q.width,
        height: q.height,
        aspect: AspectPolicy::Stretch,
    };
    view.validate()
        .map_err(|_| ServerError::invalid("Invalid map view"))?;
    Ok(view)
}
async fn map_image(
    State(s): State<Arc<ServerState>>,
    Path((id,)): Path<(String,)>,
    headers: HeaderMap,
    Query(q): Query<MapQuery>,
) -> Result<Response> {
    let view = parse_bbox(&q)?;
    let image = s
        .render_map_scaled(
            &id,
            view,
            q.geello_time.unwrap_or(0.),
            q.geello_pixel_ratio.unwrap_or(1.),
        )
        .await?;
    api::response(&s, &image, &headers)
}
async fn map_tile(
    State(s): State<Arc<ServerState>>,
    Path((id, grid, z, row, col)): Path<(String, String, u32, u32, u32)>,
    headers: HeaderMap,
    Query(q): Query<TileQuery>,
) -> Result<Response> {
    let grid = TileGrid::parse(&grid)?;
    let image = s
        .render_protocol_tile_at(&id, grid, [z, col, row], q.geello_time.unwrap_or(0.))
        .await?;
    api::response(&s, &image, &headers)
}
const GRIDS: [TileGrid; 2] = [TileGrid::WebMercatorQuad, TileGrid::WorldCrs84Quad];

async fn tile_matrix_sets(
    State(s): State<Arc<ServerState>>,
    uri: axum::http::Uri,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    let _hold = workspace(&s, s.config.public_base_url.len() * 4 + 2048)?;
    let b = base(&s);
    let root = root(&uri);
    let sets = GRIDS.iter().map(|g| serde_json::json!({"id":g.id(),"crs":g.crs_uri(),"links":[link(format!("{b}{root}/tileMatrixSets/{}",g.id()),"self",Some("application/json"))]})).collect::<Vec<_>>();
    api::json_response(&s, &serde_json::json!({"tileMatrixSets":sets}))
}
async fn tile_matrix_set(
    State(s): State<Arc<ServerState>>,
    Path((id,)): Path<(String,)>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    let grid = TileGrid::parse(&id)?;
    let _hold = workspace(&s, 32768)?;
    let matrices = (0..=30).map(|z| {
        let [columns,rows] = grid.dimensions(z)?;
        let cell = grid.cell_size(z)?;
        let metres_per_unit = if grid == TileGrid::WorldCrs84Quad { 111319.49079327358 } else { 1. };
        Ok(serde_json::json!({"id":z.to_string(),"scaleDenominator":cell * metres_per_unit / 0.00028,"cellSize":cell,"pointOfOrigin":grid.origin(),"cornerOfOrigin":"topLeft","tileWidth":256,"tileHeight":256,"matrixWidth":columns,"matrixHeight":rows}))
    }).collect::<Result<Vec<_>>>()?;
    api::json_response(
        &s,
        &serde_json::json!({"id":grid.id(),"crs":grid.crs_uri(),"orderedAxes":if grid==TileGrid::WorldCrs84Quad { ["Lon","Lat"] } else { ["X","Y"] },"tileMatrices":matrices}),
    )
}

fn published_map(s: &ServerState, id: String) -> Result<crate::Accounted<ResourceRecord>> {
    let record = s
        .catalog
        .get_accounted(&ResourceKey::new(ResourceKind::Map, id)?)?;
    if !record.published {
        return Err(ServerError::new(
            404,
            "resource-unpublished",
            "Collection is not published",
        ));
    }
    Ok(record)
}
fn tileset_value(
    s: &ServerState,
    root: &str,
    id: &str,
    grid: TileGrid,
    min: u32,
    max: u32,
) -> Result<serde_json::Value> {
    let b = base(s);
    let limits = (min..=max).map(|z| {
        let [width,height] = grid.dimensions(z)?;
        Ok(serde_json::json!({"tileMatrix":z.to_string(),"minTileRow":0,"maxTileRow":height-1,"minTileCol":0,"maxTileCol":width-1}))
    }).collect::<Result<Vec<_>>>()?;
    Ok(
        serde_json::json!({"dataType":"map","crs":grid.crs_uri(),"tileMatrixSetId":grid.id(),"tileMatrixSetURI":format!("{b}{root}/tileMatrixSets/{}",grid.id()),"tileMatrixSetLimits":limits,"links":[link(format!("{b}{root}/collections/{id}/map/tiles/{}",grid.id()),"self",Some("application/json")),link(format!("{b}{root}/tileMatrixSets/{}",grid.id()),"tiling-scheme",Some("application/json")),{"href":format!("{b}{root}/collections/{id}/map/tiles/{}/{{z}}/{{row}}/{{col}}",grid.id()),"rel":"item","type":"image/png","templated":true}]}),
    )
}
async fn tilesets(
    State(s): State<Arc<ServerState>>,
    uri: axum::http::Uri,
    Path((id,)): Path<(String,)>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    let record = published_map(&s, id)?;
    let map = map_definition(&s, &record)?;
    let (grids, min, max) = super::tile_grid::publication(&map);
    let _hold = workspace(&s, s.config.public_base_url.len() * 16 + 65536)?;
    let sets = grids
        .into_iter()
        .map(|g| tileset_value(&s, root(&uri), &record.key.id, g, min, max))
        .collect::<Result<Vec<_>>>()?;
    api::json_response(&s, &serde_json::json!({"tilesets":sets}))
}
async fn tileset(
    State(s): State<Arc<ServerState>>,
    uri: axum::http::Uri,
    Path((id, grid)): Path<(String, String)>,
    Query(_): Query<EmptyQuery>,
) -> Result<Response> {
    let grid = TileGrid::parse(&grid)?;
    let record = published_map(&s, id)?;
    let map = map_definition(&s, &record)?;
    let (_, min, max) = super::tile_grid::publication(&map);
    super::tile_grid::validate_publication(&map, grid, min)?;
    let _hold = workspace(&s, s.config.public_base_url.len() * 8 + 32768)?;
    api::json_response(
        &s,
        &tileset_value(&s, root(&uri), &record.key.id, grid, min, max)?,
    )
}

//! Strict, deliberately small legacy protocol adapters.
//!
//! These endpoints provide useful KVP interoperability while the complete
//! WMS/WMTS standards remain outside the current M4 claim. Capabilities and
//! tile metadata are generated from the same published map/grid contracts as
//! OGC Maps/Tiles; arbitrary styles, layers and grids are rejected.
use super::{api, features, tile_grid::TileGrid, *};
use crate::View;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, header},
    response::Response,
    routing::get,
};
use std::{collections::HashMap, sync::Arc};
mod schema;
async fn describe_feature_type(s: Arc<ServerState>, id: String) -> Result<Response> {
    schema::describe(s, id).await
}

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/wms", get(wms))
        .route("/wmts", get(wmts))
        .route("/wfs", get(wfs).post(wfs_post))
        .layer(axum::middleware::from_fn(legacy_errors))
}
async fn legacy_errors(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    use axum::response::IntoResponse;
    let wms = request.uri().path() == "/wms";
    let response = next.run(request).await;
    if !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }
    let status = response.status();
    let allow = response.headers().get(header::ALLOW).cloned();
    // Protocol errors expose only fixed categories, never parser/source details.
    let code = if status.as_u16() == 404 {
        "InvalidParameterValue"
    } else if status.as_u16() == 501 {
        "OperationNotSupported"
    } else if status.as_u16() < 500 {
        "InvalidParameterValue"
    } else {
        "NoApplicableCode"
    };
    let text = if wms {
        format!(
            r#"<?xml version="1.0"?><ServiceExceptionReport xmlns="http://www.opengis.net/ogc" version="1.3.0"><ServiceException code="{code}">Request failed</ServiceException></ServiceExceptionReport>"#
        )
    } else {
        format!(
            r#"<?xml version="1.0"?><ows:ExceptionReport xmlns:ows="http://www.opengis.net/ows/1.1" version="2.0.0"><ows:Exception exceptionCode="{code}"><ows:ExceptionText>Request failed</ows:ExceptionText></ows:Exception></ows:ExceptionReport>"#
        )
    };
    let mut response = (
        status,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        text,
    )
        .into_response();
    if let Some(allow) = allow {
        response.headers_mut().insert(header::ALLOW, allow);
    }
    response
}

async fn wfs_post(
    State(s): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response> {
    let ctx = s.scheduler.context();
    let _body_memory = ctx.reserve(
        s.config
            .profile
            .max_registry_bytes
            .checked_mul(2)
            .ok_or(crate::GeelloError::SizeOverflow)?,
    )?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .unwrap_or("");
    if !matches!(content_type, "application/xml" | "text/xml") {
        return Err(ServerError::new(
            415,
            "content-type",
            "WFS XML requires application/xml or text/xml",
        ));
    }
    let bytes = tokio::time::timeout(
        std::time::Duration::from_millis(s.config.profile.operation_timeout_ms),
        to_bytes(body, s.config.profile.max_registry_bytes),
    )
    .await
    .map_err(|_| ServerError::new(408, "request-timeout", "WFS XML request timed out"))?
    .map_err(|_| ServerError::new(413, "body-limit", "WFS XML request exceeds profile"))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ServerError::invalid("WFS XML request must be UTF-8"))?;
    let text = text.to_owned();
    let (name, attrs) = s
        .scheduler
        .blocking(WorkClass::Http, ctx, move |_| parse_wfs_xml(&text))
        .await?;
    let service = attrs.get("service").map(String::as_str).unwrap_or("WFS");
    let version = attrs.get("version").map(String::as_str).unwrap_or("2.0.0");
    if !service.eq_ignore_ascii_case("WFS") || version != "2.0.0" {
        return Err(ServerError::invalid(
            "WFS XML SERVICE/VERSION is unsupported",
        ));
    }
    if name == "GetCapabilities" {
        return wfs(
            State(s),
            "/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetCapabilities"
                .parse()
                .unwrap(),
        )
        .await;
    }
    let id = attrs
        .get("typeNames")
        .or_else(|| attrs.get("typename"))
        .ok_or_else(|| ServerError::invalid("WFS XML typeNames is required"))?;
    if name == "DescribeFeatureType" {
        ResourceKey::new(ResourceKind::Dataset, id)?;
        let uri =
            format!("/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=DescribeFeatureType&TYPENAMES={id}")
                .parse()
                .map_err(|_| ServerError::invalid("Invalid WFS typeNames"))?;
        return wfs(State(s), uri).await;
    }
    let _ = dataset_id(&s, id)?;
    if attrs
        .get("outputFormat")
        .is_some_and(|v| v != "application/json")
    {
        return Err(ServerError::invalid(
            "WFS XML outputFormat must be application/json",
        ));
    }
    let mut parts = Vec::new();
    for (key, value) in &attrs {
        if let Some(field) = key.strip_prefix("eq:") {
            parts.push(format!("{}={}", url_encode(field), url_encode(value)));
        }
    }
    for key in ["ids", "resultType"] {
        if let Some(value) = attrs.get(key) {
            parts.push(format!("{key}={}", url_encode(value)));
        }
    }
    if let Some(value) = attrs.get("bbox") {
        parse_bbox(value)?;
        parts.push(format!("bbox={value}"));
    }
    if let Some(value) = attrs.get("count") {
        value
            .parse::<usize>()
            .map_err(|_| ServerError::invalid("Invalid WFS XML count"))?;
        parts.push(format!("limit={value}"));
    }
    if let Some(value) = attrs.get("startIndex") {
        value
            .parse::<usize>()
            .map_err(|_| ServerError::invalid("Invalid WFS XML startIndex"))?;
        parts.push(format!("offset={value}"));
    }
    let uri = if parts.is_empty() {
        format!("/ogc/features/collections/{id}/items")
    } else {
        format!("/ogc/features/collections/{id}/items?{}", parts.join("&"))
    };
    let uri = uri
        .parse()
        .map_err(|_| ServerError::invalid("Invalid WFS XML query"))?;
    features::execute_items(s, id.to_owned(), uri, true).await
}

mod xml;
fn parse_wfs_xml(text: &str) -> Result<(String, HashMap<String, String>)> {
    xml::parse(text)
}

async fn wfs(State(s): State<Arc<ServerState>>, uri: axum::http::Uri) -> Result<Response> {
    let mut allowed = vec![
        "SERVICE",
        "VERSION",
        "REQUEST",
        "TYPENAMES",
        "OUTPUTFORMAT",
        "COUNT",
        "STARTINDEX",
        "BBOX",
        "RESOURCEID",
        "RESULTTYPE",
    ];
    let raw = uri.query().unwrap_or("");
    let id = raw
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| k.eq_ignore_ascii_case("TYPENAMES"))
        .map(|(_, v)| features::decode_query_value(v))
        .transpose()?;
    let mut aliases = Vec::new();
    if let Some(id) = &id {
        let record = dataset_id(&s, id)?;
        if let Some(definition) = record.definition.as_deref() {
            let definition: crate::config::DatasetDefinition = crate::config::parse(definition)?;
            aliases = definition.queryables.into_iter().map(|q| q.name).collect();
        }
    }
    allowed.extend(aliases.iter().map(String::as_str));
    let q = strict_query(raw, &allowed)?;
    if !required(&q, "SERVICE")?.eq_ignore_ascii_case("WFS") || required(&q, "VERSION")? != "2.0.0"
    {
        return Err(ServerError::invalid("WFS SERVICE/VERSION is unsupported"));
    }
    match required(&q, "REQUEST")?.to_ascii_lowercase().as_str() {
        "getcapabilities" => wfs_capabilities(&s),
        "describefeaturetype" => {
            describe_feature_type(s.clone(), required(&q, "TYPENAMES")?.to_owned()).await
        }
        "getfeature" => {
            if q.get("OUTPUTFORMAT")
                .is_some_and(|v| !v.eq_ignore_ascii_case("application/json"))
            {
                return Err(ServerError::invalid(
                    "WFS OUTPUTFORMAT must be application/json",
                ));
            }
            let id = required(&q, "TYPENAMES")?;
            let _ = dataset_id(&s, id)?;
            let mut query = format!("/ogc/features/collections/{id}/items");
            let mut parts = Vec::new();
            for (name, target) in [
                ("BBOX", "bbox"),
                ("COUNT", "limit"),
                ("STARTINDEX", "offset"),
                ("RESOURCEID", "ids"),
                ("RESULTTYPE", "resultType"),
            ] {
                if let Some(v) = q.get(name) {
                    parts.push(format!("{target}={}", url_encode(v)));
                }
            }
            for alias in &aliases {
                if let Some(value) = q.get(alias) {
                    parts.push(format!("{}={}", url_encode(alias), url_encode(value)));
                }
            }
            if !parts.is_empty() {
                query.push('?');
                query.push_str(&parts.join("&"));
            }
            let uri = query
                .parse()
                .map_err(|_| ServerError::invalid("Invalid WFS query URI"))?;
            features::execute_items(s, id.to_owned(), uri, true).await
        }
        _ => Err(ServerError::new(
            501,
            "protocol-operation",
            "WFS operation is not implemented",
        )),
    }
}
fn dataset_id(s: &ServerState, id: &str) -> Result<crate::Accounted<ResourceRecord>> {
    let r = s
        .catalog
        .get_accounted(&ResourceKey::new(ResourceKind::Dataset, id.to_owned())?)?;
    if !r.published || r.class.as_deref() != Some("vector") {
        return Err(ServerError::new(
            404,
            "resource-unpublished",
            "Feature collection is not published",
        ));
    }
    Ok(r)
}

fn required<'a>(q: &'a HashMap<String, String>, name: &str) -> Result<&'a str> {
    q.get(name)
        .map(String::as_str)
        .ok_or_else(|| ServerError::invalid("Missing protocol parameter"))
}
fn strict_query(raw: &str, allowed: &[&str]) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for part in raw.split('&').filter(|p| !p.is_empty()) {
        let (name, value) = part
            .split_once('=')
            .ok_or_else(|| ServerError::invalid("Malformed protocol parameter"))?;
        if !allowed.iter().any(|key| key.eq_ignore_ascii_case(name)) {
            return Err(ServerError::invalid("Unknown protocol parameter"));
        }
        let canonical = allowed
            .iter()
            .find(|key| key.eq_ignore_ascii_case(name))
            .unwrap();
        if out
            .insert(
                (*canonical).to_owned(),
                features::decode_query_value(value)?,
            )
            .is_some()
        {
            return Err(ServerError::invalid("Duplicate protocol parameter"));
        }
    }
    Ok(out)
}
fn parse_bbox(value: &str) -> Result<[f64; 4]> {
    let values = value
        .split(',')
        .map(|v| v.parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| ServerError::invalid("BBOX requires finite coordinates"))?;
    if values.len() != 4 || values[0] >= values[2] || values[1] >= values[3] {
        return Err(ServerError::invalid(
            "BBOX requires four ordered coordinates",
        ));
    }
    Ok([values[0], values[1], values[2], values[3]])
}
fn xml_response(s: &ServerState, text: String) -> Result<Response> {
    if text.len() > s.config.profile.max_registry_bytes {
        return Err(ServerError::new(
            503,
            "xml-budget",
            "XML response exceeds profile",
        ));
    }
    api::bytes_response(s, text.into_bytes(), "text/xml; charset=utf-8")
}

async fn wms(
    State(s): State<Arc<ServerState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Response> {
    let q = strict_query(
        uri.query().unwrap_or(""),
        &[
            "SERVICE",
            "VERSION",
            "REQUEST",
            "LAYERS",
            "STYLES",
            "CRS",
            "BBOX",
            "WIDTH",
            "HEIGHT",
            "FORMAT",
            "TRANSPARENT",
            "BGCOLOR",
            "TIME",
            "geello_time",
            "geello_pixel_ratio",
        ],
    )?;
    if !q
        .get("SERVICE")
        .is_some_and(|v| v.eq_ignore_ascii_case("WMS"))
        || q.get("VERSION").is_none_or(|v| v != "1.3.0")
    {
        return Err(ServerError::invalid("WMS SERVICE/VERSION is unsupported"));
    }
    match required(&q, "REQUEST")?.to_ascii_lowercase().as_str() {
        "getcapabilities" => wms_capabilities(&s).await,
        "getmap" => wms_map(&s, &headers, &q).await,
        _ => Err(ServerError::new(
            501,
            "protocol-operation",
            "WMS operation is not implemented",
        )),
    }
}
async fn wms_capabilities(s: &ServerState) -> Result<Response> {
    let _workspace = s.scheduler.memory_context().reserve(
        s.config
            .profile
            .max_registry_bytes
            .checked_mul(2)
            .ok_or(crate::GeelloError::SizeOverflow)?,
    )?;
    let (_generation, records, _) = s.catalog.public_maps(0, s.config.profile.max_list_items)?;
    let mut layers = String::new();
    for record in records.iter() {
        let map = crate::config::parse_map(
            record
                .definition
                .as_deref()
                .ok_or_else(|| ServerError::io("missing map"))?,
        )?;
        layers.push_str(&format!(
            "<Layer><Name>{}</Name><Title>{}</Title>",
            xml_escape(&record.key.id),
            xml_escape(&record.key.id)
        ));
        for crs in &map.output_crs {
            layers.push_str(&format!("<CRS>{}</CRS>", xml_escape(crs)));
            if crs == "OGC:CRS84" {
                layers.push_str("<CRS>CRS:84</CRS><CRS>EPSG:4326</CRS>");
            }
        }
        layers.push_str("</Layer>");
    }
    xml_response(
        s,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><WMS_Capabilities xmlns="http://www.opengis.net/wms" version="1.3.0"><Service><Title>Geello tested WMS subset</Title></Service><Capability><Request><GetCapabilities/><GetMap><Format>image/png</Format><Format>image/jpeg</Format></GetMap></Request><Layer>{layers}</Layer></Capability><geello:status xmlns:geello="urn:geello">tested subset; not an official WMS conformance certification</geello:status></WMS_Capabilities>"#
        ),
    )
}
fn wfs_capabilities(s: &ServerState) -> Result<Response> {
    let _workspace = s.scheduler.memory_context().reserve(
        s.config
            .profile
            .max_registry_bytes
            .checked_mul(2)
            .ok_or(crate::GeelloError::SizeOverflow)?,
    )?;
    let (_, records, _) = s
        .catalog
        .public_datasets(0, s.config.profile.max_list_items)?;
    let types = records.iter().map(|r| format!("<FeatureType><Name>{}</Name><Title>{}</Title><DefaultCRS>urn:ogc:def:crs:OGC::CRS84</DefaultCRS><OutputFormats><Format>application/json</Format></OutputFormats></FeatureType>",xml_escape(&r.key.id),xml_escape(&r.key.id))).collect::<String>();
    xml_response(
        s,
        format!(
            r#"<?xml version="1.0"?><WFS_Capabilities xmlns="http://www.opengis.net/wfs/2.0" xmlns:ows="http://www.opengis.net/ows/1.1" xmlns:fes="http://www.opengis.net/fes/2.0" version="2.0.0"><ows:ServiceIdentification><ows:Title>Geello tested WFS GeoJSON query subset</ows:Title></ows:ServiceIdentification><ows:OperationsMetadata><ows:Operation name="GetCapabilities"/><ows:Operation name="DescribeFeatureType"/><ows:Operation name="GetFeature"><ows:Parameter name="outputFormat"><ows:AllowedValues><ows:Value>application/json</ows:Value></ows:AllowedValues></ows:Parameter></ows:Operation></ows:OperationsMetadata><FeatureTypeList>{types}</FeatureTypeList><fes:Filter_Capabilities><fes:Scalar_Capabilities><fes:ComparisonOperators><fes:ComparisonOperator name="PropertyIsEqualTo"/></fes:ComparisonOperators></fes:Scalar_Capabilities></fes:Filter_Capabilities><geello:status xmlns:geello="urn:geello">tested subset; GeoJSON only; no GML or official WFS conformance certification</geello:status></WFS_Capabilities>"#
        ),
    )
}

fn wmts_capabilities(s: &ServerState) -> Result<Response> {
    let _workspace = s.scheduler.memory_context().reserve(
        s.config
            .profile
            .max_registry_bytes
            .checked_mul(2)
            .ok_or(crate::GeelloError::SizeOverflow)?,
    )?;
    let (_, records, _) = s.catalog.public_maps(0, s.config.profile.max_list_items)?;
    let mut layers = String::new();
    for record in records.iter() {
        let map = crate::config::parse_map(
            record
                .definition
                .as_deref()
                .ok_or_else(|| ServerError::io("missing map"))?,
        )?;
        let (grids, min, max) = super::tile_grid::publication(&map);
        if grids.is_empty() {
            continue;
        }
        layers.push_str(&format!("<Layer><ows:Title>{}</ows:Title><ows:Identifier>{}</ows:Identifier><Style isDefault=\"true\"><ows:Identifier>default</ows:Identifier></Style><Format>image/png</Format>",xml_escape(&record.key.id),xml_escape(&record.key.id)));
        for grid in grids {
            layers.push_str(&format!(
                "<TileMatrixSetLink><TileMatrixSet>{}</TileMatrixSet><TileMatrixSetLimits>",
                grid.id()
            ));
            for z in min..=max {
                let [width, height] = grid.dimensions(z)?;
                layers.push_str(&format!("<TileMatrixLimits><TileMatrix>{z}</TileMatrix><MinTileRow>0</MinTileRow><MaxTileRow>{}</MaxTileRow><MinTileCol>0</MinTileCol><MaxTileCol>{}</MaxTileCol></TileMatrixLimits>",height-1,width-1));
            }
            layers.push_str("</TileMatrixSetLimits></TileMatrixSetLink>");
        }
        layers.push_str("</Layer>");
    }
    let mut sets = String::new();
    for grid in [TileGrid::WebMercatorQuad, TileGrid::WorldCrs84Quad] {
        sets.push_str(&format!("<TileMatrixSet><ows:Identifier>{}</ows:Identifier><ows:SupportedCRS>{}</ows:SupportedCRS>",grid.id(),grid.crs_uri()));
        let [x, y] = grid.origin();
        for z in 0..=30 {
            let [width, height] = grid.dimensions(z)?;
            let metres = if grid == TileGrid::WorldCrs84Quad {
                std::f64::consts::PI * 6378137.0 / 180.0
            } else {
                1.0
            };
            let scale = grid.cell_size(z)? * metres / 0.00028;
            sets.push_str(&format!("<TileMatrix><ows:Identifier>{z}</ows:Identifier><ScaleDenominator>{scale}</ScaleDenominator><TopLeftCorner>{x} {y}</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>{width}</MatrixWidth><MatrixHeight>{height}</MatrixHeight></TileMatrix>"));
        }
        sets.push_str("</TileMatrixSet>");
    }
    xml_response(
        s,
        format!(
            r#"<?xml version="1.0"?><Capabilities xmlns="http://www.opengis.net/wmts/1.0" xmlns:ows="http://www.opengis.net/ows/1.1" version="1.0.0"><ows:ServiceIdentification><ows:Title>Geello tested WMTS subset</ows:Title></ows:ServiceIdentification><Contents>{layers}{sets}</Contents><geello:status xmlns:geello="urn:geello">tested subset; not an official WMTS conformance certification</geello:status></Capabilities>"#
        ),
    )
}
async fn wms_map(
    s: &Arc<ServerState>,
    headers: &HeaderMap,
    q: &HashMap<String, String>,
) -> Result<Response> {
    let format = required(q, "FORMAT")?.to_ascii_lowercase();
    if !matches!(format.as_str(), "image/png" | "image/jpeg") {
        return Err(ServerError::invalid("Unsupported WMS FORMAT"));
    }
    let ids = required(q, "LAYERS")?
        .split(',')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if q.get("STYLES")
        .is_some_and(|v| v.split(',').any(|s| !s.is_empty()))
    {
        return Err(ServerError::invalid("WMS STYLES must be empty"));
    }
    let [mut a, mut b, mut c, mut d] = parse_bbox(required(q, "BBOX")?)?;
    let crs = required(q, "CRS")?;
    if crs == "EPSG:4326" {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut c, &mut d);
    }
    let crs = if matches!(crs, "CRS:84" | "CRS84" | "EPSG:4326") {
        "OGC:CRS84"
    } else {
        crs
    };
    let width = q
        .get("WIDTH")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ServerError::invalid("Invalid WMS WIDTH"))?;
    let height = q
        .get("HEIGHT")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ServerError::invalid("Invalid WMS HEIGHT"))?;
    let view = View {
        crs: super::runtime::parse_crs(crs)?,
        bounds: crate::Bounds {
            min_x: a,
            min_y: b,
            max_x: c,
            max_y: d,
        },
        width,
        height,
        aspect: crate::AspectPolicy::Stretch,
    };
    if q.contains_key("TIME") && q.contains_key("geello_time") {
        return Err(ServerError::invalid("Ambiguous time parameters"));
    }
    let time = parse_time(q.get("geello_time").or_else(|| q.get("TIME")))?;
    let transparent = match q
        .get("TRANSPARENT")
        .map(|v| v.to_ascii_lowercase())
        .as_deref()
    {
        None | Some("false") => false,
        Some("true") => true,
        _ => return Err(ServerError::invalid("Invalid TRANSPARENT")),
    };
    let background = if let Some(value) = q.get("BGCOLOR") {
        let hex = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .ok_or_else(|| ServerError::invalid("BGCOLOR requires 0xRRGGBB"))?;
        if hex.len() != 6 {
            return Err(ServerError::invalid("Invalid BGCOLOR"));
        }
        crate::config::parse_color(&format!("#{hex}"))?
    } else {
        crate::Color([255, 255, 255, 255])
    };
    let encoding = if format == "image/jpeg" {
        crate::encoding::ImageFormat::Jpeg {
            quality: 90,
            background: [background.0[0], background.0[1], background.0[2]],
        }
    } else {
        crate::encoding::ImageFormat::Png
    };
    let image = s
        .render_protocol_maps_scaled(
            ids,
            view,
            time,
            if transparent { None } else { Some(background) },
            encoding,
            q.get("geello_pixel_ratio")
                .map(|value| {
                    value
                        .parse::<f64>()
                        .map_err(|_| ServerError::invalid("Invalid geello_pixel_ratio"))
                })
                .transpose()?
                .unwrap_or(1.),
        )
        .await?;
    api::response(s, &image, headers)
}

async fn wmts(
    State(s): State<Arc<ServerState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Response> {
    let q = strict_query(
        uri.query().unwrap_or(""),
        &[
            "SERVICE",
            "VERSION",
            "REQUEST",
            "LAYER",
            "STYLE",
            "TILEMATRIXSET",
            "TILEMATRIX",
            "TILEROW",
            "TILECOL",
            "FORMAT",
            "geello_time",
        ],
    )?;
    if !required(&q, "SERVICE")?.eq_ignore_ascii_case("WMTS") || required(&q, "VERSION")? != "1.0.0"
    {
        return Err(ServerError::invalid("WMTS SERVICE/VERSION is unsupported"));
    }
    match required(&q, "REQUEST")?.to_ascii_lowercase().as_str() {
        "getcapabilities" => wmts_capabilities(&s),
        "gettile" => {
            if q.get("STYLE").is_some_and(|v| v != "default")
                || !q
                    .get("FORMAT")
                    .is_some_and(|v| v.eq_ignore_ascii_case("image/png"))
            {
                return Err(ServerError::invalid("WMTS STYLE/FORMAT is unsupported"));
            }
            let id = required(&q, "LAYER")?;
            let grid = TileGrid::parse(required(&q, "TILEMATRIXSET")?)?;
            let z = required(&q, "TILEMATRIX")?
                .parse()
                .map_err(|_| ServerError::invalid("Invalid WMTS TILEMATRIX"))?;
            let row = required(&q, "TILEROW")?
                .parse()
                .map_err(|_| ServerError::invalid("Invalid WMTS TILEROW"))?;
            let col = required(&q, "TILECOL")?
                .parse()
                .map_err(|_| ServerError::invalid("Invalid WMTS TILECOL"))?;
            let time = parse_time(q.get("geello_time"))?;
            let image = s
                .render_protocol_tile_at(id, grid, [z, col, row], time)
                .await?;
            api::response(&s, &image, &headers)
        }
        _ => Err(ServerError::new(
            501,
            "protocol-operation",
            "WMTS operation is not implemented",
        )),
    }
}
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn parse_time(value: Option<&String>) -> Result<f64> {
    value.map_or(Ok(0.), |v| {
        v.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.)
            .ok_or_else(|| ServerError::invalid("Invalid animation time"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_xml_parser_rejects_nested_queries_trailing_content_and_mismatched_roots() {
        for text in [
            r#"<GetFeature typeNames="test:data"/>"#,
            r#"<w:GetFeature xmlns:w="urn:wrong" typeNames="test:data"/>"#,
            r#"<!DOCTYPE GetFeature [<!ENTITY x SYSTEM "file:///private">]><GetFeature xmlns="http://www.opengis.net/wfs/2.0" typeNames="test:data"/>"#,
            r#"<GetFeature typeNames="test:data"><Query/></GetFeature>"#,
            r#"<GetFeature typeNames="test:data">ignored</GetFeature>"#,
            r#"<GetFeature xmlns="http://www.opengis.net/wfs/2.0" typeNames="test:data"/><Query/>"#,
            r#"<GetFeature xmlns="http://www.opengis.net/wfs/2.0" typeNames="test:data"/></GetFeature>"#,
            r#"<wfs:GetFeature typeNames="test:data"></GetFeature>"#,
            r#"<GetFeature typeNames="test:data"></wfs:GetFeature>"#,
            r#"<GetFeature typeNames="test:data"></GetFeature><GetFeature/>"#,
        ] {
            assert!(parse_wfs_xml(text).is_err(), "{text}");
        }
        for text in [
            r#"<?xml version='1.0' encoding='UTF-8'?><q:GetFeature xmlns:q='http://www.opengis.net/wfs/2.0'><q:Query typeNames='test:data'/></q:GetFeature>"#,
            r#"<GetFeature xmlns="http://www.opengis.net/wfs/2.0" typeNames="test:data"/>"#,
            r#"<GetFeature xmlns="http://www.opengis.net/wfs/2.0" typeNames="test:data">  </GetFeature>"#,
            r#"<wfs:GetFeature xmlns:wfs="http://www.opengis.net/wfs/2.0" typeNames="test:data"></wfs:GetFeature>"#,
        ] {
            let (name, attributes) = parse_wfs_xml(text).unwrap();
            assert_eq!(name, "GetFeature");
            assert_eq!(attributes["typeNames"], "test:data");
        }
    }
    #[test]
    fn xml_fes_subset_binds_namespaces_and_preserves_literal_data() {
        let xml = r#"<w:GetFeature xmlns:w="http://www.opengis.net/wfs/2.0" xmlns:f="http://www.opengis.net/fes/2.0" xmlns:g="http://www.opengis.net/gml/3.2" resultType="hits"><w:Query typeNames="test:data"><f:Filter><f:And><f:PropertyIsEqualTo><f:ValueReference>name</f:ValueReference><f:Literal>A &amp; B</f:Literal></f:PropertyIsEqualTo><f:BBOX><f:ValueReference>geometry</f:ValueReference><g:Envelope srsName="OGC:CRS84"><g:lowerCorner>0 1</g:lowerCorner><g:upperCorner>2 3</g:upperCorner></g:Envelope></f:BBOX></f:And></f:Filter></w:Query></w:GetFeature>"#;
        let (_, attrs) = parse_wfs_xml(xml).unwrap();
        assert_eq!(attrs["eq:name"], "A & B");
        assert_eq!(attrs["bbox"], "0,1,2,3");
        assert_eq!(attrs["resultType"], "hits");
        assert!(
            parse_wfs_xml(&xml.replace("http://www.opengis.net/fes/2.0", "urn:wrong")).is_err()
        );
    }
}

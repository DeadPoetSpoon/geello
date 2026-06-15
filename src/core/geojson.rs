use std::collections::HashMap;

use crate::core::error::GeelloResult;
use crate::core::layer::{
    Crs, LayerId, LayerSource, ProjectionSpec, PropertyValue, RenderedGeometry, RenderedLayer,
};

/// Extract EPSG code from a GeoJSON `crs` foreign member.
///
/// Handles both EPSG and OGC authority URNs:
/// - `urn:ogc:def:crs:EPSG::4326` → 4326
/// - `urn:ogc:def:crs:OGC:1.3:CRS84` → 4326
/// - `EPSG:3857` → 3857
fn extract_epsg_from_crs_json(crs: &serde_json::Value) -> Option<u16> {
    let name = crs.get("properties")?.get("name")?.as_str()?;

    // Short form: "EPSG:4326" or "EPSG::4326"
    for sep in ["EPSG::", "EPSG:"] {
        if let Some(rest) = name.split(sep).nth(1) {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = digits.parse::<u16>() {
                return Some(code);
            }
        }
    }

    // URN form: "urn:ogc:def:crs:{authority}:{...}:{code}"
    // e.g. "urn:ogc:def:crs:OGC:1.3:CRS84"
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() >= 7
        && parts[0] == "urn"
        && parts[1] == "ogc"
        && parts[2] == "def"
        && parts[3] == "crs"
    {
        let authority = parts[4];
        // The last part is the code (skip version in between)
        let code = parts.last()?;

        match authority {
            "OGC" if code.eq_ignore_ascii_case("CRS84") => return Some(4326),
            _ => {
                // Try numeric parse for unknown OGC codes
                if let Ok(n) = code.parse::<u16>() {
                    return Some(n);
                }
            }
        }
    }

    None
}

/// Resolve the coordinate reference system for the layer.
///
/// Priority:
/// 1. Explicit [`ProjectionSpec::Epsg`] set in the [`LayerSource`].
/// 2. `crs` member in GeoJSON `foreign_members` (deprecated but still found in older files).
/// 3. Fallback default: WGS84 (EPSG:4326) per RFC 7946.
fn resolve_crs(
    spec: ProjectionSpec,
    foreign_members: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Crs {
    match spec {
        ProjectionSpec::Epsg(code) => Crs { epsg: code },
        ProjectionSpec::Auto => {
            if let Some(members) = foreign_members {
                if let Some(crs_value) = members.get("crs") {
                    if let Some(epsg) = extract_epsg_from_crs_json(crs_value) {
                        return Crs { epsg };
                    }
                }
            }
            Crs { epsg: 4326 }
        }
    }
}

/// Convert a `geojson::JsonValue` into our internal [`PropertyValue`].
fn convert_property(value: &serde_json::Value) -> PropertyValue {
    match value {
        serde_json::Value::String(s) => PropertyValue::String(s.clone()),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(PropertyValue::Number)
            .unwrap_or(PropertyValue::Null),
        serde_json::Value::Bool(b) => PropertyValue::Bool(*b),
        serde_json::Value::Null => PropertyValue::Null,
        // Objects and arrays are not supported — fall back to Null
        _ => PropertyValue::Null,
    }
}

/// Convert a single GeoJSON Feature into a [`RenderedGeometry`].
fn feature_to_rendered(
    feature: geojson::Feature,
) -> Result<RenderedGeometry, crate::core::error::GeelloError> {
    let geometry = match feature.geometry {
        Some(geom) => geo_types::Geometry::<f64>::try_from(geom).map_err(|e| {
            crate::core::error::GeelloError::GeoJson(format!("Geometry conversion failed: {e}"))
        })?,
        None => geo_types::Geometry::GeometryCollection(geo_types::GeometryCollection::default()),
    };

    let properties: HashMap<String, PropertyValue> = feature
        .properties
        .map(|props| {
            props
                .iter()
                .map(|(k, v)| (k.clone(), convert_property(v)))
                .collect()
        })
        .unwrap_or_default();

    Ok(RenderedGeometry {
        geometry,
        properties,
    })
}

/// Convert a bare GeoJSON Geometry (no Feature wrapper) into a [`RenderedGeometry`].
fn geometry_to_rendered(
    geom: geojson::Geometry,
) -> Result<RenderedGeometry, crate::core::error::GeelloError> {
    let geometry = geo_types::Geometry::<f64>::try_from(geom).map_err(|e| {
        crate::core::error::GeelloError::GeoJson(format!("Geometry conversion failed: {e}"))
    })?;

    Ok(RenderedGeometry {
        geometry,
        properties: HashMap::new(),
    })
}

/// Parse raw GeoJSON string into a [`RenderedLayer`].
///
/// Accepts the three valid GeoJSON root types:
/// - `FeatureCollection` → multiple geometries with properties
/// - `Feature` → single geometry with properties
/// - `Geometry` → single geometry, no properties
pub(crate) fn parse_geojson_str(
    raw: &str,
    layer_id: LayerId,
    source: LayerSource,
) -> GeelloResult<RenderedLayer> {
    let geojson = raw
        .parse::<geojson::GeoJson>()
        .map_err(|e| crate::core::error::GeelloError::GeoJson(e.to_string()))?;

    match geojson {
        geojson::GeoJson::FeatureCollection(fc) => {
            let spec = match &source {
                LayerSource::FileSystem { projection, .. } => *projection,
                #[cfg(feature = "from_http")]
                LayerSource::Http { projection, .. } => *projection,
                #[cfg(feature = "from_samba")]
                LayerSource::Samba { projection, .. } => *projection,
            };
            let crs = resolve_crs(spec, fc.foreign_members.as_ref());

            let geometries: Vec<RenderedGeometry> = fc
                .features
                .into_iter()
                .map(feature_to_rendered)
                .collect::<Result<_, _>>()?;

            Ok(RenderedLayer {
                id: layer_id,
                source,
                crs,
                geometries,
            })
        }
        geojson::GeoJson::Feature(feature) => {
            let spec = ProjectionSpec::Auto; // no CRS on bare Feature
            let crs = resolve_crs(spec, None);
            let geometry = feature_to_rendered(feature)?;
            Ok(RenderedLayer {
                id: layer_id,
                source,
                crs,
                geometries: vec![geometry],
            })
        }
        geojson::GeoJson::Geometry(geom) => {
            let crs = resolve_crs(ProjectionSpec::Auto, None);
            let geometry = geometry_to_rendered(geom)?;
            Ok(RenderedLayer {
                id: layer_id,
                source,
                crs,
                geometries: vec![geometry],
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_epsg_from_crs_json_urn() {
        let crs = json!({
            "type": "name",
            "properties": { "name": "urn:ogc:def:crs:EPSG::4326" }
        });
        assert_eq!(extract_epsg_from_crs_json(&crs), Some(4326));
    }

    #[test]
    fn test_extract_epsg_from_crs_json_short() {
        let crs = json!({
            "type": "name",
            "properties": { "name": "EPSG:3857" }
        });
        assert_eq!(extract_epsg_from_crs_json(&crs), Some(3857));
    }

    #[test]
    fn test_resolve_crs_explicit() {
        let crs = resolve_crs(ProjectionSpec::Epsg(3857), None);
        assert_eq!(crs.epsg, 3857);
    }

    #[test]
    fn test_resolve_crs_auto_fallback() {
        let crs = resolve_crs(ProjectionSpec::Auto, None);
        assert_eq!(crs.epsg, 4326);
    }

    #[test]
    fn test_extract_epsg_from_crs_json_ogc_crs84() {
        let crs = json!({
            "type": "name",
            "properties": { "name": "urn:ogc:def:crs:OGC:1.3:CRS84" }
        });
        assert_eq!(extract_epsg_from_crs_json(&crs), Some(4326));
    }

    #[test]
    fn test_extract_epsg_from_crs_json_ogc_crs84_lowercase() {
        let crs = json!({
            "type": "name",
            "properties": { "name": "urn:ogc:def:crs:OGC::CRS84" }
        });
        // case-insensitive match
        assert_eq!(extract_epsg_from_crs_json(&crs), Some(4326));
    }

    #[test]
    fn test_resolve_crs_auto_from_fc_ogc_crs84() {
        let raw = r#"{
            "type": "FeatureCollection",
            "crs": {
                "type": "name",
                "properties": { "name": "urn:ogc:def:crs:OGC:1.3:CRS84" }
            },
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [0.0, 0.0] }
                }
            ]
        }"#;

        let layer_id = LayerId {
            workspace: "test".into(),
            name: "crs84".into(),
        };
        let source = LayerSource::FileSystem {
            path: "memory".into(),
            projection: ProjectionSpec::Auto,
        };
        let layer = parse_geojson_str(raw, layer_id, source).unwrap();

        assert_eq!(layer.crs.epsg, 4326);
    }

    #[test]
    fn test_resolve_crs_auto_from_fc() {
        let raw = r#"{
            "type": "FeatureCollection",
            "crs": {
                "type": "name",
                "properties": { "name": "urn:ogc:def:crs:EPSG::3857" }
            },
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [0.0, 0.0] }
                }
            ]
        }"#;

        let layer_id = LayerId {
            workspace: "test".into(),
            name: "crs_test".into(),
        };
        let source = LayerSource::FileSystem {
            path: "memory".into(),
            projection: ProjectionSpec::Auto,
        };
        let layer = parse_geojson_str(raw, layer_id, source).unwrap();

        assert_eq!(layer.crs.epsg, 3857);
    }

    #[test]
    fn test_resolve_crs_explicit_overrides_fc() {
        let raw = r#"{
            "type": "FeatureCollection",
            "crs": {
                "type": "name",
                "properties": { "name": "EPSG:4326" }
            },
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [0.0, 0.0] }
                }
            ]
        }"#;

        let layer_id = LayerId {
            workspace: "test".into(),
            name: "override".into(),
        };
        let source = LayerSource::FileSystem {
            path: "memory".into(),
            projection: ProjectionSpec::Epsg(3857),
        };
        let layer = parse_geojson_str(raw, layer_id, source).unwrap();

        // Explicit Epsg(3857) takes priority over file CRS EPSG:4326
        assert_eq!(layer.crs.epsg, 3857);
    }

    #[test]
    fn test_parse_feature_collection() {
        let raw = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [102.0, 0.5] },
                    "properties": { "name": "test", "pop": 100, "active": true, "extra": null }
                }
            ]
        }"#;

        let layer_id = LayerId {
            workspace: "test".into(),
            name: "points".into(),
        };
        let source = LayerSource::FileSystem {
            path: "memory".into(),
            projection: ProjectionSpec::Auto,
        };
        let layer = parse_geojson_str(raw, layer_id, source).unwrap();

        assert_eq!(layer.geometries.len(), 1);
        assert_eq!(layer.crs.epsg, 4326);

        let props = &layer.geometries[0].properties;
        assert_eq!(
            props.get("name"),
            Some(&PropertyValue::String("test".into()))
        );
        assert_eq!(props.get("pop"), Some(&PropertyValue::Number(100.0)));
        assert_eq!(props.get("active"), Some(&PropertyValue::Bool(true)));
        assert_eq!(props.get("extra"), Some(&PropertyValue::Null));
    }

    #[test]
    fn test_parse_feature() {
        let raw = r#"{
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [0.0, 0.0] }
        }"#;

        let layer_id = LayerId {
            workspace: "test".into(),
            name: "single".into(),
        };
        let source = LayerSource::FileSystem {
            path: "memory".into(),
            projection: ProjectionSpec::Auto,
        };
        let layer = parse_geojson_str(raw, layer_id, source).unwrap();

        assert_eq!(layer.geometries.len(), 1);
    }

    #[test]
    fn test_parse_geometry() {
        let raw = r#"{
            "type": "LineString",
            "coordinates": [[0.0, 0.0], [1.0, 1.0]]
        }"#;

        let layer_id = LayerId {
            workspace: "test".into(),
            name: "line".into(),
        };
        let source = LayerSource::FileSystem {
            path: "memory".into(),
            projection: ProjectionSpec::Auto,
        };
        let layer = parse_geojson_str(raw, layer_id, source).unwrap();

        assert_eq!(layer.geometries.len(), 1);
        assert!(layer.geometries[0].properties.is_empty());
    }
}

use geo::Transform;
use geo::proj::Proj;
use geo_types::Geometry;

use crate::core::error::{GeelloError, GeelloResult};
use crate::core::layer::{Crs, RenderedLayer};

/// Supported render-target coordinate reference systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetCrs {
    /// WGS84 (EPSG:4326) — latitude / longitude.
    Epsg4326,
    /// Web Mercator (EPSG:3857) — metre-based flat projection.
    Epsg3857,
}

impl TargetCrs {
    fn epsg_code(self) -> u16 {
        match self {
            TargetCrs::Epsg4326 => 4326,
            TargetCrs::Epsg3857 => 3857,
        }
    }

    fn proj_string(self) -> String {
        format!("EPSG:{}", self.epsg_code())
    }
}

/// Build a [`Proj`] instance that transforms from `source` to `target`.
fn build_proj(source: Crs, target: TargetCrs) -> GeelloResult<Proj> {
    let from = format!("EPSG:{}", source.epsg);
    let to = target.proj_string();
    Proj::new_known_crs(&from, &to, None).map_err(|e| {
        GeelloError::Projection(format!(
            "Failed to create projection EPSG:{} → EPSG:{}: {e}",
            source.epsg,
            target.epsg_code(),
        ))
    })
}

// ---------------------------------------------------------------------------
// Per-geometry-type transformation
// ---------------------------------------------------------------------------

macro_rules! transform_via_proj {
    ($geom:expr, $proj:expr) => {
        *$geom = $geom
            .transformed($proj)
            .map_err(|e| GeelloError::Projection(e.to_string()))?
    };
}

/// Transform **every** geometry in a [`RenderedLayer`] from its source CRS
/// to the given `target` CRS, and update the layer's [`Crs`] accordingly.
///
/// Geometries whose source CRS already matches the target are left unchanged.
///
/// This is the primary entry point — call it once after loading a layer
/// and before building the vello scene.
pub fn transform_layer(layer: &mut RenderedLayer, target: TargetCrs) -> GeelloResult<()> {
    if layer.crs.epsg == target.epsg_code() {
        return Ok(());
    }
    let source = layer.crs;
    let proj = build_proj(source, target)?;
    for geom in layer.geometries.iter_mut() {
        transform_geometry(&mut geom.geometry, &proj)?;
    }
    layer.crs = Crs {
        epsg: target.epsg_code(),
    };
    Ok(())
}

fn transform_geometry(geom: &mut Geometry<f64>, proj: &Proj) -> GeelloResult<()> {
    use geo_types::Geometry::*;
    match geom {
        Point(p) => transform_via_proj!(p, proj),
        Line(p) => transform_via_proj!(p, proj),
        LineString(p) => transform_via_proj!(p, proj),
        Polygon(p) => transform_via_proj!(p, proj),
        MultiPoint(p) => transform_via_proj!(p, proj),
        MultiLineString(p) => transform_via_proj!(p, proj),
        MultiPolygon(p) => transform_via_proj!(p, proj),
        Rect(p) => transform_via_proj!(p, proj),
        Triangle(p) => transform_via_proj!(p, proj),
        GeometryCollection(gc) => {
            for inner in gc.iter_mut() {
                transform_geometry(inner, proj)?;
            }
        }
    }
    Ok(())
}

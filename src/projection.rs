//! Explicit pure-Rust XY projection. No native fallback or network grids.
use crate::{Accounted, CoordinateSpace, Crs, GeelloError, OperationContext, VectorBatch};
use geo::MapCoords;
use geo_types::Coord;
use proj4rs::Proj;

/// A fixed source/target operation. Geographic API coordinates always use degrees XY.
#[derive(Debug, Clone)]
pub struct RustProjection {
    source: Crs,
    target: Crs,
    from: Proj,
    to: Proj,
}

/// Explicit operation choice for server preparation. Selection never falls back.
pub enum Projection {
    Rust(Box<RustProjection>),
    #[cfg(feature = "native-proj")]
    Native(crate::native_projection::NativeProjection),
}
impl Projection {
    pub fn new(backend: Option<&str>, source: Crs, target: Crs) -> Result<Self, GeelloError> {
        match backend.unwrap_or("rust") {
            "rust" => Ok(Self::Rust(Box::new(RustProjection::new(source, target)?))),
            #[cfg(feature = "native-proj")]
            "native" => Ok(Self::Native(
                crate::native_projection::NativeProjection::new(source, target)?,
            )),
            _ => Err(GeelloError::Unsupported(
                "projection backend is not compiled".into(),
            )),
        }
    }
    pub fn source(&self) -> &Crs {
        match self {
            Self::Rust(p) => p.source(),
            #[cfg(feature = "native-proj")]
            Self::Native(p) => p.source(),
        }
    }
    pub fn target(&self) -> &Crs {
        match self {
            Self::Rust(p) => p.target(),
            #[cfg(feature = "native-proj")]
            Self::Native(p) => p.target(),
        }
    }
    pub fn transform_xy(&self, xy: [f64; 2]) -> Result<[f64; 2], GeelloError> {
        match self {
            Self::Rust(p) => p.transform_xy(xy),
            #[cfg(feature = "native-proj")]
            Self::Native(p) => p.transform_xy(xy),
        }
    }
    pub fn project_batch(
        &self,
        batch: &VectorBatch,
        context: &OperationContext,
    ) -> Result<Accounted<VectorBatch>, GeelloError> {
        project_batch(batch, self.source(), self.target(), context, |xy| {
            self.transform_xy(xy)
        })
    }
}

pub fn validate_backend(backend: Option<&str>) -> Result<(), GeelloError> {
    match backend.unwrap_or("rust") {
        "rust" => Ok(()),
        #[cfg(feature = "native-proj")]
        "native" => crate::native_projection::distribution_identity().map(|_| ()),
        _ => Err(GeelloError::Unsupported(
            "projection backend is not compiled".into(),
        )),
    }
}

/// A process fixes one native distribution; its identity partitions render caches.
pub fn runtime_identity() -> Result<&'static str, GeelloError> {
    #[cfg(feature = "native-proj")]
    {
        crate::native_projection::distribution_identity()
    }
    #[cfg(not(feature = "native-proj"))]
    {
        Ok("rust-proj-0.3.0")
    }
}
fn resolve(crs: &Crs) -> Result<Proj, GeelloError> {
    let input = crs.definition();
    let definition = if input == "OGC:CRS84" {
        crs_definitions::EPSG_4326.proj4.to_owned()
    } else if let Some(code) = input.strip_prefix("EPSG:") {
        crs_definitions::from_code(code.parse().map_err(|_| GeelloError::InvalidCrs)?)
            .ok_or(GeelloError::InvalidCrs)?
            .proj4
            .to_owned()
    } else if input.trim_start().starts_with('+') {
        input.to_owned()
    } else {
        proj4wkt::wkt_to_projstring(input).map_err(|e| GeelloError::Projection(e.to_string()))?
    };
    if definition.split_whitespace().any(|v| {
        (v.starts_with("+nadgrids=") && v != "+nadgrids=@null")
            || v.starts_with("+geoidgrids=")
            || v.starts_with("+init=")
    }) {
        return Err(GeelloError::Unsupported(
            "external projection grids/init are not enabled".into(),
        ));
    }
    let definition = definition
        .split_whitespace()
        .filter(|v| *v != "+nadgrids=@null")
        .collect::<Vec<_>>()
        .join(" ");
    let proj =
        Proj::from_proj_string(&definition).map_err(|e| GeelloError::Projection(e.to_string()))?;
    if proj.is_geocent()
        || !proj.is_normalized_axis()
        || proj.is_latlong() != (crs.space() == CoordinateSpace::GeographicXy)
    {
        return Err(GeelloError::Projection(
            "CRS space/XY axes mismatch or non-2D CRS".into(),
        ));
    }
    Ok(proj)
}
impl RustProjection {
    pub fn new(source: Crs, target: Crs) -> Result<Self, GeelloError> {
        let from = resolve(&source)?;
        let to = resolve(&target)?;
        Ok(Self {
            source,
            target,
            from,
            to,
        })
    }
    pub fn source(&self) -> &Crs {
        &self.source
    }
    pub fn target(&self) -> &Crs {
        &self.target
    }
    pub fn transform_xy(&self, xy: [f64; 2]) -> Result<[f64; 2], GeelloError> {
        if xy.iter().any(|v| !v.is_finite()) {
            return Err(GeelloError::Projection("nonfinite coordinate".into()));
        }
        if self.from.is_latlong() && (xy[0].abs() > 180. || xy[1].abs() > 90.) {
            return Err(GeelloError::Projection("geographic domain exceeded".into()));
        }
        let (x, y) = if self.from.is_latlong() {
            (xy[0].to_radians(), xy[1].to_radians())
        } else {
            (xy[0], xy[1])
        };
        let mut point = (x, y, 0.);
        proj4rs::transform::transform(&self.from, &self.to, &mut point).map_err(
            |error| match error {
                proj4rs::errors::Error::CoordTransOutsideProjectionDomain
                | proj4rs::errors::Error::CoordinateOutOfRange
                | proj4rs::errors::Error::LatitudeOutOfRange
                | proj4rs::errors::Error::LatOrLongExceedLimit => {
                    GeelloError::ProjectionDomain(error.to_string())
                }
                _ => GeelloError::Projection(error.to_string()),
            },
        )?;
        let out = if self.to.is_latlong() {
            [point.0.to_degrees(), point.1.to_degrees()]
        } else {
            [point.0, point.1]
        };
        if out.iter().any(|v| !v.is_finite()) {
            return Err(GeelloError::ProjectionDomain(
                "coordinate outside projection domain".into(),
            ));
        }
        Ok(out)
    }
    /// Preserve original properties, identities and Z/M provenance. New XY values
    /// never imply a vertical transformation or mutate the source batch.
    pub fn project_batch(
        &self,
        batch: &VectorBatch,
        context: &OperationContext,
    ) -> Result<Accounted<VectorBatch>, GeelloError> {
        project_batch(batch, self.source(), self.target(), context, |xy| {
            self.transform_xy(xy)
        })
    }
}

fn project_batch(
    batch: &VectorBatch,
    source: &Crs,
    target: &Crs,
    context: &OperationContext,
    transform: impl Fn([f64; 2]) -> Result<[f64; 2], GeelloError>,
) -> Result<Accounted<VectorBatch>, GeelloError> {
    if &batch.crs != source {
        return Err(GeelloError::Projection(
            "operation source differs from batch CRS".into(),
        ));
    }
    let bytes = batch.features.iter().try_fold(256usize, |n, f| {
        n.checked_add(crate::core::feature_clone_bytes(f)?)
            .ok_or(GeelloError::SizeOverflow)
    })?;
    // Transformation temporarily holds the cloned and newly mapped geometry.
    let reservation = context.reserve(bytes.checked_mul(2).ok_or(GeelloError::SizeOverflow)?)?;
    let mut features = Vec::with_capacity(batch.features.len());
    for feature in &batch.features {
        context.checkpoint(1)?;
        let mut output = feature.clone();
        output.geometry = feature
            .geometry
            .as_ref()
            .map(|g| {
                g.try_map_coords(|c: Coord<f64>| {
                    context.checkpoint(1)?;
                    let [x, y] = transform([c.x, c.y])?;
                    Ok::<_, GeelloError>(Coord { x, y })
                })
            })
            .transpose()?;
        features.push(output);
    }
    Ok(Accounted::new(
        VectorBatch {
            crs: target.clone(),
            features,
        },
        vec![reservation],
    ))
}

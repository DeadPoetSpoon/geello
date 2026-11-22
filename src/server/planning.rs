use crate::{config::*, derived::DerivedRaster, projection::Projection, *};

pub(super) fn validate_pixel_ratio(ratio: f64) -> super::Result<()> {
    if !ratio.is_finite() || ratio <= 0. || ratio > 4. {
        return Err(super::ServerError::invalid(
            "geello_pixel_ratio must be finite, positive and at most 4",
        ));
    }
    Ok(())
}

fn scale_length(length: &mut Length, ratio: f64) {
    if let Length::Pixels(value) = length {
        *value *= ratio;
    }
}
fn scale_stroke(stroke: &mut StrokeStyle, ratio: f64) {
    if matches!(stroke.width, Length::Pixels(_)) {
        stroke.dashes.iter_mut().for_each(|value| *value *= ratio);
        stroke.dash_offset *= ratio;
    }
    scale_length(&mut stroke.width, ratio);
}
pub(super) fn scale_composition(composition: &mut MapComposition, ratio: f64) {
    if ratio == 1. {
        return;
    }
    for item in &mut composition.items {
        match item {
            DrawItem::Vector(vector) => {
                for symbol in &mut vector.symbols {
                    match symbol {
                        Symbol::Point(point) => {
                            scale_length(&mut point.size, ratio);
                            if let Some(stroke) = &mut point.stroke {
                                scale_stroke(stroke, ratio);
                            }
                        }
                        Symbol::Line(stroke) | Symbol::PolygonOutline(stroke) => {
                            scale_stroke(stroke, ratio)
                        }
                        Symbol::PolygonFill { .. } => {}
                    }
                }
            }
            DrawItem::Label(label) => {
                scale_length(&mut label.style.size, ratio);
                label.wrap_width = label.wrap_width.map(|width| width * ratio);
                label.repeat_distance *= ratio;
            }
            DrawItem::Image(_) => {}
        }
    }
}

fn fixed_length(expression: &NumericExpression, scale: f64, pixel_ratio: f64) -> Option<f64> {
    if expression.field.is_some() {
        return None;
    }
    let value = expression.value?;
    Some(match expression.unit? {
        Unit::Px => value * pixel_ratio,
        Unit::Map => value * scale,
    })
}

pub(super) fn vector_bounds(
    style: &StyleDefinition,
    source: &Crs,
    view: &View,
    animated_style: bool,
    pixel_ratio: f64,
) -> Option<Bounds> {
    if animated_style {
        return None;
    }
    let (scale_x, scale_y, _, _) = view.scales();
    let scale = (scale_x * scale_y).abs().sqrt();
    let mut padding = 2.;
    for symbol in style.rules.iter().flat_map(|rule| &rule.symbols) {
        let extent = match symbol {
            SymbolDefinition::Point { size, stroke, .. } => {
                let radius = fixed_length(size, scale, pixel_ratio)? * 2.;
                radius
                    + if let Some(stroke) = stroke {
                        fixed_length(&stroke.width, scale, pixel_ratio)? * (stroke.miter_limit + 2.)
                    } else {
                        0.
                    }
            }
            SymbolDefinition::Line {
                width, miter_limit, ..
            } => fixed_length(width, scale, pixel_ratio)? * (miter_limit + 2.),
            SymbolDefinition::Area { .. } => 0.,
            _ => return None,
        };
        padding = f64::max(padding, extent + 2.);
    }
    let bounds = Bounds {
        min_x: view.bounds.min_x - padding / scale_x.abs(),
        max_x: view.bounds.max_x + padding / scale_x.abs(),
        min_y: view.bounds.min_y - padding / scale_y.abs(),
        max_y: view.bounds.max_y + padding / scale_y.abs(),
    };
    if bounds.validate().is_err() {
        return None;
    }
    if source == &view.crs {
        return Some(bounds);
    }
    let pair = (view.crs.definition(), source.definition());
    if !matches!(
        pair,
        ("OGC:CRS84", "EPSG:3857") | ("EPSG:3857", "OGC:CRS84")
    ) {
        return None;
    }
    if pair.0 == "OGC:CRS84"
        && (bounds.min_x < -180.
            || bounds.max_x > 180.
            || bounds.min_y < -85.0511287798066
            || bounds.max_y > 85.0511287798066)
    {
        return None;
    }
    if pair.0 == "EPSG:3857"
        && [bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y]
            .iter()
            .any(|value| value.abs() > 20037508.342789244)
    {
        return None;
    }
    let projection = Projection::new(None, view.crs.clone(), source.clone()).ok()?;
    let minimum = projection.transform_xy([bounds.min_x, bounds.min_y]).ok()?;
    let maximum = projection.transform_xy([bounds.max_x, bounds.max_y]).ok()?;
    let result = Bounds {
        min_x: minimum[0],
        min_y: minimum[1],
        max_x: maximum[0],
        max_y: maximum[1],
    };
    result.validate().ok().map(|()| result)
}

fn smallest_scale(matrix: [f64; 4]) -> f64 {
    let trace = matrix.iter().map(|value| value * value).sum::<f64>();
    let determinant = (matrix[0] * matrix[3] - matrix[1] * matrix[2]).abs();
    let largest = ((trace
        + (trace * trace - 4. * determinant * determinant)
            .max(0.)
            .sqrt())
        / 2.)
        .sqrt();
    determinant / largest
}

pub(super) fn projected_raster_level(
    store: &DerivedRaster,
    view: &View,
    inverse: &Projection,
    backend: Option<&str>,
    ctx: &OperationContext,
) -> Result<usize, GeelloError> {
    let grid = store.grid();
    let Ok(forward) = Projection::new(backend, grid.crs.clone(), view.crs.clone()) else {
        return Ok(0);
    };
    let (width, height, _) = store
        .level_dimensions(0)
        .ok_or_else(|| GeelloError::InvalidRaster("missing base level".into()))?;
    let step_x = (view.bounds.max_x - view.bounds.min_x) / view.width as f64;
    let step_y = (view.bounds.max_y - view.bounds.min_y) / view.height as f64;
    let epsilon_x = step_x.min(if view.crs.space() == CoordinateSpace::GeographicXy {
        0.0001
    } else {
        1.
    });
    let epsilon_y = step_y.min(if view.crs.space() == CoordinateSpace::GeographicXy {
        0.0001
    } else {
        1.
    });
    let mut minimum = f64::INFINITY;
    let mut maximum = 0f64;
    let [origin_x, basis_x, shear_x, origin_y, shear_y, basis_y] = grid.affine;
    let determinant = basis_x * basis_y - shear_x * shear_y;
    let native_pixel = |point: [f64; 2]| {
        [
            (basis_y * (point[0] - origin_x) - shear_x * (point[1] - origin_y)) / determinant,
            (-shear_y * (point[0] - origin_x) + basis_x * (point[1] - origin_y)) / determinant,
        ]
    };
    let mut sample = |point: [f64; 2]| -> Result<(), GeelloError> {
        ctx.checkpoint(1)?;
        if point[0] < view.bounds.min_x
            || point[0] > view.bounds.max_x
            || point[1] < view.bounds.min_y
            || point[1] > view.bounds.max_y
        {
            return Ok(());
        }
        let mapped: Result<Option<f64>, GeelloError> = (|| {
            let center = native_pixel(inverse.transform_xy(point)?);
            if center[0] < 0.
                || center[0] > width as f64
                || center[1] < 0.
                || center[1] > height as f64
            {
                return Ok(None);
            }
            let delta_x = if point[0] + epsilon_x > view.bounds.max_x {
                -epsilon_x
            } else {
                epsilon_x
            };
            let delta_y = if point[1] + epsilon_y > view.bounds.max_y {
                -epsilon_y
            } else {
                epsilon_y
            };
            let horizontal = native_pixel(inverse.transform_xy([point[0] + delta_x, point[1]])?);
            let vertical = native_pixel(inverse.transform_xy([point[0], point[1] + delta_y])?);
            Ok(Some(smallest_scale([
                (horizontal[0] - center[0]) * step_x / delta_x,
                (vertical[0] - center[0]) * step_y / delta_y,
                (horizontal[1] - center[1]) * step_x / delta_x,
                (vertical[1] - center[1]) * step_y / delta_y,
            ])))
        })();
        match mapped {
            Ok(Some(scale)) if scale.is_finite() && scale > 0. => {
                minimum = minimum.min(scale);
                maximum = maximum.max(scale);
            }
            Ok(_) | Err(_) => {}
        }
        Ok(())
    };
    for row in 0..=8 {
        for column in 0..=8 {
            let fraction_x = column as f64 / 8.;
            let fraction_y = row as f64 / 8.;
            sample([
                view.bounds.min_x + fraction_x * (view.bounds.max_x - view.bounds.min_x),
                view.bounds.min_y + fraction_y * (view.bounds.max_y - view.bounds.min_y),
            ])?;
            let native_x = fraction_x * width as f64;
            let native_y = fraction_y * height as f64;
            if let Ok(point) = forward.transform_xy([
                origin_x + basis_x * native_x + shear_x * native_y,
                origin_y + shear_y * native_x + basis_y * native_y,
            ]) {
                sample(point)?;
            }
        }
    }
    Ok(if minimum.is_finite() && maximum <= minimum * 4. {
        store.select_level(minimum * 0.5)
    } else {
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn view() -> View {
        View {
            crs: Crs::new("OGC:CRS84", CoordinateSpace::GeographicXy).unwrap(),
            bounds: Bounds {
                min_x: 0.,
                min_y: 0.,
                max_x: 10.,
                max_y: 10.,
            },
            width: 100,
            height: 100,
            aspect: AspectPolicy::Stretch,
        }
    }

    #[test]
    fn vector_selection_buffers_strokes_and_falls_back_for_unbounded_styles() {
        let fixed = "schema_version=1\nid='test:style'\n[[rules]]\nid='rule'\n[[rules.symbols]]\nkind='line'\nid='line'\nwidth={value=3,unit='px'}\n";
        let style = parse_style(fixed).unwrap();
        let view = view();
        let bounds = vector_bounds(&style, &view.crs, &view, false, 1.).unwrap();
        assert!(bounds.min_x < -1. && bounds.max_x > 11.);
        assert!(vector_bounds(&style, &view.crs, &view, true, 1.).is_none());
        let dynamic = parse_style(&fixed.replace("value=3", "field='/width',fallback=3")).unwrap();
        assert!(vector_bounds(&dynamic, &view.crs, &view, false, 1.).is_none());
        let projected = Crs::new("EPSG:3857", CoordinateSpace::Planar).unwrap();
        let projected_bounds = vector_bounds(&style, &projected, &view, false, 1.).unwrap();
        assert!(projected_bounds.min_x < 0. && projected_bounds.max_x > 1_000_000.);
        let map_units = parse_style(&fixed.replace("unit='px'", "unit='map'")).unwrap();
        assert_eq!(
            vector_bounds(&map_units, &view.crs, &view, false, 0.25),
            vector_bounds(&map_units, &view.crs, &view, false, 1.)
        );
        assert!(
            vector_bounds(&style, &view.crs, &view, false, 2.)
                .unwrap()
                .min_x
                < bounds.min_x
        );
    }

    #[test]
    fn hidpi_scales_only_pixel_space_lengths_and_dash_state() {
        let mut composition = MapComposition {
            items: vec![DrawItem::Vector(VectorItem {
                batches: vec![],
                symbols: vec![
                    Symbol::Line(StrokeStyle {
                        width: Length::Pixels(2.),
                        dashes: vec![3., 5.],
                        dash_offset: 1.,
                        ..StrokeStyle::default()
                    }),
                    Symbol::Point(PointStyle {
                        size: Length::MapUnits(10.),
                        ..PointStyle::default()
                    }),
                ],
            })],
            background: None,
        };
        scale_composition(&mut composition, 2.);
        let DrawItem::Vector(vector) = &composition.items[0] else {
            panic!()
        };
        let Symbol::Line(stroke) = &vector.symbols[0] else {
            panic!()
        };
        assert_eq!(stroke.width, Length::Pixels(4.));
        assert_eq!(stroke.dashes, [6., 10.]);
        assert_eq!(stroke.dash_offset, 2.);
        let Symbol::Point(point) = &vector.symbols[1] else {
            panic!()
        };
        assert_eq!(point.size, Length::MapUnits(10.));
        for invalid in [0., -1., f64::NAN, f64::INFINITY, 5.] {
            assert!(validate_pixel_ratio(invalid).is_err());
        }
    }

    #[test]
    fn projected_rasters_use_compatible_overviews_only_when_reducing() {
        let config = super::super::ServerConfig::load(std::path::Path::new(
            "examples/server.experimental.toml",
        ))
        .unwrap();
        let ctx = config.profile.context();
        let directory = tempfile::tempdir().unwrap();
        let grid = RasterGrid {
            crs: Crs::new("EPSG:32618", CoordinateSpace::Planar).unwrap(),
            affine: [500_000., 10., 0., 4_500_000., 0., -10.],
            interpretation: PixelInterpretation::Area,
        };
        let store = DerivedRaster::build_base(
            directory.path(),
            "overview-selection",
            64,
            64,
            &grid,
            16,
            config.profile.store_limits(),
            &ctx,
            |window, _| {
                Ok(RasterBlock {
                    width: window.width,
                    height: window.height,
                    grid: RasterGrid {
                        affine: [
                            500_000. + window.column as f64 * 10.,
                            10.,
                            0.,
                            4_500_000. - window.row as f64 * 10.,
                            0.,
                            -10.,
                        ],
                        ..grid.clone()
                    },
                    bands: vec![RasterBand {
                        samples: RasterSamples::U16(Arc::from(vec![
                            100;
                            (window.width * window.height)
                                as usize
                        ])),
                        no_data: None,
                        validity: None,
                        scale: 1.,
                        offset: 0.,
                    }],
                    alpha: None,
                    alpha_association: AlphaAssociation::Straight,
                })
            },
        )
        .unwrap()
        .build_overviews(directory.path(), false, &ctx)
        .unwrap();
        let target = Crs::new("EPSG:3857", CoordinateSpace::Planar).unwrap();
        let forward = Projection::new(None, grid.crs.clone(), target.clone()).unwrap();
        let lower = forward.transform_xy([500_000., 4_499_360.]).unwrap();
        let upper = forward.transform_xy([500_640., 4_500_000.]).unwrap();
        let mut view = View {
            crs: target.clone(),
            bounds: Bounds {
                min_x: lower[0] - 1.,
                min_y: lower[1] - 1.,
                max_x: upper[0] + 1.,
                max_y: upper[1] + 1.,
            },
            width: 4,
            height: 4,
            aspect: AspectPolicy::Stretch,
        };
        let inverse = Projection::new(None, target, grid.crs.clone()).unwrap();
        assert!(projected_raster_level(&store, &view, &inverse, None, &ctx).unwrap() > 0);
        view.width = 1024;
        view.height = 1024;
        assert_eq!(
            projected_raster_level(&store, &view, &inverse, None, &ctx).unwrap(),
            0
        );
    }
}

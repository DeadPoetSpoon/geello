use geo::algorithm::BoundingRect;
use geo_types::{
    CoordNum, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use std::collections::HashMap;
use vello::kurbo::{BezPath, PathEl};

use crate::core::layer::PropertyValue;
use crate::core::style::{BboxFilter, PropertyCondition, PropertyOp, ResolvedGeometryFilter};

// ---------------------------------------------------------------------------
// Point → Circle BezPath
// ---------------------------------------------------------------------------

/// kappa = (4/3) * (√2 − 1) — magic constant for approximating a circle
/// with four cubic Bézier segments with 0.027253 % error.
const CIRCLE_KAPPA: f64 = 0.552_284_749_8;

/// Approximate a circle of `radius` centred at `(cx, cy)` as a closed [`BezPath`].
pub fn circle_bezier(cx: f64, cy: f64, radius: f64) -> BezPath {
    let r = radius.max(0.0);
    let k = r * CIRCLE_KAPPA;
    BezPath::from_vec(vec![
        PathEl::MoveTo((cx + r, cy).into()),
        PathEl::CurveTo(
            (cx + r, cy + k).into(),
            (cx + k, cy + r).into(),
            (cx, cy + r).into(),
        ),
        PathEl::CurveTo(
            (cx - k, cy + r).into(),
            (cx - r, cy + k).into(),
            (cx - r, cy).into(),
        ),
        PathEl::CurveTo(
            (cx - r, cy - k).into(),
            (cx - k, cy - r).into(),
            (cx, cy - r).into(),
        ),
        PathEl::CurveTo(
            (cx + k, cy - r).into(),
            (cx + r, cy - k).into(),
            (cx + r, cy).into(),
        ),
        PathEl::ClosePath,
    ])
}

// ---------------------------------------------------------------------------
// LineString → BezPath
// ---------------------------------------------------------------------------

/// Convert a [`LineString`] to an open [`BezPath`] (no `ClosePath`).
pub fn linestring_to_bezpath<T: CoordNum + Into<f64>>(ls: &LineString<T>) -> BezPath {
    let mut path = BezPath::new();
    let mut coords = ls.points();
    if let Some(first) = coords.next() {
        path.move_to(to_kurbo_point(first));
        for p in coords {
            path.line_to(to_kurbo_point(p));
        }
    }
    path
}

/// Convert a closed ring (exterior or interior of a polygon) to a [`BezPath`]
/// with a final `ClosePath`.
fn ring_to_bezpath<T: CoordNum + Into<f64>>(ls: &LineString<T>) -> BezPath {
    let mut path = linestring_to_bezpath(ls);
    path.close_path();
    path
}

// ---------------------------------------------------------------------------
// Polygon → BezPath
// ---------------------------------------------------------------------------

/// Convert a [`Polygon`] to a [`BezPath`].
///
/// The exterior ring is drawn first, then each interior ring (hole) is
/// added with opposite winding so that vello's fill rule correctly
/// punches out the holes.
pub fn polygon_to_bezpath<T: CoordNum + Into<f64>>(poly: &Polygon<T>) -> BezPath {
    let mut path = ring_to_bezpath(poly.exterior());
    for interior in poly.interiors() {
        let hole = ring_to_bezpath(interior);
        // Reverse the hole path so it winds opposite to the exterior.
        let reversed: Vec<PathEl> = hole
            .elements()
            .iter()
            .rev()
            .map(|el| match el {
                PathEl::MoveTo(p) => PathEl::LineTo(*p),
                PathEl::LineTo(p) => PathEl::LineTo(*p),
                PathEl::CurveTo(p1, p2, p) => PathEl::CurveTo(*p2, *p1, *p),
                PathEl::ClosePath => PathEl::MoveTo(
                    hole.elements()
                        .first()
                        .map(|e| e.end_point())
                        .flatten()
                        .unwrap_or_default(),
                ),
                PathEl::QuadTo(p1, p) => PathEl::QuadTo(*p1, *p),
            })
            .collect();
        for el in reversed {
            path.push(el);
        }
        path.close_path();
    }
    path
}

// ---------------------------------------------------------------------------
// Multi-geometry converters
// ---------------------------------------------------------------------------

pub fn multipoint_to_bezpath<T: CoordNum + Into<f64>>(mp: &MultiPoint<T>, radius: f64) -> BezPath {
    let mut path = BezPath::new();
    for p in &mp.0 {
        let circle = circle_bezier(p.x().into(), p.y().into(), radius);
        extend_path(&mut path, &circle);
    }
    path
}

pub fn multilinestring_to_bezpath<T: CoordNum + Into<f64>>(mls: &MultiLineString<T>) -> BezPath {
    let mut path = BezPath::new();
    for ls in &mls.0 {
        extend_path(&mut path, &linestring_to_bezpath(ls));
    }
    path
}

pub fn multipolygon_to_bezpath<T: CoordNum + Into<f64>>(mp: &MultiPolygon<T>) -> BezPath {
    let mut path = BezPath::new();
    for poly in &mp.0 {
        extend_path(&mut path, &polygon_to_bezpath(poly));
    }
    path
}

// ---------------------------------------------------------------------------
// Generic geometry → BezPath dispatcher
// ---------------------------------------------------------------------------

/// Convert any [`Geometry`] variant into a [`BezPath`].
///
/// `point_radius` is used for `Point` / `MultiPoint` geometries.
/// Other geometry types ignore it.
pub fn geometry_to_bezpath<T: CoordNum + Into<f64>>(
    geom: &Geometry<T>,
    point_radius: f64,
) -> BezPath {
    match geom {
        Geometry::Point(p) => circle_bezier(p.x().into(), p.y().into(), point_radius),
        Geometry::Line(line) => {
            let mut path = BezPath::new();
            path.move_to((line.start.x.into(), line.start.y.into()));
            path.line_to((line.end.x.into(), line.end.y.into()));
            path
        }
        Geometry::LineString(ls) => linestring_to_bezpath(ls),
        Geometry::Polygon(poly) => polygon_to_bezpath(poly),
        Geometry::MultiPoint(mp) => multipoint_to_bezpath(mp, point_radius),
        Geometry::MultiLineString(mls) => multilinestring_to_bezpath(mls),
        Geometry::MultiPolygon(mp) => multipolygon_to_bezpath(mp),
        Geometry::Rect(r) => {
            let min = r.min();
            let max = r.max();
            let mut path = BezPath::new();
            path.move_to((min.x.into(), min.y.into()));
            path.line_to((max.x.into(), min.y.into()));
            path.line_to((max.x.into(), max.y.into()));
            path.line_to((min.x.into(), max.y.into()));
            path.close_path();
            path
        }
        Geometry::Triangle(t) => {
            let v1 = t.v1();
            let v2 = t.v2();
            let v3 = t.v3();
            let mut path = BezPath::new();
            path.move_to((v1.x.into(), v1.y.into()));
            path.line_to((v2.x.into(), v2.y.into()));
            path.line_to((v3.x.into(), v3.y.into()));
            path.close_path();
            path
        }
        Geometry::GeometryCollection(gc) => {
            let mut path = BezPath::new();
            for inner in &gc.0 {
                extend_path(&mut path, &geometry_to_bezpath(inner, point_radius));
            }
            path
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_kurbo_point<T: CoordNum + Into<f64>>(p: Point<T>) -> vello::kurbo::Point {
    vello::kurbo::Point::new(p.x().into(), p.y().into())
}

fn extend_path(target: &mut BezPath, source: &BezPath) {
    for el in source.elements() {
        target.push(*el);
    }
}

// ---------------------------------------------------------------------------
// GeometryFilter
// ---------------------------------------------------------------------------

/// Check whether a geometry passes the given [`ResolvedGeometryFilter`].
///
/// Both `prop` and `bbox` conditions are AND-ed — if both are set, the
/// geometry must satisfy both.  If the filter is empty (both `None`), every
/// geometry passes.
pub fn geometry_passes_filter(
    geom: &Geometry<f64>,
    properties: &HashMap<String, PropertyValue>,
    filter: &ResolvedGeometryFilter,
) -> bool {
    // ---- property condition ----
    if let Some(ref cond) = filter.prop {
        if !check_property(properties, cond) {
            return false;
        }
    }

    // ---- bbox condition ----
    if let Some(ref bbox) = filter.bbox {
        if !check_bbox(geom, bbox) {
            return false;
        }
    }

    true
}

fn check_property(props: &HashMap<String, PropertyValue>, cond: &PropertyCondition) -> bool {
    let Some(val) = props.get(&cond.name) else {
        return false;
    };
    match cond.op {
        PropertyOp::Eq => prop_eq(val, &cond.value),
    }
}

fn prop_eq(a: &PropertyValue, b: &crate::core::style::PropertyValue) -> bool {
    match (a, b) {
        (PropertyValue::String(s1), crate::core::style::PropertyValue::String(s2)) => s1 == s2,
        (PropertyValue::Number(n1), crate::core::style::PropertyValue::Number(n2)) => {
            (n1 - n2).abs() < f64::EPSILON
        }
        (PropertyValue::Bool(b1), crate::core::style::PropertyValue::Bool(b2)) => b1 == b2,
        _ => false,
    }
}

fn check_bbox(geom: &Geometry<f64>, bbox: &BboxFilter) -> bool {
    let Some(geom_bbox) = geom.bounding_rect() else {
        return false;
    };
    let min = geom_bbox.min();
    let max = geom_bbox.max();
    // Axis-aligned rectangle intersection
    min.x <= bbox.max_x && max.x >= bbox.min_x && min.y <= bbox.max_y && max.y >= bbox.min_y
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::{Geometry, point, polygon};

    fn make_filter(
        prop: Option<PropertyCondition>,
        bbox: Option<BboxFilter>,
    ) -> ResolvedGeometryFilter {
        ResolvedGeometryFilter { prop, bbox }
    }

    #[test]
    fn test_empty_filter_passes() {
        let geom = Geometry::Point(point! { x: 0.0, y: 0.0 });
        let props = HashMap::new();
        assert!(geometry_passes_filter(
            &geom,
            &props,
            &make_filter(None, None)
        ));
    }

    #[test]
    fn test_prop_filter_match() {
        let geom = Geometry::Point(point! { x: 0.0, y: 0.0 });
        let mut props = HashMap::new();
        props.insert("class".into(), PropertyValue::String("capital".into()));
        let filter = make_filter(
            Some(PropertyCondition {
                name: "class".into(),
                op: PropertyOp::Eq,
                value: crate::core::style::PropertyValue::String("capital".into()),
            }),
            None,
        );
        assert!(geometry_passes_filter(&geom, &props, &filter));
    }

    #[test]
    fn test_prop_filter_mismatch() {
        let geom = Geometry::Point(point! { x: 0.0, y: 0.0 });
        let mut props = HashMap::new();
        props.insert("class".into(), PropertyValue::String("town".into()));
        let filter = make_filter(
            Some(PropertyCondition {
                name: "class".into(),
                op: PropertyOp::Eq,
                value: crate::core::style::PropertyValue::String("capital".into()),
            }),
            None,
        );
        assert!(!geometry_passes_filter(&geom, &props, &filter));
    }

    #[test]
    fn test_bbox_intersects() {
        let geom = Geometry::Polygon(polygon![
            (x: 0.0, y: 0.0),
            (x: 10.0, y: 0.0),
            (x: 10.0, y: 10.0),
            (x: 0.0, y: 10.0),
        ]);
        let filter = make_filter(
            None,
            Some(BboxFilter {
                min_x: 5.0,
                min_y: 5.0,
                max_x: 15.0,
                max_y: 15.0,
            }),
        );
        assert!(geometry_passes_filter(&geom, &HashMap::new(), &filter));
    }

    #[test]
    fn test_bbox_no_intersect() {
        let geom = Geometry::Point(point! { x: 0.0, y: 0.0 });
        let filter = make_filter(
            None,
            Some(BboxFilter {
                min_x: 100.0,
                min_y: 100.0,
                max_x: 200.0,
                max_y: 200.0,
            }),
        );
        assert!(!geometry_passes_filter(&geom, &HashMap::new(), &filter));
    }
}

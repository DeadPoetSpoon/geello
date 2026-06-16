use std::collections::HashMap;

use geo::algorithm::BoundingRect;
use geo_types::Geometry;

use crate::core::layer::PropertyValue;
use crate::core::style::{BboxFilter, PropertyCondition, PropertyOp, ResolvedGeometryFilter};

pub fn geometry_passes_filter(
    geom: &Geometry<f64>,
    properties: &HashMap<String, PropertyValue>,
    filter: &ResolvedGeometryFilter,
) -> bool {
    if let Some(ref cond) = filter.prop {
        if !check_property(properties, cond) {
            return false;
        }
    }
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
    min.x <= bbox.max_x && max.x >= bbox.min_x && min.y <= bbox.max_y && max.y >= bbox.min_y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::style::ResolvedGeometryFilter;
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

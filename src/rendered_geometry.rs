use geo::{
    BooleanOps, BoundingRect, Centroid, Contains, ConvexHull, CoordsIter, Geometry, Intersects,
    LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect,
};
use vello::{Scene, kurbo::Affine};

use crate::{TileProj, renderer::GeometryRenderer};
pub struct RenderedGeometry {
    inner_geom: Geometry,
    center_point: Option<Point>,
    has_calc_center_point: bool,
    lines: Option<MultiLineString>,
    has_calc_lines: bool,
    areas: Option<MultiPolygon>,
    has_calc_areas: bool,
    render_rect: Option<Rect>,
}

impl RenderedGeometry {
    pub fn new(mut inner_geom: Geometry, proj: &Option<TileProj>) -> Self {
        if let Some(proj) = proj {
            crate::utils::transform(&mut inner_geom, proj);
        }
        RenderedGeometry {
            inner_geom,
            center_point: None,
            has_calc_center_point: false,
            lines: None,
            has_calc_lines: false,
            areas: None,
            has_calc_areas: false,
            render_rect: None,
        }
    }

    // Methods for rendering geometry
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        transform: Affine,
        renderers: &Vec<GeometryRenderer>,
    ) {
        // Implementation for rendering geometry
        let affine = match self.render_rect {
            Some(rect) => Affine::translate((-rect.min().x, -rect.max().y))
                .then_scale_non_uniform(1f64, -1f64),
            None => Affine::IDENTITY,
        };
        for ele in renderers {
            ele.draw(scene, transform * affine, self);
        }
    }
    pub fn with_rect(&mut self, rect: Option<Rect>) -> &mut Self {
        if rect.is_some() {
            self.render_rect = rect;
            self.has_calc_center_point = false;
        }
        self
    }
    pub fn lines(&mut self) -> Option<&MultiLineString> {
        if self.has_calc_lines {
            return self.lines.as_ref();
        }
        let lines = RenderedGeometry::get_lines_from_geom(&self.inner_geom);
        self.lines = if lines.len() > 0 {
            let multi = MultiLineString::new(lines);
            Some(multi)
        } else {
            None
        };
        self.has_calc_lines = true;
        self.lines.as_ref()
    }
    pub fn areas(&mut self) -> Option<&MultiPolygon> {
        if self.has_calc_areas {
            return self.areas.as_ref();
        }
        let areas = RenderedGeometry::get_areas_from_geom(&self.inner_geom);
        self.areas = if areas.len() > 0 {
            let multi = MultiPolygon::new(areas);
            Some(multi)
        } else {
            None
        };
        self.has_calc_areas = true;
        self.areas.as_ref()
    }
    fn get_lines_from_geom(geom: &Geometry) -> Vec<LineString> {
        let mut lines = Vec::new();
        match geom {
            Geometry::Point(_) => {}
            Geometry::Line(line) => {
                let line_string = LineString::from_iter(line.coords_iter());
                lines.push(line_string);
            }
            Geometry::LineString(line_string) => {
                lines.push(line_string.clone());
            }
            Geometry::Polygon(polygon) => {
                lines.push(polygon.exterior().clone());
                for ele in polygon.interiors() {
                    lines.push(ele.clone());
                }
            }
            Geometry::MultiPoint(multi_point) => {
                let line_string = LineString::from_iter(multi_point.coords_iter());
                lines.push(line_string);
            }
            Geometry::MultiLineString(multi_line_string) => {
                for line_string in multi_line_string {
                    lines.push(line_string.clone());
                }
            }
            Geometry::MultiPolygon(multi_polygon) => {
                for polygon in multi_polygon {
                    lines.push(polygon.exterior().clone());
                    for ele in polygon.interiors() {
                        lines.push(ele.clone());
                    }
                }
            }
            Geometry::GeometryCollection(geometry_collection) => {
                for ele in geometry_collection {
                    let mut ele_lines = RenderedGeometry::get_lines_from_geom(ele);
                    lines.append(&mut ele_lines);
                }
            }
            Geometry::Rect(rect) => {
                let mut line_string = LineString::from_iter(rect.coords_iter());
                line_string.close();
                lines.push(line_string);
            }
            Geometry::Triangle(triangle) => {
                let mut line_string = LineString::from_iter(triangle.coords_iter());
                line_string.close();
                lines.push(line_string);
            }
        };
        lines
    }
    fn get_areas_from_geom(geom: &Geometry) -> Vec<Polygon> {
        let mut polygons = Vec::new();
        match geom {
            Geometry::Point(_) => {}
            Geometry::Line(line) => {
                polygons.push(line.bounding_rect().to_polygon());
            }
            Geometry::LineString(line_string) => {
                polygons.push(line_string.convex_hull());
            }
            Geometry::Polygon(polygon) => {
                polygons.push(polygon.clone());
            }
            Geometry::MultiPoint(multi_point) => {
                polygons.push(multi_point.convex_hull());
            }
            Geometry::MultiLineString(multi_line_string) => {
                polygons.push(multi_line_string.convex_hull());
            }
            Geometry::MultiPolygon(multi_polygon) => {
                for polygon in multi_polygon {
                    polygons.push(polygon.clone());
                }
            }
            Geometry::GeometryCollection(geometry_collection) => {
                for ele in geometry_collection {
                    let mut ele_lines = RenderedGeometry::get_areas_from_geom(ele);
                    polygons.append(&mut ele_lines);
                }
            }
            Geometry::Rect(rect) => {
                polygons.push(rect.to_polygon());
            }
            Geometry::Triangle(triangle) => {
                polygons.push(triangle.to_polygon());
            }
        };
        polygons
    }
    pub fn center_point(&mut self) -> Option<&Point> {
        if self.has_calc_center_point {
            self.center_point.as_ref()
        } else {
            let center = match self.render_rect {
                Some(rect) => match &self.inner_geom {
                    Geometry::Point(point) => Some(point.clone()),
                    Geometry::Line(line) => {
                        if rect.intersects(&line.start) {
                            Some(line.start_point())
                        } else {
                            if rect.intersects(&line.end) {
                                Some(line.end_point())
                            } else {
                                None
                            }
                        }
                    }
                    Geometry::LineString(line_string) => {
                        let multi_line = MultiLineString::new(vec![line_string.clone()]);
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.clip(&multi_line, false);
                        intersection.centroid()
                    }
                    Geometry::Polygon(polygon) => {
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.intersection(polygon);
                        intersection.centroid()
                    }
                    Geometry::MultiPoint(multi_point) => {
                        let mut inter_point = Vec::new();
                        for point in multi_point {
                            if rect.contains(point) {
                                inter_point.push(point.clone());
                            }
                        }
                        let multi_point = MultiPoint::new(inter_point);
                        multi_point.centroid()
                    }
                    Geometry::MultiLineString(multi_line_string) => {
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.clip(&multi_line_string, false);
                        intersection.centroid()
                    }
                    Geometry::MultiPolygon(multi_polygon) => {
                        let rect_polygon = rect.to_polygon();
                        let intersection = rect_polygon.intersection(multi_polygon);
                        intersection.centroid()
                    }
                    Geometry::GeometryCollection(geometry_collection) => {
                        let mut inter_point = Vec::new();
                        for geom in geometry_collection {
                            let point = geom.centroid().unwrap_or_default();
                            if rect.contains(&point) {
                                inter_point.push(point);
                            }
                        }
                        let multi_point = MultiPoint::new(inter_point);
                        multi_point.centroid()
                    }
                    Geometry::Rect(o_rect) => {
                        let rect_polygon = rect.to_polygon();
                        let o_rect_polygon = o_rect.to_polygon();
                        let intersection = rect_polygon.intersection(&o_rect_polygon);
                        intersection.centroid()
                    }
                    Geometry::Triangle(triangle) => {
                        let rect_polygon = rect.to_polygon();
                        let triangle_polygon = triangle.to_polygon();
                        let intersection = rect_polygon.intersection(&triangle_polygon);
                        intersection.centroid()
                    }
                },
                None => self.inner_geom.centroid(),
            };
            self.center_point = center;
            self.has_calc_center_point = true;
            self.center_point.as_ref()
        }
    }
}

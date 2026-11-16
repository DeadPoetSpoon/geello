//! Fixed phase-one grids. Origin is top-left, rows grow downward, no wrapping.
use super::*;
use crate::{AspectPolicy, Bounds, View};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TileGrid {
    WebMercatorQuad,
    WorldCrs84Quad,
}
impl TileGrid {
    pub(crate) fn parse(id: &str) -> Result<Self> {
        match id {
            "WebMercatorQuad" => Ok(Self::WebMercatorQuad),
            "WorldCRS84Quad" => Ok(Self::WorldCrs84Quad),
            _ => Err(ServerError::new(
                404,
                "tile-matrix-set",
                "Tile matrix set is not supported",
            )),
        }
    }
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::WebMercatorQuad => "WebMercatorQuad",
            Self::WorldCrs84Quad => "WorldCRS84Quad",
        }
    }
    pub(crate) fn crs(self) -> &'static str {
        match self {
            Self::WebMercatorQuad => "EPSG:3857",
            Self::WorldCrs84Quad => "OGC:CRS84",
        }
    }
    pub(crate) fn crs_uri(self) -> &'static str {
        match self {
            Self::WebMercatorQuad => "http://www.opengis.net/def/crs/EPSG/0/3857",
            Self::WorldCrs84Quad => "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
        }
    }
    pub(crate) fn dimensions(self, z: u32) -> Result<[u32; 2]> {
        if z > 30 {
            return Err(ServerError::invalid("Tile zoom exceeds supported range"));
        }
        let rows = 1u32 << z;
        Ok([
            rows * if self == Self::WorldCrs84Quad { 2 } else { 1 },
            rows,
        ])
    }
    pub(crate) fn origin(self) -> [f64; 2] {
        match self {
            Self::WebMercatorQuad => [-20037508.342789244, 20037508.342789244],
            Self::WorldCrs84Quad => [-180., 90.],
        }
    }
    pub(crate) fn cell_size(self, z: u32) -> Result<f64> {
        let width = self.dimensions(z)?[0] as f64;
        Ok(match self {
            Self::WebMercatorQuad => 40075016.68557849,
            Self::WorldCrs84Quad => 360.,
        } / width
            / 256.)
    }
    pub(crate) fn layout(
        self,
        z: u32,
        x: u32,
        y: u32,
        p: &ServerProfile,
    ) -> Result<(View, [u32; 4])> {
        let [columns, rows] = self.dimensions(z)?;
        if x >= columns || y >= rows {
            return Err(ServerError::invalid(
                "Tile is outside the matrix; wrapping is disabled",
            ));
        }
        let gw = p.metatile_size.min(columns);
        let gh = p.metatile_size.min(rows);
        let gx = x / gw * gw;
        let gy = y / gh * gh;
        let size = self.cell_size(z)? * 256.;
        let [left, top] = self.origin();
        // Clip padding at the finite matrix boundary. Geographic views must
        // remain inside CRS84 and neither grid wraps into the opposite edge.
        let padding =
            |tiles: u32| (u64::from(tiles) * 256).min(u64::from(p.metatile_buffer)) as u32;
        let left_pad = padding(gx);
        let top_pad = padding(gy);
        let right_pad = padding(columns - gx - gw);
        let bottom_pad = padding(rows - gy - gh);
        let pixel = size / 256.;
        let view = View {
            crs: super::runtime::parse_crs(self.crs())?,
            bounds: Bounds {
                min_x: left + gx as f64 * size - left_pad as f64 * pixel,
                max_x: left + (gx + gw) as f64 * size + right_pad as f64 * pixel,
                max_y: top - gy as f64 * size + top_pad as f64 * pixel,
                min_y: top - (gy + gh) as f64 * size - bottom_pad as f64 * pixel,
            },
            width: gw * 256 + left_pad + right_pad,
            height: gh * 256 + top_pad + bottom_pad,
            aspect: AspectPolicy::Stretch,
        };
        Ok((
            view,
            [
                (x - gx) * 256 + left_pad,
                (y - gy) * 256 + top_pad,
                256,
                256,
            ],
        ))
    }
}

/// Centered square clipped at matrix edges; coordinates never wrap.
pub(crate) fn neighbors(
    grid: TileGrid,
    z: u32,
    x: u32,
    y: u32,
    p: &ServerProfile,
) -> Vec<[u32; 3]> {
    let Ok([columns, rows]) = grid.dimensions(z) else {
        return vec![];
    };
    if x >= columns || y >= rows {
        return vec![];
    }
    let radius = p.wmts_prewarm_size / 2;
    let mut result = vec![];
    for row in y.saturating_sub(radius)..=y.saturating_add(radius).min(rows - 1) {
        for column in x.saturating_sub(radius)..=x.saturating_add(radius).min(columns - 1) {
            if [column, row] != [x, y] {
                result.push([z, column, row]);
            }
        }
    }
    // Warm near tiles first if the background queue is busy.
    result.sort_by_key(|&[_, nx, ny]| (nx.abs_diff(x).max(ny.abs_diff(y)), ny, nx));
    result
}

/// Freeze protocol defaults independently of the legacy/basic tile route.
pub(crate) fn publication(map: &crate::config::MapDefinition) -> (Vec<TileGrid>, u32, u32) {
    let (names, min, max) = map.tiles.as_ref().map_or((None, 0, 22), |t| {
        (Some(&t.matrix_sets), t.min_zoom, t.max_zoom)
    });
    let grids = [TileGrid::WebMercatorQuad, TileGrid::WorldCrs84Quad]
        .into_iter()
        .filter(|g| {
            map.output_crs.iter().any(|crs| crs == g.crs())
                && names.is_none_or(|names| names.iter().any(|name| name == g.id()))
        })
        .collect();
    (grids, min, max)
}
pub(crate) fn validate_publication(
    map: &crate::config::MapDefinition,
    grid: TileGrid,
    z: u32,
) -> Result<()> {
    let (grids, min, max) = publication(map);
    if !grids.contains(&grid) || z < min || z > max {
        return Err(ServerError::new(
            404,
            "tile-not-published",
            "Tile matrix or zoom is not published by this map",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn centered_neighbors_clip_both_world_grids() {
        let mut p = ServerConfig::load(std::path::Path::new("examples/server.experimental.toml"))
            .unwrap()
            .profile;
        assert_eq!(p.wmts_prewarm_size, 5);
        for grid in [TileGrid::WebMercatorQuad, TileGrid::WorldCrs84Quad] {
            for z in [0, 1, 2, 4, 30] {
                let [columns, rows] = grid.dimensions(z).unwrap();
                for [x, y] in [[0, 0], [columns - 1, rows - 1], [columns / 2, rows / 2]] {
                    let others = neighbors(grid, z, x, y, &p);
                    let expected = (x.saturating_add(2).min(columns - 1) - x.saturating_sub(2) + 1)
                        * (y.saturating_add(2).min(rows - 1) - y.saturating_sub(2) + 1)
                        - 1;
                    assert_eq!(others.len(), expected as usize);
                    let mut unique = std::collections::BTreeSet::new();
                    for [nz, nx, ny] in others {
                        assert_eq!(nz, z);
                        assert!(nx < columns && ny < rows);
                        assert!(nx.abs_diff(x) <= 2 && ny.abs_diff(y) <= 2);
                        assert_ne!([nx, ny], [x, y]);
                        assert!(unique.insert([nx, ny]));
                        let (view, crop) = grid.layout(nz, nx, ny, &p).unwrap();
                        assert!(crop[0] + 256 <= view.width && crop[1] + 256 <= view.height);
                    }
                }
            }
            assert!(neighbors(grid, 31, 0, 0, &p).is_empty());
            assert!(neighbors(grid, 0, 2, 2, &p).is_empty());
            assert_eq!(neighbors(grid, 4, 8, 8, &p).len(), 24);
            p.wmts_prewarm_size = 1;
            assert!(neighbors(grid, 4, 8, 8, &p).is_empty());
            p.wmts_prewarm_size = 5;
        }
    }
}

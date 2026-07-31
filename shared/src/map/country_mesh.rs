use std::iter;
use crate::artifact::geometry::{CountryFeature, GeometryLayer, Polygon};
use crate::error::AppError;
use crate::map::gpu_types::ProjectedVertex;
use crate::map::projection::{self, ProjectedPoint};
use crate::render::gpu_types::Vec2;

/// Divide-by-zero guard for normalizing a boundary normal, not a geometric tolerance. A normal is a
/// vector divided by its length; a zero-length edge (duplicate vertices) or a spike vertex whose two
/// edge normals cancel gives a ~zero length that would normalize to NaN. Below this the code substitutes
/// a safe value instead of normalizing: a zero vector for a dead edge, or one adjacent edge's unit
/// normal for a cancelling spike. Projected coordinates are radians spanning roughly ±π, so any distance
/// is at most a few units; 1e-12 sits far below the separation of any two genuinely distinct points yet
/// above f64 rounding noise, so it flags only coincident points and exact spikes at any resolution.
const NORMAL_EPSILON: f64 = 1e-12;

/// A country's GPU-ready geometry. It owns its data and holds no GPU handles, so it is `Send`: a worker
/// thread can build it, or the producer can bake it into the artifact, without involving the renderer.
#[derive(Debug, Clone)]
pub struct CountryMesh {
    pub iso3: String,
    pub region_code: String,
    pub vertices: Vec<ProjectedVertex>,
    /// One per item in `vertices`: the unit direction to push that vertex to inflate the country outward
    /// (away from its interior), used to raise and outline it when hovered or selected.
    pub outward_directions: Vec<Vec2>,
    pub fill_indices: Vec<u32>,
    pub border_indices: Vec<u32>,
}

impl CountryMesh {
    fn from_feature(feature: &CountryFeature) -> Result<CountryMesh, AppError> {
        let mut mesh: CountryMesh = CountryMesh {
            iso3: feature.iso3.clone(),
            region_code: feature.region_code.clone(),
            vertices: Vec::new(),
            outward_directions: Vec::new(),
            fill_indices: Vec::new(),
            border_indices: Vec::new(),
        };

        for polygon in &feature.polygons {
            mesh.append_polygon(polygon)?;
        }

        Ok(mesh)
    }

    fn append_polygon(&mut self, polygon: &Polygon) -> Result<(), AppError> {
        let polygon_vertex_offset: u32 = self.vertices.len() as u32;
        let rings: Vec<&[(f64, f64)]> = collect_rings(polygon);

        let (geographic_points, hole_indices): (Vec<(f64, f64)>, Vec<usize>) = flatten_rings(&rings);
        let projected_coordinates: Vec<f64> = project_points(&geographic_points);
        let fill_triangle_indices: Vec<u32> = triangulate_fill(&projected_coordinates, &hole_indices, polygon_vertex_offset)?;

        self.vertices.extend(to_projected_vertices(&projected_coordinates));
        self.outward_directions.extend(polygon_outward_directions(&rings));
        self.fill_indices.extend(fill_triangle_indices);
        self.border_indices.extend(ring_edge_indices(&rings, polygon_vertex_offset));

        Ok(())
    }
}

/// Project and triangulate every country in the layer into owned meshes.
pub fn build_country_meshes(geometry: &GeometryLayer) -> Result<Vec<CountryMesh>, AppError> {
    let country_features: Vec<CountryFeature> = geometry.iter_features()?;

    country_features.iter().map(CountryMesh::from_feature).collect()
}

fn collect_rings(polygon: &Polygon) -> Vec<&[(f64, f64)]> {
    iter::once(open_ring(&polygon.exterior))
        .chain(polygon.interiors.iter()
            .map(|interior_ring| open_ring(interior_ring)))
        .collect()
}

/// Concatenates the rings into one point sequence, returned alongside the vertex index at which each
/// interior ring (a hole) begins. The first ring is the exterior, so it contributes no entry.
fn flatten_rings(rings: &[&[(f64, f64)]]) -> (Vec<(f64, f64)>, Vec<usize>) {
    let mut geographic_points: Vec<(f64, f64)> = Vec::new();
    let mut hole_indices: Vec<usize> = Vec::new();

    for (ring_index, ring) in rings.iter().enumerate() {
        if ring_index > 0 {
            hole_indices.push(geographic_points.len());
        }

        geographic_points.extend_from_slice(ring);
    }

    (geographic_points, hole_indices)
}

/// Projects each (lon, lat) point through Miller into the `[x0, y0, x1, y1, ...]` array earcut expects.
fn project_points(geographic_points: &[(f64, f64)]) -> Vec<f64> {
    let mut projected_coordinates: Vec<f64> = Vec::new();

    // Antimeridian-crossing rings are not handled here. A ring whose source vertices span the
    // +180/-180 seam (e.g. Russia's Chukotka, Fiji, Kiribati) projects to x-values on both far
    // ends of the range, and earcut then triangulates the polygon across the entire ~358 degree
    // span instead of the thin sliver hugging the seam, smearing the country across the map. This
    // only matters if the geometry source stores such features as a single unsplit ring; if the
    // source already splits them per hemisphere, the naive projection below is correct. If it
    // does manifest, the fix is to clip each crossing ring against the antimeridian: insert new
    // vertices where edges cross +/-180, split the ring into two rings (one per side of the
    // line), and triangulate each independently.
    for &(lon, lat) in geographic_points {
        let projected: ProjectedPoint = projection::project(lat, lon);
        projected_coordinates.push(projected.x);
        projected_coordinates.push(projected.y);
    }

    projected_coordinates
}

/// earcut returns triangle indices local to this polygon's coordinate array; `polygon_vertex_offset`
/// shifts them to their position in the country's shared vertex buffer.
fn triangulate_fill(projected_coordinates: &[f64], hole_indices: &[usize], polygon_vertex_offset: u32) -> Result<Vec<u32>, AppError> {
    let fill_triangle_indices: Vec<usize> = earcutr::earcut(projected_coordinates, hole_indices, 2)
        .map_err(|error| AppError::from(format!("triangulation failed for a country polygon: {error:?}")))?;

    Ok(fill_triangle_indices.into_iter()
        .map(|index| polygon_vertex_offset + index as u32)
        .collect())
}

fn to_projected_vertices(projected_coordinates: &[f64]) -> Vec<ProjectedVertex> {
    projected_coordinates
        .chunks_exact(2)
        .map(|coordinate_pair| ProjectedVertex {
            position: Vec2 { x: coordinate_pair[0] as f32, y: coordinate_pair[1] as f32 },
        })
        .collect()
}

fn ring_edge_indices(rings: &[&[(f64, f64)]], polygon_vertex_offset: u32) -> Vec<u32> {
    let mut edge_indices: Vec<u32> = Vec::new();
    let mut ring_start: u32 = polygon_vertex_offset;

    for ring in rings {
        let ring_length: u32 = ring.len() as u32;
        for offset in 0..ring_length {
            edge_indices.push(ring_start + offset);
            edge_indices.push(ring_start + (offset + 1) % ring_length);
        }

        ring_start += ring_length;
    }

    edge_indices
}

/// FlatGeobuf/geo-types rings repeat the first vertex as the last to close the loop; earcut and the
/// border edges want the open ring, so drop that trailing duplicate.
fn open_ring(ring: &[(f64, f64)]) -> &[(f64, f64)] {
    if ring.len() >= 2 && ring.first() == ring.last() {
        &ring[..ring.len() - 1]
    } else {
        ring
    }
}

/// Per-vertex outward directions for a polygon's rings, concatenated in the same order `flatten_rings`
/// produces vertices (exterior first, then holes).
fn polygon_outward_directions(rings: &[&[(f64, f64)]]) -> Vec<Vec2> {
    rings
        .iter()
        .enumerate()
        .flat_map(|(ring_index, ring)| {
            let projected_ring: Vec<(f64, f64)> = ring
                .iter()
                .map(|&(lon, lat)| {
                    let projected: ProjectedPoint = projection::project(lat, lon);
                    (projected.x, projected.y)
                })
                .collect();
            let is_hole: bool = ring_index > 0;

            outward_directions_for_ring(&projected_ring, is_hole)
        })
        .collect()
}

/// The outward direction at each vertex of a closed ring of projected points (no repeated closing
/// vertex): the unit miter of the two adjacent edges' normals, oriented away from the solid area.
/// Winding is read from the signed area, so the ring's stored orientation does not matter; a hole flips
/// inward so the solid grows into it. The miter is unit length (no `1/cos` scaling), so a sharp corner
/// rounds slightly rather than shooting a spike.
fn outward_directions_for_ring(ring: &[(f64, f64)], is_hole: bool) -> Vec<Vec2> {
    let vertex_count: usize = ring.len();
    if vertex_count < 3 {
        return vec![Vec2 { x: 0.0, y: 0.0 }; vertex_count];
    }

    let winding_sign: f64 = if signed_area(ring) >= 0.0 { 1.0 } else { -1.0 };
    let direction_sign: f64 = if is_hole { -winding_sign } else { winding_sign };

    (0..vertex_count)
        .map(|index| {
            let previous: (f64, f64) = ring[(index + vertex_count - 1) % vertex_count];
            let current: (f64, f64) = ring[index];
            let next: (f64, f64) = ring[(index + 1) % vertex_count];

            let incoming: (f64, f64) = edge_outward_normal(previous, current, direction_sign);
            let outgoing: (f64, f64) = edge_outward_normal(current, next, direction_sign);

            unit_or((incoming.0 + outgoing.0, incoming.1 + outgoing.1), outgoing)
        })
        .collect()
}

/// The ring's signed area (shoelace formula). Only its sign is used here: positive if the vertices are
/// ordered counterclockwise around the interior, negative if clockwise; `edge_outward_normal` uses it to
/// rotate each edge toward the outside.
fn signed_area(ring: &[(f64, f64)]) -> f64 {
    let vertex_count: usize = ring.len();
    let mut area: f64 = 0.0;
    for index in 0..vertex_count {
        let (x0, y0): (f64, f64) = ring[index];
        let (x1, y1): (f64, f64) = ring[(index + 1) % vertex_count];
        area += x0 * y1 - x1 * y0;
    }

    area / 2.0
}

/// Unit normal of edge `a -> b`, rotated so `sign` (+1 for a counterclockwise ring, negated for holes)
/// makes it point away from the solid area; `(0, 0)` for a degenerate zero-length edge.
fn edge_outward_normal(a: (f64, f64), b: (f64, f64), sign: f64) -> (f64, f64) {
    let dx: f64 = b.0 - a.0;
    let dy: f64 = b.1 - a.1;
    let length: f64 = (dx * dx + dy * dy).sqrt();
    if length > NORMAL_EPSILON {
        (dy / length * sign, -dx / length * sign)
    } else {
        (0.0, 0.0)
    }
}

/// Normalize `v` into a `Vec2`, falling back to `fallback` (already unit, or zero) when `v` is near
/// zero, which happens at a spike vertex whose two edge normals cancel.
fn unit_or(v: (f64, f64), fallback: (f64, f64)) -> Vec2 {
    let length: f64 = (v.0 * v.0 + v.1 * v.1).sqrt();
    let (x, y): (f64, f64) = if length > NORMAL_EPSILON {
        (v.0 / length, v.1 / length)
    } else {
        fallback
    };

    Vec2 { x: x as f32, y: y as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::geometry::parse_geometry_layer;
    use crate::artifact::geometry::tests::one_feature_fgb_bytes;

    const TOLERANCE: f64 = 1e-4;

    // Testland is a rectangle over lon 0..2, lat 0..3; its exterior ring is the four corners
    // (0,0), (2,0), (2,3), (0,3) wound counterclockwise, with the closing duplicate dropped.
    const TESTLAND_CORNERS: [(f64, f64); 4] = [(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)];

    fn testland_mesh() -> CountryMesh {
        let geometry: GeometryLayer = parse_geometry_layer(one_feature_fgb_bytes()).unwrap();
        let mut country_meshes: Vec<CountryMesh> = build_country_meshes(&geometry).unwrap();

        assert_eq!(country_meshes.len(), 1);

        country_meshes.remove(0)
    }

    fn vertex_position(mesh: &CountryMesh, vertex_index: u32) -> (f64, f64) {
        let vertex: &ProjectedVertex = &mesh.vertices[vertex_index as usize];

        (vertex.position.x as f64, vertex.position.y as f64)
    }

    #[test]
    fn from_feature_projects_the_corners_in_ring_order() {
        let mesh: CountryMesh = testland_mesh();

        assert_eq!(mesh.iso3, "TST");
        assert_eq!(mesh.region_code, "testland");
        assert_eq!(mesh.vertices.len(), TESTLAND_CORNERS.len());

        for (vertex_index, &(lon, lat)) in TESTLAND_CORNERS.iter().enumerate() {
            let expected: ProjectedPoint = projection::project(lat, lon);
            let (x, y): (f64, f64) = vertex_position(&mesh, vertex_index as u32);

            assert!((x - expected.x).abs() < TOLERANCE);
            assert!((y - expected.y).abs() < TOLERANCE);
        }
    }

    #[test]
    fn from_feature_triangulation_tiles_the_whole_rectangle() {
        let mesh: CountryMesh = testland_mesh();

        assert_eq!(mesh.fill_indices.len(), 6);

        let mut triangulated_area: f64 = 0.0;
        for triangle in mesh.fill_indices.chunks_exact(3) {
            let (ax, ay): (f64, f64) = vertex_position(&mesh, triangle[0]);
            let (bx, by): (f64, f64) = vertex_position(&mesh, triangle[1]);
            let (cx, cy): (f64, f64) = vertex_position(&mesh, triangle[2]);

            triangulated_area += ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs() / 2.0;
        }

        // The projected rectangle spans the projected x-width of lon 0..2 by the projected y-height of
        // lat 0..3; a correct triangulation covers exactly that area, so a wrong winding or degenerate
        // triangle fails.
        let projected_width: f64 = projection::project(0.0, 2.0).x - projection::project(0.0, 0.0).x;
        let rectangle_area: f64 = projected_width * projection::project(3.0, 0.0).y;
        assert!((triangulated_area - rectangle_area).abs() < TOLERANCE);
    }

    #[test]
    fn from_feature_emits_the_closed_border_loop() {
        let mesh: CountryMesh = testland_mesh();

        assert_eq!(mesh.border_indices, vec![0, 1, 1, 2, 2, 3, 3, 0]);
    }

    #[test]
    fn outward_directions_for_ring_points_away_from_a_ccw_exterior() {
        let square: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

        let outward_directions: Vec<Vec2> = outward_directions_for_ring(&square, false);

        let diagonal: f32 = 0.5_f32.sqrt();
        let expected: [(f32, f32); 4] = [(-diagonal, -diagonal), (diagonal, -diagonal), (diagonal, diagonal), (-diagonal, diagonal)];
        for (outward_direction, (expected_x, expected_y)) in outward_directions.iter().zip(expected) {
            assert!((outward_direction.x - expected_x).abs() < 1e-5);
            assert!((outward_direction.y - expected_y).abs() < 1e-5);
        }
    }

    #[test]
    fn outward_directions_for_ring_is_winding_agnostic() {
        let clockwise_square: [(f64, f64); 4] = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];

        let outward_directions: Vec<Vec2> = outward_directions_for_ring(&clockwise_square, false);

        let diagonal: f32 = 0.5_f32.sqrt();
        assert!((outward_directions[0].x - -diagonal).abs() < 1e-5);
        assert!((outward_directions[0].y - -diagonal).abs() < 1e-5);
    }

    #[test]
    fn outward_directions_for_ring_flips_inward_for_a_hole() {
        let square: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

        let outward_directions: Vec<Vec2> = outward_directions_for_ring(&square, true);

        let diagonal: f32 = 0.5_f32.sqrt();
        assert!((outward_directions[0].x - diagonal).abs() < 1e-5);
        assert!((outward_directions[0].y - diagonal).abs() < 1e-5);
    }

    #[test]
    fn from_feature_outward_direction_per_vertex_points_away_from_the_interior() {
        let mesh: CountryMesh = testland_mesh();

        assert_eq!(mesh.outward_directions.len(), mesh.vertices.len());

        let projected_corners: Vec<ProjectedPoint> = TESTLAND_CORNERS
            .iter()
            .map(|&(lon, lat)| projection::project(lat, lon))
            .collect();
        let center_x: f64 = projected_corners.iter().map(|corner| corner.x).sum::<f64>() / 4.0;
        let center_y: f64 = projected_corners.iter().map(|corner| corner.y).sum::<f64>() / 4.0;

        for vertex_index in 0..mesh.vertices.len() {
            let (x, y): (f64, f64) = vertex_position(&mesh, vertex_index as u32);
            let outward_direction: &Vec2 = &mesh.outward_directions[vertex_index];
            let outward_dot: f64 = outward_direction.x as f64 * (x - center_x) + outward_direction.y as f64 * (y - center_y);

            assert!(outward_dot > 0.0);
        }
    }
}

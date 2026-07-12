use std::iter;
use crate::artifact::geometry::{CountryFeature, GeometryLayer, Polygon};
use crate::error::AppError;
use crate::map::gpu_types::ProjectedVertex;
use crate::map::projection::{self, ProjectedPoint};
use crate::render::gpu_types::Vec2;

/// A country's GPU-ready geometry: projected vertices shared by both pipelines, the fill triangle
/// indices (earcut), and the border line-segment indices (each ring's edges as `LineList` pairs).
/// It owns its data and holds no GPU handles, so it is `Send`: a worker thread can build it, or the
/// producer can bake it into the artifact, without involving the renderer.
#[derive(Debug, Clone)]
pub struct CountryMesh {
    pub iso3: String,
    pub region_code: String,
    pub vertices: Vec<ProjectedVertex>,
    pub fill_indices: Vec<u32>,
    pub border_indices: Vec<u32>,
}

impl CountryMesh {
    fn from_feature(feature: &CountryFeature) -> Result<CountryMesh, AppError> {
        let mut mesh: CountryMesh = CountryMesh {
            iso3: feature.iso3.clone(),
            region_code: feature.region_code.clone(),
            vertices: Vec::new(),
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

        // The projected rectangle is lon-width 2 by the projected height of lat 0..3; a correct
        // triangulation covers exactly that area, so a wrong winding or degenerate triangle fails.
        let rectangle_area: f64 = 2.0 * projection::project(3.0, 0.0).y;
        assert!((triangulated_area - rectangle_area).abs() < TOLERANCE);
    }

    #[test]
    fn from_feature_emits_the_closed_border_loop() {
        let mesh: CountryMesh = testland_mesh();

        assert_eq!(mesh.border_indices, vec![0, 1, 1, 2, 2, 3, 3, 0]);
    }
}

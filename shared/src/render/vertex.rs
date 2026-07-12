use std::iter;
use crate::artifact::geometry::{CountryFeature, GeometryLayer, Polygon};
use crate::error::AppError;
use crate::map::projection::{self, ProjectedPoint};
use crate::render::gpu_types::{ProjectedVertex, Vec2};

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
        let base: u32 = self.vertices.len() as u32;
        let rings: Vec<&[(f64, f64)]> = collect_rings(polygon);

        let (geographic_points, hole_indices): (Vec<(f64, f64)>, Vec<usize>) = flatten_rings(&rings);
        let projected_coordinates: Vec<f64> = project_points(&geographic_points);
        let fill_triangle_indices: Vec<u32> = triangulate_fill(&projected_coordinates, &hole_indices, base)?;

        self.fill_indices.extend(fill_triangle_indices);
        self.vertices.extend(to_projected_vertices(&projected_coordinates));
        append_border_edges(&rings, base, &mut self.border_indices);

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

/// earcut returns triangle indices local to this polygon's coordinate array; `base` shifts them to
/// their position in the country's shared vertex buffer.
fn triangulate_fill(projected_coordinates: &[f64], hole_indices: &[usize], base: u32) -> Result<Vec<u32>, AppError> {
    let fill_triangle_indices: Vec<usize> = earcutr::earcut(projected_coordinates, hole_indices, 2)
        .map_err(|error| AppError::from(format!("triangulation failed for a country polygon: {error:?}")))?;

    Ok(fill_triangle_indices.into_iter()
        .map(|index| base + index as u32)
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

fn append_border_edges(rings: &[&[(f64, f64)]], base: u32, border_indices: &mut Vec<u32>) {
    let mut ring_start: u32 = base;
    for ring in rings {
        let ring_length: u32 = ring.len() as u32;
        for offset in 0..ring_length {
            border_indices.push(ring_start + offset);
            border_indices.push(ring_start + (offset + 1) % ring_length);
        }

        ring_start += ring_length;
    }
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

    #[test]
    fn build_country_meshes_triangulates_the_testland_rectangle() {
        let geometry: GeometryLayer = parse_geometry_layer(one_feature_fgb_bytes()).unwrap();

        let country_meshes: Vec<CountryMesh> = build_country_meshes(&geometry).unwrap();

        assert_eq!(country_meshes.len(), 1);
        let mesh: &CountryMesh = &country_meshes[0];
        assert_eq!(mesh.iso3, "TST");
        assert_eq!(mesh.region_code, "testland");
        // Four corners (the closing duplicate dropped), two triangles, one four-edge ring.
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.fill_indices.len(), 6);
        assert_eq!(mesh.border_indices.len(), 8);
    }
}

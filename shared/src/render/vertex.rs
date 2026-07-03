use crate::artifact::geometry::{CountryFeature, GeometryLayer, Polygon};
use crate::error::AppError;
use crate::map::projection::{self, ProjectedPoint};

/// One map vertex in Miller-projected space, before the viewport transform the vertex shader applies.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MapVertex {
    pub position: [f32; 2],
}

/// A country's GPU-ready geometry: projected vertices shared by both pipelines, the fill triangle
/// indices (earcut), and the border line-segment indices (each ring's edges as `LineList` pairs).
/// Built off the GPU thread and owned outright, so it is `Send` and can be produced on a worker or
/// baked by the producer later without touching the renderer.
#[derive(Debug, Clone)]
pub struct CountryMesh {
    pub iso3: String,
    pub region_code: String,
    pub vertices: Vec<MapVertex>,
    pub fill_indices: Vec<u32>,
    pub border_indices: Vec<u32>,
}

/// Project and triangulate every country in the layer. Pure and `Send`-returning: the only
/// GPU-thread-bound step is uploading the result into buffers, which the renderer does separately.
pub fn build_country_meshes(geometry: &GeometryLayer) -> Result<Vec<CountryMesh>, AppError> {
    let country_features: Vec<CountryFeature> = geometry.iter_features()?;

    country_features.iter().map(build_country_mesh).collect()
}

fn build_country_mesh(feature: &CountryFeature) -> Result<CountryMesh, AppError> {
    let mut vertices: Vec<MapVertex> = Vec::new();
    let mut fill_indices: Vec<u32> = Vec::new();
    let mut border_indices: Vec<u32> = Vec::new();

    for polygon in &feature.polygons {
        append_polygon(polygon, &mut vertices, &mut fill_indices, &mut border_indices)?;
    }

    Ok(CountryMesh {
        iso3: feature.iso3.clone(),
        region_code: feature.region_code.clone(),
        vertices,
        fill_indices,
        border_indices,
    })
}

fn append_polygon(
    polygon: &Polygon,
    vertices: &mut Vec<MapVertex>,
    fill_indices: &mut Vec<u32>,
    border_indices: &mut Vec<u32>,
) -> Result<(), AppError> {
    let base: u32 = vertices.len() as u32;

    let rings: Vec<&[(f64, f64)]> = std::iter::once(open_ring(&polygon.exterior))
        .chain(polygon.interiors.iter().map(|interior_ring| open_ring(interior_ring)))
        .collect();

    let mut projected_coordinates: Vec<f64> = Vec::new();
    let mut hole_indices: Vec<usize> = Vec::new();
    for (ring_index, ring) in rings.iter().enumerate() {
        if ring_index > 0 {
            hole_indices.push(projected_coordinates.len() / 2);
        }

        for &(lon, lat) in *ring {
            let projected: ProjectedPoint = projection::project(lat, lon);
            projected_coordinates.push(projected.x);
            projected_coordinates.push(projected.y);
        }
    }

    let fill_triangle_indices: Vec<usize> = earcutr::earcut(&projected_coordinates, &hole_indices, 2)
        .map_err(|error| AppError::from(format!("triangulation failed for a country polygon: {error:?}")))?;

    for index in fill_triangle_indices {
        fill_indices.push(base + index as u32);
    }

    for coordinate_pair in projected_coordinates.chunks_exact(2) {
        vertices.push(MapVertex { position: [coordinate_pair[0] as f32, coordinate_pair[1] as f32] });
    }

    append_border_edges(&rings, base, border_indices);

    Ok(())
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

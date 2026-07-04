use std::marker::PhantomData;
use std::sync::Arc;

use tokio::sync::watch;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::{
    Adapter, BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferAddress, BufferDescriptor, BufferUsages,
    Color, CommandBuffer, CommandEncoder, CommandEncoderDescriptor, CurrentSurfaceTexture, Device, DeviceDescriptor,
    ExperimentalFeatures, Features, IndexFormat, Instance, Limits, LoadOp, MemoryHints, Operations, PowerPreference,
    Queue, RenderPass, RenderPassColorAttachment, RenderPassDescriptor, RequestAdapterOptions, StoreOp, SurfaceTexture,
    TextureView, TextureViewDescriptor, Trace,
};

use crate::artifact::{Bundle, StatisticShardKey};
use crate::canonical::StatisticKind;
use crate::error::AppError;
use crate::map::color::{self, Rgba};
use crate::map::projection;
use crate::map::value_types::{FrameState, Viewport};
use crate::render::gpu_types::{FillVertex, ProjectedVertex, ViewportUniform};
use crate::render::pipeline::RenderPipelines;
use crate::render::surface::WgpuSurface;
use crate::render::vertex::{self, CountryMesh};
use crate::sqlite::shard_db::{self, ShardValues};

// not for wasm32: the native attach path takes a raw window handle; the web attaches from a canvas.
#[cfg(not(target_arch = "wasm32"))]
use crate::map::value_types::WindowHandle;

/// The wgpu state machine. `!Send` (the `PhantomData<*const ()>`) because wgpu resources are bound
/// to the thread that created them: the single WASM thread on web, the Swift main thread on iOS.
pub struct Renderer {
    instance: Instance,
    adapter: Adapter,
    device: Device,
    queue: Queue,
    bundle_receiver: watch::Receiver<Arc<Bundle>>,
    country_geometry: CountryGeometry,
    attached: Option<AttachedSurface>,
    _not_send: PhantomData<*const ()>,
}

/// The country polygons uploaded to the GPU once: one shared position buffer, the fill triangle
/// indices, the border line indices, and the per-country vertex ranges the choropleth colors each.
struct CountryGeometry {
    positions: Buffer,
    fill_indices: Buffer,
    fill_index_count: u32,
    border_indices: Buffer,
    border_index_count: u32,
    vertex_count: u32,
    spans: Vec<CountrySpan>,
}

struct CountrySpan {
    iso3: String,
    vertex_start: u32,
    vertex_count: u32,
}

/// The surface-dependent state, built together at attach and dropped together at detach: the
/// surface, the pipelines (compiled against its format), and the viewport uniform + bind group.
struct AttachedSurface {
    surface: WgpuSurface,
    pipelines: RenderPipelines,
    viewport_buffer: Buffer,
    viewport_bind_group: BindGroup,
}

impl Renderer {
    pub async fn new(bundle_receiver: watch::Receiver<Arc<Bundle>>) -> Result<Renderer, AppError> {
        let instance: Instance = Instance::default();

        let adapter: Adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| AppError::from(format!("requesting a GPU adapter failed: {error}")))?;

        let (device, queue): (Device, Queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("eafora-renderer-device"),
                required_features: Features::empty(),
                required_limits: Limits::downlevel_webgl2_defaults(),
                experimental_features: ExperimentalFeatures::default(),
                memory_hints: MemoryHints::Performance,
                trace: Trace::Off,
            })
            .await
            .map_err(|error| AppError::from(format!("requesting a GPU device failed: {error}")))?;

        let bundle: Arc<Bundle> = bundle_receiver.borrow().clone();
        let country_geometry: CountryGeometry = upload_country_geometry(&device, &bundle)?;

        Ok(Renderer {
            instance,
            adapter,
            device,
            queue,
            bundle_receiver,
            country_geometry,
            attached: None,
            _not_send: PhantomData,
        })
    }

    #[cfg(not(target_arch = "wasm32"))] // not for wasm32: the web attaches from a canvas, not a window handle
    pub fn attach_surface(&mut self, window_handle: WindowHandle, width: u32, height: u32) -> Result<(), AppError> {
        let surface: WgpuSurface =
            WgpuSurface::from_window_handle(&self.instance, &self.adapter, &self.device, window_handle, width, height)?;
        let pipelines: RenderPipelines = RenderPipelines::create(&self.device, surface.format());

        let viewport_buffer: Buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("eafora-viewport-uniform"),
            size: std::mem::size_of::<ViewportUniform>() as BufferAddress,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let viewport_bind_group: BindGroup = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("eafora-viewport-bind-group"),
            layout: &pipelines.viewport_bind_group_layout,
            entries: &[BindGroupEntry { binding: 0, resource: viewport_buffer.as_entire_binding() }],
        });

        self.attached = Some(AttachedSurface { surface, pipelines, viewport_buffer, viewport_bind_group });

        Ok(())
    }

    pub fn detach_surface(&mut self) {
        self.attached = None;
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        let attached: &mut AttachedSurface =
            self.attached.as_mut().expect("resize_surface: attach_surface must be called first");
        attached.surface.resize(&self.device, width, height);

        Ok(())
    }

    pub fn draw_frame(&mut self, viewport: Viewport, frame_state: FrameState) -> Result<(), AppError> {
        let bundle: Arc<Bundle> = self.bundle_receiver.borrow_and_update().clone();
        let fill_colors: Vec<FillVertex> = self.compute_fill_colors(&bundle, &frame_state)?;

        let attached: &AttachedSurface =
            self.attached.as_ref().expect("draw_frame: attach_surface must be called first");

        let surface_texture: SurfaceTexture = match attached.surface.inner().get_current_texture() {
            CurrentSurfaceTexture::Success(texture) | CurrentSurfaceTexture::Suboptimal(texture) => texture,
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                attached.surface.reconfigure(&self.device);
                return Ok(());
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            CurrentSurfaceTexture::Validation => {
                return Err(AppError::from("draw_frame: acquiring the surface texture failed validation".to_string()));
            }
        };

        let viewport_uniform: ViewportUniform = viewport_to_uniform(viewport);
        self.queue.write_buffer(&attached.viewport_buffer, 0, bytemuck::cast_slice(&[viewport_uniform]));

        let fill_color_buffer: Buffer = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("eafora-fill-colors"),
            contents: bytemuck::cast_slice(&fill_colors),
            usage: BufferUsages::VERTEX,
        });

        let view: TextureView = surface_texture.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder: CommandEncoder =
            self.device.create_command_encoder(&CommandEncoderDescriptor { label: Some("eafora-frame-encoder") });

        {
            let mut render_pass: RenderPass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("eafora-map-pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations { load: LoadOp::Clear(Color::WHITE), store: StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            render_pass.set_bind_group(0, &attached.viewport_bind_group, &[]);

            render_pass.set_pipeline(&attached.pipelines.fill);
            render_pass.set_vertex_buffer(0, self.country_geometry.positions.slice(..));
            render_pass.set_vertex_buffer(1, fill_color_buffer.slice(..));
            render_pass.set_index_buffer(self.country_geometry.fill_indices.slice(..), IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.country_geometry.fill_index_count, 0, 0..1);

            render_pass.set_pipeline(&attached.pipelines.border);
            render_pass.set_vertex_buffer(0, self.country_geometry.positions.slice(..));
            render_pass.set_index_buffer(self.country_geometry.border_indices.slice(..), IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.country_geometry.border_index_count, 0, 0..1);
        }

        let command_buffer: CommandBuffer = encoder.finish();
        self.queue.submit([command_buffer]);
        self.queue.present(surface_texture);

        Ok(())
    }

    fn compute_fill_colors(&self, bundle: &Bundle, frame_state: &FrameState) -> Result<Vec<FillVertex>, AppError> {
        let no_data_fill: FillVertex = to_fill_vertex(color::choropleth_fill(None, 0.0, 1.0));
        let mut fill_colors: Vec<FillVertex> = vec![no_data_fill; self.country_geometry.vertex_count as usize];

        let Some(shard_bytes) = select_shard(bundle, frame_state.active_statistic) else {
            return Ok(fill_colors);
        };

        let shard_values: ShardValues = shard_db::load_shard(shard_bytes)?;
        let Some((statistic_min, statistic_max)) = shard_values.range() else {
            return Ok(fill_colors);
        };

        for span in &self.country_geometry.spans {
            let value: Option<f64> = shard_values.value(&span.iso3, frame_state.active_period_start);
            let fill_vertex: FillVertex = to_fill_vertex(color::choropleth_fill(value, statistic_min, statistic_max));
            for vertex_index in span.vertex_start..(span.vertex_start + span.vertex_count) {
                fill_colors[vertex_index as usize] = fill_vertex;
            }
        }

        Ok(fill_colors)
    }
}

fn upload_country_geometry(device: &Device, bundle: &Bundle) -> Result<CountryGeometry, AppError> {
    let country_meshes: Vec<CountryMesh> = vertex::build_country_meshes(&bundle.geometry)?;

    let mut positions: Vec<ProjectedVertex> = Vec::new();
    let mut fill_indices: Vec<u32> = Vec::new();
    let mut border_indices: Vec<u32> = Vec::new();
    let mut spans: Vec<CountrySpan> = Vec::new();
    for country_mesh in &country_meshes {
        let vertex_start: u32 = positions.len() as u32;

        positions.extend_from_slice(&country_mesh.vertices);
        fill_indices.extend(country_mesh.fill_indices.iter().map(|&index| vertex_start + index));
        border_indices.extend(country_mesh.border_indices.iter().map(|&index| vertex_start + index));
        spans.push(CountrySpan {
            iso3: country_mesh.iso3.clone(),
            vertex_start,
            vertex_count: country_mesh.vertices.len() as u32,
        });
    }

    let positions_buffer: Buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("eafora-country-positions"),
        contents: bytemuck::cast_slice(&positions),
        usage: BufferUsages::VERTEX,
    });
    let fill_index_buffer: Buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("eafora-country-fill-indices"),
        contents: bytemuck::cast_slice(&fill_indices),
        usage: BufferUsages::INDEX,
    });
    let border_index_buffer: Buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("eafora-country-border-indices"),
        contents: bytemuck::cast_slice(&border_indices),
        usage: BufferUsages::INDEX,
    });

    Ok(CountryGeometry {
        positions: positions_buffer,
        fill_indices: fill_index_buffer,
        fill_index_count: fill_indices.len() as u32,
        border_indices: border_index_buffer,
        border_index_count: border_indices.len() as u32,
        vertex_count: positions.len() as u32,
        spans,
    })
}

/// The shard whose values color the map. Provisional policy: the first authorized license class that
/// ships a shard for the active statistic. Refining this to the source-choice rules is future work.
fn select_shard(bundle: &Bundle, statistic_kind: StatisticKind) -> Option<&Vec<u8>> {
    bundle
        .distribution_context
        .authorized_classes()
        .iter()
        .find_map(|license_shard_class| {
            bundle
                .shard_bytes
                .get(&StatisticShardKey { statistic_kind, license_shard_class: *license_shard_class })
        })
}

fn viewport_to_uniform(viewport: Viewport) -> ViewportUniform {
    // Longitude is the projected x directly (Miller x = lon); latitude drives the nonlinear y, so the
    // viewport's latitude bounds are projected before they become clip-space extents.
    let projected_min_y: f32 = projection::project(viewport.latitude_min, 0.0).y as f32;
    let projected_max_y: f32 = projection::project(viewport.latitude_max, 0.0).y as f32;

    ViewportUniform {
        bounds: [
            viewport.longitude_min as f32,
            projected_min_y,
            viewport.longitude_max as f32,
            projected_max_y,
        ],
        offset: [0.0, 0.0, 0.0, 0.0],
    }
}

fn to_fill_vertex(color: Rgba) -> FillVertex {
    FillVertex { color: [color.r, color.g, color.b, color.a] }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 1e-6;

    #[test]
    fn viewport_to_uniform_projects_the_latitude_bounds() {
        let viewport: Viewport = Viewport {
            longitude_min: -10.0,
            longitude_max: 30.0,
            latitude_min: 0.0,
            latitude_max: 3.0,
        };

        let uniform: ViewportUniform = viewport_to_uniform(viewport);

        // Longitude passes through as x; the equator projects to y = 0; the offset defaults to zero.
        assert!((uniform.bounds[0] - (-10.0)).abs() < TOLERANCE);
        assert!((uniform.bounds[1] - 0.0).abs() < TOLERANCE);
        assert!((uniform.bounds[2] - 30.0).abs() < TOLERANCE);
        assert!(uniform.bounds[3] > 0.0);
        assert_eq!(uniform.offset, [0.0, 0.0, 0.0, 0.0]);
    }
}

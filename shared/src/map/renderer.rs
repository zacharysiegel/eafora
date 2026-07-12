use std::marker::PhantomData;
use std::sync::Arc;

use tokio::sync::watch;
use chrono::NaiveDate;
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
use crate::map::value_types::{FrameState, Viewport};
use crate::map::country_mesh::{self, CountryMesh};
use crate::map::gpu_types::{FillVertex, ProjectedVertex, ViewportUniform};
use crate::map::pipeline::RenderPipelines;
use crate::render::gpu_types::{Vec2, Vec4};
use crate::render::surface::WgpuSurface;
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
    cached_fill_colors: Option<CachedFillColors>,
    attached: Option<AttachedState>,
    _not_send: PhantomData<*const ()>,
}

/// A GPU index buffer paired with the number of indices to draw from it.
struct IndexBuffer {
    buffer: Buffer,
    count: u32,
}

/// Uploaded to the GPU once at construction and reused every frame; only the fill colors change.
struct CountryGeometry {
    positions: Buffer,
    vertex_count: u32,
    fill: IndexBuffer,
    border: IndexBuffer,
    spans: Vec<CountrySpan>,
}

struct CountrySpan {
    iso3: String,
    vertex_start: u32,
    vertex_count: u32,
}

/// The inputs that determine the choropleth colors — the cache key for the color buffer. The bundle
/// is compared by identity (`Arc::ptr_eq`), since a hot-swap publishes a new `Arc`.
struct FillColorKey {
    statistic_kind: StatisticKind,
    period_start: NaiveDate,
    bundle: Arc<Bundle>,
}

impl FillColorKey {
    fn matches(&self, other: &FillColorKey) -> bool {
        self.statistic_kind == other.statistic_kind
            && self.period_start == other.period_start
            && Arc::ptr_eq(&self.bundle, &other.bundle)
    }
}

/// The choropleth color buffer plus the key it was built for. Rebuilt only when the key changes;
/// a viewport pan/zoom or a hover reuses the same buffer.
struct CachedFillColors {
    buffer: Buffer,
    key: FillColorKey,
}

/// The surface-dependent state, built together at attach and dropped together at detach.
struct AttachedState {
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
            cached_fill_colors: None,
            attached: None,
            _not_send: PhantomData,
        })
    }

    #[cfg(not(target_arch = "wasm32"))] // not for wasm32: the web attaches from a canvas, not a window handle
    pub async fn attach_surface(&mut self, window_handle: WindowHandle, width: u32, height: u32) -> Result<(), AppError> {
        let surface: WgpuSurface =
            WgpuSurface::from_window_handle(&self.instance, &self.adapter, &self.device, window_handle, width, height)?;
        let pipelines: RenderPipelines = RenderPipelines::create(&self.device, surface.format()).await?;

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

        self.attached = Some(AttachedState { surface, pipelines, viewport_buffer, viewport_bind_group });

        Ok(())
    }

    pub fn detach_surface(&mut self) {
        self.attached = None;
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        let attached: &mut AttachedState =
            self.attached.as_mut().expect("resize_surface: attach_surface must be called first");
        attached.surface.resize(&self.device, width, height);

        Ok(())
    }

    pub fn draw_frame(&mut self, viewport: Viewport, frame_state: FrameState) -> Result<(), AppError> {
        let bundle: Arc<Bundle> = self.bundle_receiver.borrow_and_update().clone();
        self.refresh_fill_colors(&bundle, &frame_state)?;

        let attached: &AttachedState =
            self.attached.as_ref().expect("draw_frame: attach_surface must be called first");
        let fill_color_buffer: &Buffer = &self
            .cached_fill_colors
            .as_ref()
            .expect("refresh_fill_colors populates the cache")
            .buffer;

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

        let instance_count: u32 = wrap_instance_count(viewport);

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
            render_pass.set_index_buffer(self.country_geometry.fill.buffer.slice(..), IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.country_geometry.fill.count, 0, 0..instance_count);

            render_pass.set_pipeline(&attached.pipelines.border);
            render_pass.set_vertex_buffer(0, self.country_geometry.positions.slice(..));
            render_pass.set_index_buffer(self.country_geometry.border.buffer.slice(..), IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.country_geometry.border.count, 0, 0..instance_count);
        }

        let command_buffer: CommandBuffer = encoder.finish();
        self.queue.submit([command_buffer]);
        self.queue.present(surface_texture);

        Ok(())
    }

    /// Rebuilds the fill-color buffer only when its inputs (active statistic, period, or the bundle)
    /// have changed since the last frame. A pan, zoom, or hover leaves them untouched and reuses the
    /// cached buffer.
    fn refresh_fill_colors(&mut self, bundle: &Arc<Bundle>, frame_state: &FrameState) -> Result<(), AppError> {
        let key: FillColorKey = FillColorKey {
            statistic_kind: frame_state.active_statistic,
            period_start: frame_state.active_period_start,
            bundle: Arc::clone(bundle),
        };

        let is_current: bool = self.cached_fill_colors.as_ref().is_some_and(|cached| cached.key.matches(&key));
        if is_current {
            return Ok(());
        }

        let fill_colors: Vec<FillVertex> = self.compute_fill_colors(bundle, frame_state)?;
        let buffer: Buffer = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("eafora-fill-colors"),
            contents: bytemuck::cast_slice(&fill_colors),
            usage: BufferUsages::VERTEX,
        });

        self.cached_fill_colors = Some(CachedFillColors { buffer, key });

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
    let country_meshes: Vec<CountryMesh> = country_mesh::build_country_meshes(&bundle.geometry)?;

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
        vertex_count: positions.len() as u32,
        fill: IndexBuffer { buffer: fill_index_buffer, count: fill_indices.len() as u32 },
        border: IndexBuffer { buffer: border_index_buffer, count: border_indices.len() as u32 },
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
    ViewportUniform {
        projected_min: Vec2 { x: viewport.min.x as f32, y: viewport.min.y as f32 },
        projected_max: Vec2 { x: viewport.max.x as f32, y: viewport.max.y as f32 },
    }
}

/// Two instances when the viewport crosses the +/-180 antimeridian (the natural copy plus the wrapped
/// copy the shader shifts a full turn), one otherwise. Assumes the viewport is clamped to at most
/// 360 degrees wide, so it can cross at most one seam.
fn wrap_instance_count(viewport: Viewport) -> u32 {
    let crosses_antimeridian: bool = viewport.min.x < -180.0 || viewport.max.x > 180.0;

    if crosses_antimeridian {
        2
    } else {
        1
    }
}

fn to_fill_vertex(color: Rgba) -> FillVertex {
    FillVertex { color: Vec4 { x: color.r, y: color.g, z: color.b, w: color.a } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::projection::ProjectedPoint;

    const TOLERANCE: f32 = 1e-6;

    #[test]
    fn viewport_to_uniform_copies_the_projected_bounds() {
        let viewport: Viewport = Viewport {
            min: ProjectedPoint { x: -10.0, y: -1.5 },
            max: ProjectedPoint { x: 30.0, y: 1.5 },
        };

        let uniform: ViewportUniform = viewport_to_uniform(viewport);

        assert!((uniform.projected_min.x - (-10.0)).abs() < TOLERANCE);
        assert!((uniform.projected_min.y - (-1.5)).abs() < TOLERANCE);
        assert!((uniform.projected_max.x - 30.0).abs() < TOLERANCE);
        assert!((uniform.projected_max.y - 1.5).abs() < TOLERANCE);
    }

    #[test]
    fn wrap_instance_count_is_two_only_when_the_viewport_crosses_the_seam() {
        let within_one_world: Viewport = Viewport {
            min: ProjectedPoint { x: -170.0, y: -1.0 },
            max: ProjectedPoint { x: 170.0, y: 1.0 },
        };
        assert_eq!(wrap_instance_count(within_one_world), 1);

        let past_west_edge: Viewport = Viewport {
            min: ProjectedPoint { x: -190.0, y: -1.0 },
            max: ProjectedPoint { x: -10.0, y: 1.0 },
        };
        assert_eq!(wrap_instance_count(past_west_edge), 2);

        let past_east_edge: Viewport = Viewport {
            min: ProjectedPoint { x: 10.0, y: -1.0 },
            max: ProjectedPoint { x: 190.0, y: 1.0 },
        };
        assert_eq!(wrap_instance_count(past_east_edge), 2);
    }
}

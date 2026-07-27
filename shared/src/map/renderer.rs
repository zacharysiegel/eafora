use std::marker::PhantomData;
use std::sync::Arc;

use tokio::sync::watch;
use chrono::NaiveDate;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::{
    Adapter, Backends, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Buffer, BufferAddress,
    BufferDescriptor, BufferUsages, Color, CommandBuffer, CommandEncoder, CommandEncoderDescriptor, CurrentSurfaceTexture,
    Device, DeviceDescriptor, ExperimentalFeatures, Features, IndexFormat, Instance, InstanceDescriptor, Limits,
    LoadOp, MemoryHints, Operations, PowerPreference, Queue, RenderPass, RenderPassColorAttachment,
    RenderPassDescriptor, RequestAdapterOptions, StoreOp, SurfaceTexture, TextureView, TextureViewDescriptor, Trace,
};

use crate::artifact::Bundle;
use crate::canonical::StatisticKind;
use crate::error::AppError;
use crate::map::color::{self, StatisticColorTransform, Rgba};
use crate::map::{FrameState, Viewport};
use crate::map::country_mesh::{self, CountryMesh};
use crate::map::gpu_types::{FillVertex, ProjectedVertex, ViewportUniform};
use crate::map::pipeline::{self, RenderPipelines};
use crate::render::gpu_types::{Vec2, Vec4};
use crate::render::surface::WgpuSurface;
use crate::sqlite::shard_db::{self, ShardValues};

// the native attach path takes a raw window handle; the web attaches from a canvas.
#[cfg(not(target_arch = "wasm32"))]
use crate::render::WindowHandle;

// the canvas attach path takes an HtmlCanvasElement instead of a raw window handle.
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;

/// Which GPU backend the renderer's wgpu instance may use. `ForceGl` restricts it to WebGL2 for the
/// web client's `?renderer=webgl2` parity-testing flag; `Default` lets wgpu prefer WebGPU where present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererBackend {
    Default,
    ForceGl,
}

/// The wgpu state machine. `!Send` (the `PhantomData<*const ()>`) because wgpu resources are bound
/// to the thread that created them: the single WASM thread on web, the Swift main thread on iOS.
pub struct Renderer {
    instance: Instance,
    adapter: Adapter,
    device: Device,
    queue: Queue,
    bundle_receiver: watch::Receiver<Arc<Bundle>>,
    country_geometry: CountryGeometry,
    viewport_binding: ViewportBinding,
    fill_colors: FillColors,
    attached: Option<AttachedState>,
    _not_send: PhantomData<*const ()>,
}

/// Uploaded to the GPU once at construction and never rebuilt.
struct CountryGeometry {
    positions: CountedBuffer,
    fill: CountedBuffer,
    border: CountedBuffer,
    spans: Vec<CountrySpan>,
}

/// A GPU buffer paired with the number of elements it holds.
struct CountedBuffer {
    buffer: Buffer,
    count: u32,
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

/// The choropleth color buffer paired with the key describing what is currently written into it. The
/// buffer is persistent and rewritten in place; the key gates those rewrites (`None` before the first).
struct FillColors {
    buffer: Buffer,
    key: Option<FillColorKey>,
}

/// The viewport uniform's device-lifetime GPU resources. They are format-independent, so unlike the
/// pipelines they are created once and outlive any surface; the buffer's contents are rewritten each
/// frame with the current camera.
struct ViewportBinding {
    buffer: Buffer,
    bind_group: BindGroup,
    layout: BindGroupLayout,
}

/// The surface and its format-specialized pipelines, built together at attach and dropped together
/// at detach. The geometry, viewport binding, and color buffer all outlive the surface.
struct AttachedState {
    surface: WgpuSurface,
    pipelines: RenderPipelines,
}

impl Renderer {
    pub async fn new(bundle_receiver: watch::Receiver<Arc<Bundle>>, backend: RendererBackend) -> Result<Renderer, AppError> {
        let instance: Instance = create_instance(backend);

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
        let viewport_binding: ViewportBinding = create_viewport_binding(&device);
        let fill_color_buffer: Buffer = device.create_buffer(&BufferDescriptor {
            label: Some("eafora-fill-colors"),
            size: country_geometry.positions.count as BufferAddress * std::mem::size_of::<FillVertex>() as BufferAddress,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fill_colors: FillColors = FillColors { buffer: fill_color_buffer, key: None };

        Ok(Renderer {
            instance,
            adapter,
            device,
            queue,
            bundle_receiver,
            country_geometry,
            viewport_binding,
            fill_colors,
            attached: None,
            _not_send: PhantomData,
        })
    }

    #[cfg(not(target_arch = "wasm32"))] // takes a raw window handle; the web attaches from a canvas
    pub async fn attach_surface_from_window_handle(&mut self, window_handle: WindowHandle, width: u32, height: u32) -> Result<(), AppError> {
        let surface: WgpuSurface =
            WgpuSurface::from_window_handle(&self.instance, &self.adapter, &self.device, window_handle, width, height)?;

        self.attach(surface).await
    }

    #[cfg(target_arch = "wasm32")] // attaches from an HtmlCanvasElement, not a raw window handle
    pub async fn attach_surface_from_canvas(&mut self, canvas: HtmlCanvasElement, width: u32, height: u32) -> Result<(), AppError> {
        let surface: WgpuSurface =
            WgpuSurface::from_canvas(&self.instance, &self.adapter, &self.device, canvas, width, height)?;

        self.attach(surface).await
    }

    /// Builds the surface-format pipelines and stores the attached state. Surface-agnostic — shared by
    /// the native window-handle path and the canvas path — so it is not target-gated.
    async fn attach(&mut self, surface: WgpuSurface) -> Result<(), AppError> {
        let pipelines: RenderPipelines =
            RenderPipelines::create(&self.device, surface.format(), &self.viewport_binding.layout).await?;

        self.attached = Some(AttachedState { surface, pipelines });

        Ok(())
    }

    pub fn detach_surface(&mut self) {
        self.attached = None;
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        let attached: &mut AttachedState = self.attached.as_mut()
                .expect("resize_surface: a surface must be attached first");
        attached.surface.resize(&self.device, width, height);

        Ok(())
    }

    pub fn draw_frame(&mut self, viewport: Viewport, frame_state: &FrameState) -> Result<(), AppError> {
        let bundle: Arc<Bundle> = self.bundle_receiver.borrow_and_update().clone();
        self.refresh_fill_colors(&bundle, frame_state)?;

        let Some(surface_texture) = self.acquire_surface_texture()? else {
            return Ok(());
        };

        self.write_viewport_uniform(viewport);

        let view: TextureView = surface_texture.texture.create_view(&TextureViewDescriptor::default());
        let instance_count: u32 = if is_antimeridian_wrap(viewport) { 2 } else { 1 };
        let command_buffer: CommandBuffer = self.record_map_pass(&view, instance_count);

        self.queue.submit([command_buffer]);
        self.queue.present(surface_texture);

        Ok(())
    }

    /// Maps each wgpu surface state to a frame action: `Some(texture)` to render, `None` to skip this
    /// frame, `Err` to abort. A lost surface detaches and errors because recreating it needs the
    /// window handle the renderer doesn't retain; only the shell can, by re-attaching the surface.
    fn acquire_surface_texture(&mut self) -> Result<Option<SurfaceTexture>, AppError> {
        let acquired: CurrentSurfaceTexture = self
            .attached
            .as_ref()
            .expect("draw_frame: a surface must be attached first")
            .surface
            .inner()
            .get_current_texture();

        match acquired {
            CurrentSurfaceTexture::Success(texture) => Ok(Some(texture)),
            CurrentSurfaceTexture::Suboptimal(texture) => {
                self.reconfigure_attached_surface();
                Ok(Some(texture))
            }
            CurrentSurfaceTexture::Outdated => {
                self.reconfigure_attached_surface();
                Ok(None)
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => Ok(None),
            CurrentSurfaceTexture::Lost => {
                self.attached = None;
                Err(AppError::from("draw_frame: surface lost; the shell must re-attach".to_string()))
            }
            CurrentSurfaceTexture::Validation => {
                Err(AppError::from("draw_frame: acquiring the surface texture failed validation".to_string()))
            }
        }
    }

    fn reconfigure_attached_surface(&self) {
        self.attached
            .as_ref()
            .expect("draw_frame: a surface must be attached first")
            .surface
            .reconfigure(&self.device);
    }

    fn write_viewport_uniform(&self, viewport: Viewport) {
        let viewport_uniform: ViewportUniform = viewport.to_gpu();

        self.queue.write_buffer(&self.viewport_binding.buffer, 0, bytemuck::cast_slice(&[viewport_uniform]));
    }

    fn record_map_pass(&self, view: &TextureView, instance_count: u32) -> CommandBuffer {
        let attached: &AttachedState = self.attached.as_ref()
            .expect("draw_frame: a surface must be attached first");

        let mut encoder: CommandEncoder =
            self.device.create_command_encoder(&CommandEncoderDescriptor { label: Some("eafora-map-command-encoder") });

        let mut render_pass: RenderPass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("eafora-map-pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(Color::WHITE), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_bind_group(0, &self.viewport_binding.bind_group, &[]);

        render_pass.set_pipeline(&attached.pipelines.fill);
        render_pass.set_vertex_buffer(0, self.country_geometry.positions.buffer.slice(..));
        render_pass.set_vertex_buffer(1, self.fill_colors.buffer.slice(..));
        render_pass.set_index_buffer(self.country_geometry.fill.buffer.slice(..), IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.country_geometry.fill.count, 0, 0..instance_count);

        render_pass.set_pipeline(&attached.pipelines.border);
        render_pass.set_vertex_buffer(0, self.country_geometry.positions.buffer.slice(..));
        render_pass.set_index_buffer(self.country_geometry.border.buffer.slice(..), IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.country_geometry.border.count, 0, 0..instance_count);

        // wgpu has no RenderPass::end(); a pass ends only when dropped. Dropping records the end-of-pass
        // (via the backend pass's own Drop) and releases the pass's mutable borrow of the encoder; both
        // are required before encoder.finish().
        drop(render_pass);

        encoder.finish()
    }

    /// Rewrites the fill-color buffer in place only when its inputs (active statistic, period, or the
    /// bundle) have changed since the last frame. A pan, zoom, or hover leaves them untouched, so the
    /// buffer keeps whatever was last uploaded.
    fn refresh_fill_colors(&mut self, bundle: &Arc<Bundle>, frame_state: &FrameState) -> Result<(), AppError> {
        let key: FillColorKey = FillColorKey {
            statistic_kind: frame_state.active_statistic,
            period_start: frame_state.active_period_start,
            bundle: Arc::clone(bundle),
        };

        let is_current: bool = self.fill_colors.key.as_ref()
            .is_some_and(|current| current.matches(&key));
        if is_current {
            return Ok(());
        }

        let fill_vertices: Vec<FillVertex> = self.compute_fill_colors(bundle, frame_state)?;
        self.queue.write_buffer(&self.fill_colors.buffer, 0, bytemuck::cast_slice(&fill_vertices));
        self.fill_colors.key = Some(key);

        Ok(())
    }

    fn compute_fill_colors(&self, bundle: &Bundle, frame_state: &FrameState) -> Result<Vec<FillVertex>, AppError> {
        let no_data_fill: FillVertex = color::CHOROPLETH_SCALE.no_data().to_gpu();
        let mut fill_vertices: Vec<FillVertex> = vec![no_data_fill; self.country_geometry.positions.count as usize];

        let Some(shard_bytes) = bundle.shard_for(frame_state.active_statistic) else {
            return Ok(fill_vertices);
        };

        let shard_values: ShardValues = shard_db::read_shard(shard_bytes)?;
        let Some((statistic_min, statistic_max)) = shard_values.value_range() else {
            return Ok(fill_vertices);
        };

        let transform: StatisticColorTransform = color::transform_for(frame_state.active_statistic);

        for span in &self.country_geometry.spans {
            let value: Option<f64> = shard_values.value(&span.iso3, frame_state.active_period_start);
            let fill: Rgba = match value {
                Some(value) => color::CHOROPLETH_SCALE.sample(transform.position(value, statistic_min, statistic_max)),
                None => color::CHOROPLETH_SCALE.no_data(),
            };
            let fill_vertex: FillVertex = fill.to_gpu();
            for vertex_index in span.vertex_start..(span.vertex_start + span.vertex_count) {
                fill_vertices[vertex_index as usize] = fill_vertex;
            }
        }

        Ok(fill_vertices)
    }
}

fn create_instance(backend: RendererBackend) -> Instance {
    match backend {
        RendererBackend::Default => Instance::default(),
        RendererBackend::ForceGl => {
            let mut descriptor: InstanceDescriptor = InstanceDescriptor::new_without_display_handle();
            descriptor.backends = Backends::GL;
            Instance::new(descriptor)
        }
    }
}

fn create_viewport_binding(device: &Device) -> ViewportBinding {
    let layout: BindGroupLayout = pipeline::create_viewport_bind_group_layout(device);
    let buffer: Buffer = device.create_buffer(&BufferDescriptor {
        label: Some("eafora-viewport-uniform"),
        size: std::mem::size_of::<ViewportUniform>() as BufferAddress,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group: BindGroup = device.create_bind_group(&BindGroupDescriptor {
        label: Some("eafora-viewport-bind-group"),
        layout: &layout,
        entries: &[BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
    });

    ViewportBinding { buffer, bind_group, layout }
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
        positions: CountedBuffer { buffer: positions_buffer, count: positions.len() as u32 },
        fill: CountedBuffer { buffer: fill_index_buffer, count: fill_indices.len() as u32 },
        border: CountedBuffer { buffer: border_index_buffer, count: border_indices.len() as u32 },
        spans,
    })
}

impl Viewport {
    fn to_gpu(&self) -> ViewportUniform {
        ViewportUniform {
            projected_min: Vec2 { x: self.min.x as f32, y: self.min.y as f32 },
            projected_max: Vec2 { x: self.max.x as f32, y: self.max.y as f32 },
        }
    }
}

impl Rgba {
    fn to_gpu(&self) -> FillVertex {
        FillVertex { color: Vec4 { x: self.r, y: self.g, z: self.b, w: self.a } }
    }
}

fn is_antimeridian_wrap(viewport: Viewport) -> bool {
    viewport.min.x < -180.0 || viewport.max.x > 180.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::projection::ProjectedPoint;

    const TOLERANCE: f32 = 1e-6;

    #[test]
    fn viewport_to_gpu_copies_the_projected_bounds() {
        let viewport: Viewport = Viewport {
            min: ProjectedPoint { x: -10.0, y: -1.5 },
            max: ProjectedPoint { x: 30.0, y: 1.5 },
        };

        let uniform: ViewportUniform = viewport.to_gpu();

        assert!((uniform.projected_min.x - (-10.0)).abs() < TOLERANCE);
        assert!((uniform.projected_min.y - (-1.5)).abs() < TOLERANCE);
        assert!((uniform.projected_max.x - 30.0).abs() < TOLERANCE);
        assert!((uniform.projected_max.y - 1.5).abs() < TOLERANCE);
    }

    #[test]
    fn is_antimeridian_wrap_is_true_only_when_the_viewport_crosses_the_seam() {
        let within_one_world: Viewport = Viewport {
            min: ProjectedPoint { x: -170.0, y: -1.0 },
            max: ProjectedPoint { x: 170.0, y: 1.0 },
        };
        assert!(!is_antimeridian_wrap(within_one_world));

        let past_west_edge: Viewport = Viewport {
            min: ProjectedPoint { x: -190.0, y: -1.0 },
            max: ProjectedPoint { x: -10.0, y: 1.0 },
        };
        assert!(is_antimeridian_wrap(past_west_edge));

        let past_east_edge: Viewport = Viewport {
            min: ProjectedPoint { x: 10.0, y: -1.0 },
            max: ProjectedPoint { x: 190.0, y: 1.0 },
        };
        assert!(is_antimeridian_wrap(past_east_edge));
    }
}

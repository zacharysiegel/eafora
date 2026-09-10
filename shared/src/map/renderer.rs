use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::ops::Range;
use std::sync::Arc;

use tokio::sync::watch;
use chrono::NaiveDate;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::{
    Adapter, Backends, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Buffer,
    BufferAddress, BufferDescriptor, BufferUsages, Color, CommandBuffer, CommandEncoder, CommandEncoderDescriptor,
    CurrentSurfaceTexture, Device, DeviceDescriptor, ExperimentalFeatures, Extent3d, Features, IndexFormat, Instance,
    InstanceDescriptor, Limits, LoadOp, MemoryHints, Operations, Origin3d, PowerPreference, Queue, RenderPass,
    RenderPassColorAttachment, RenderPassDescriptor, RequestAdapterOptions, StoreOp, SurfaceTexture,
    TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView, TextureViewDescriptor, Trace,
};

use crate::artifact::Bundle;
use crate::canonical::StatisticKind;
use crate::error::AppError;
use crate::map::color::{self, StatisticColorTransform, Rgba};
use crate::map::{FrameState, RegionCode, Viewport};
use crate::map::country_mesh::{self, CountryMesh};
use crate::map::gpu_types::{CountryState, FillVertexAttributes, EmphasisVertexAttributes, ProjectedVertexAttributes, ViewportUniform, COUNTRY_STATE_TEXTURE_WIDTH};
use crate::map::pipeline::{self, RenderPipelines};
use crate::render::gpu_types::{Vec2, Vec4};
use crate::render::surface::WgpuSurface;
use crate::sqlite::shard_db::ShardValues;

// the native attach path takes a raw window handle; the web attaches from a canvas.
#[cfg(not(target_arch = "wasm32"))]
use crate::render::WindowHandle;

// the canvas attach path takes an HtmlCanvasElement instead of a raw window handle.
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;

/// The outward lift, in screen pixels, applied to a hovered country so it reads as raised.
const HOVER_LIFT_PX: f32 = 4.0;

/// The black outline rim widths, in screen pixels: a thin rim on the hovered country and a bolder one on
/// the selected country (its persistent on-map indicator, since selection does not lift).
const HOVER_OUTLINE_PX: f32 = 2.0;
const SELECTED_OUTLINE_PX: f32 = 6.0;

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
    map_binding: MapBinding,
    fill_colors: FillColors,
    attached: Option<AttachedState>,
    _not_send: PhantomData<*const ()>,
}

struct CountryGeometry {
    positions: CountedBuffer,
    emphasis: Buffer,
    fill: CountedBuffer,
    boundary: CountedBuffer,
    spans: Vec<CountrySpan>,
    geometry_relative_path: String,
}

/// A GPU buffer paired with the number of elements it holds.
struct CountedBuffer {
    buffer: Buffer,
    count: u32,
}

struct CountrySpan {
    region_code: String,
    vertex_start: u32,
    vertex_count: u32,
    fill_index_start: u32,
    fill_index_count: u32,
}

impl CountrySpan {
    fn fill_range(&self) -> Range<u32> {
        self.fill_index_start..(self.fill_index_start + self.fill_index_count)
    }
}

struct FillColorInputs {
    statistic_kind: StatisticKind,
    period_start: NaiveDate,
    /// Compared by identity, since a hot swap publishes a new `Arc` over equal data.
    bundle: Arc<Bundle>,
}

impl FillColorInputs {
    fn matches(&self, other: &FillColorInputs) -> bool {
        self.statistic_kind == other.statistic_kind
            && self.period_start == other.period_start
            && Arc::ptr_eq(&self.bundle, &other.bundle)
    }
}

struct FillColors {
    buffer: Buffer,
    inputs: Option<FillColorInputs>,
}

struct EmphasisInputs {
    selected_region: Option<RegionCode>,
    hovered_region: Option<RegionCode>,
    hover_lift_enabled: bool,
}

impl EmphasisInputs {
    fn of(frame_state: &FrameState) -> EmphasisInputs {
        EmphasisInputs {
            selected_region: frame_state.selected_region.clone(),
            hovered_region: frame_state.hovered_region.clone(),
            hover_lift_enabled: frame_state.hover_lift_enabled,
        }
    }

    /// Takes the frame directly, so an unchanged frame allocates nothing.
    fn matches(&self, frame_state: &FrameState) -> bool {
        self.selected_region == frame_state.selected_region
            && self.hovered_region == frame_state.hovered_region
            && self.hover_lift_enabled == frame_state.hover_lift_enabled
    }
}

/// Independent of the surface's pixel format, so unlike the pipelines these outlive any surface.
struct MapBinding {
    viewport_buffer: Buffer,
    country_state_texture: Texture,
    /// The texels currently holding a non-zero emphasis, so a change clears only the ones it has to.
    emphasized_country_indices: Vec<u32>,
    /// What the emphasis texels hold; `None` before the first write and after a resize zeroes them.
    emphasis_inputs: Option<EmphasisInputs>,
    bind_group: BindGroup,
    layout: BindGroupLayout,
}

impl MapBinding {
    /// A swapped-in layer has its own region count, so the texture is resized to it. The bind group names
    /// the old texture and is rebuilt with it; the layout is untouched, since the pipelines were built
    /// against it.
    fn resize_country_state(&mut self, device: &Device, region_count: usize) {
        self.country_state_texture = create_country_state_texture(device, region_count);
        self.emphasized_country_indices.clear();
        self.emphasis_inputs = None;

        let bind_group: BindGroup =
            create_map_bind_group(device, &self.layout, &self.viewport_buffer, &self.country_state_texture);
        self.bind_group = bind_group;
    }
}

/// Created at attach and dropped at detach as a unit; the geometry, map binding, and color buffer all
/// outlive the surface.
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
        let country_geometry: CountryGeometry = create_country_geometry(&device, &bundle)?;
        let map_binding: MapBinding = create_map_binding(&device, country_geometry.spans.len());
        let fill_colors: FillColors = FillColors {
            buffer: create_fill_color_buffer(&device, country_geometry.positions.count),
            inputs: None,
        };

        Ok(Renderer {
            instance,
            adapter,
            device,
            queue,
            bundle_receiver,
            country_geometry,
            map_binding,
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
            RenderPipelines::create(&self.device, surface.format(), &self.map_binding.layout).await?;

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
        self.refresh_country_geometry(&bundle)?;
        self.refresh_fill_colors(&bundle, frame_state);

        let Some(surface_texture) = self.acquire_surface_texture()? else {
            return Ok(());
        };

        self.write_viewport_uniform(viewport);
        self.write_country_state(frame_state);

        let view: TextureView = surface_texture.texture.create_view(&TextureViewDescriptor::default());
        let instance_count: u32 = if is_antimeridian_wrap(viewport) { 2 } else { 1 };
        let command_buffer: CommandBuffer = self.record_map_pass(&view, instance_count, frame_state);

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
        let (width, height): (u32, u32) = self.attached.as_ref()
            .expect("draw_frame: a surface must be attached first")
            .surface.size();
        let viewport_uniform: ViewportUniform = viewport.to_gpu(Vec2 { x: width as f32, y: height as f32 });

        self.queue.write_buffer(&self.map_binding.viewport_buffer, 0, bytemuck::cast_slice(&[viewport_uniform]));
    }

    /// Rewrites the emphasis texels that changed. A country that is both hovered and selected keeps the
    /// bolder outline.
    fn write_country_state(&mut self, frame_state: &FrameState) {
        let is_already_written: bool = self.map_binding.emphasis_inputs
            .as_ref()
            .is_some_and(|emphasis_inputs| emphasis_inputs.matches(frame_state));

        if is_already_written {
            return;
        }

        let mut emphasized: BTreeMap<u32, CountryState> = BTreeMap::new();

        if let Some(country_index) = self.country_index_of(frame_state.selected_region.as_ref()) {
            emphasized.insert(country_index as u32, CountryState { lift_px: 0.0, outline_px: SELECTED_OUTLINE_PX });
        }
        if let Some(country_index) = self.country_index_of(frame_state.hovered_region.as_ref()) {
            let country_state: &mut CountryState = emphasized
                .entry(country_index as u32)
                .or_insert(CountryState { lift_px: 0.0, outline_px: 0.0 });

            if frame_state.hover_lift_enabled {
                country_state.lift_px = HOVER_LIFT_PX;
            }
            country_state.outline_px = country_state.outline_px.max(HOVER_OUTLINE_PX);
        }

        let cleared_country_indices: Vec<u32> = self.map_binding.emphasized_country_indices.iter()
            .copied()
            .filter(|country_index| !emphasized.contains_key(country_index))
            .collect();

        for country_index in cleared_country_indices {
            self.write_country_state_texel(country_index, CountryState { lift_px: 0.0, outline_px: 0.0 });
        }
        for (&country_index, &country_state) in &emphasized {
            self.write_country_state_texel(country_index, country_state);
        }

        self.map_binding.emphasized_country_indices = emphasized.into_keys().collect();
        self.map_binding.emphasis_inputs = Some(EmphasisInputs::of(frame_state));
    }

    fn write_country_state_texel(&self, country_index: u32, country_state: CountryState) {
        self.queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.map_binding.country_state_texture,
                mip_level: 0,
                origin: Origin3d {
                    x: country_index % COUNTRY_STATE_TEXTURE_WIDTH,
                    y: country_index / COUNTRY_STATE_TEXTURE_WIDTH,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(&[country_state]),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size_of::<CountryState>() as u32),
                rows_per_image: Some(1),
            },
            Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
    }

    /// The build-order index of the span whose region matches `region`, i.e. the `country_index` its
    /// vertices carry, or `None` when `region` is absent or has no matching span.
    fn country_index_of(&self, region: Option<&RegionCode>) -> Option<usize> {
        let region: &RegionCode = region?;

        self.country_geometry.spans.iter()
            .position(|span| span.region_code == region.0)
    }

    /// The fill-index ranges of the emphasized countries (selected, then hovered) in draw order, so the
    /// hovered country renders last; deduplicated when they are the same country, and skipping a region
    /// with no matching span. Each range is redrawn on top of the base layer as a black silhouette plus
    /// fill, so neighboring fills do not cover it.
    fn emphasized_country_fill_ranges(&self, frame_state: &FrameState) -> Vec<Range<u32>> {
        let selected: Option<usize> = self.country_index_of(frame_state.selected_region.as_ref());
        let hovered: Option<usize> = self.country_index_of(frame_state.hovered_region.as_ref());

        let mut indices: Vec<usize> = Vec::new();
        if let Some(selected) = selected {
            indices.push(selected);
        }
        if let Some(hovered) = hovered {
            if Some(hovered) != selected {
                indices.push(hovered);
            }
        }

        indices.into_iter()
            .map(|index| self.country_geometry.spans[index].fill_range()).collect()
    }

    fn record_map_pass(&self, view: &TextureView, instance_count: u32, frame_state: &FrameState) -> CommandBuffer {
        let attached: &AttachedState = self.attached.as_ref()
            .expect("draw_frame: a surface must be attached first");
        let emphasized_country_fill_ranges: Vec<Range<u32>> = self.emphasized_country_fill_ranges(frame_state);

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

        render_pass.set_bind_group(0, &self.map_binding.bind_group, &[]);

        // Base layer: every country's fills, then every country's boundaries.
        render_pass.set_pipeline(&attached.pipelines.fill);
        render_pass.set_vertex_buffer(0, self.country_geometry.positions.buffer.slice(..));
        render_pass.set_vertex_buffer(1, self.fill_colors.buffer.slice(..));
        render_pass.set_vertex_buffer(2, self.country_geometry.emphasis.slice(..));
        render_pass.set_index_buffer(self.country_geometry.fill.buffer.slice(..), IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.country_geometry.fill.count, 0, 0..instance_count);

        render_pass.set_pipeline(&attached.pipelines.boundary);
        render_pass.set_vertex_buffer(0, self.country_geometry.positions.buffer.slice(..));
        render_pass.set_vertex_buffer(1, self.country_geometry.emphasis.slice(..));
        render_pass.set_index_buffer(self.country_geometry.boundary.buffer.slice(..), IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.country_geometry.boundary.count, 0, 0..instance_count);

        // Redraw the emphasized countries on top of the base layer so neighboring fills do not cover
        // them. Each is drawn twice through the same fill-triangle range: first inflated outward and
        // painted black (the emphasis_outline pipeline), then in the choropleth color at its normal
        // extent (the fill pipeline). The black copy is larger, so only its rim shows around the color
        // fill; that rim is the outline, and being a filled silhouette it stays clean on multi-island
        // countries.
        for country_fill_range in &emphasized_country_fill_ranges {
            render_pass.set_vertex_buffer(0, self.country_geometry.positions.buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.fill_colors.buffer.slice(..));
            render_pass.set_vertex_buffer(2, self.country_geometry.emphasis.slice(..));
            render_pass.set_index_buffer(self.country_geometry.fill.buffer.slice(..), IndexFormat::Uint32);

            render_pass.set_pipeline(&attached.pipelines.emphasis_outline);
            render_pass.draw_indexed(country_fill_range.clone(), 0, 0..instance_count);

            render_pass.set_pipeline(&attached.pipelines.fill);
            render_pass.draw_indexed(country_fill_range.clone(), 0, 0..instance_count);
        }

        // wgpu has no RenderPass::end(); a pass ends only when dropped. Dropping records the end-of-pass
        // (via the backend pass's own Drop) and releases the pass's mutable borrow of the encoder; both
        // are required before encoder.finish().
        drop(render_pass);

        encoder.finish()
    }

    /// The buffers and their spans come from one bundle's geometry layer, and a hot-swap can bring a
    /// different one. Both the choropleth and the hover emphasis key on the region codes the spans carry, so
    /// keeping stale buffers renders a map that disagrees with what the hit-test reports.
    fn refresh_country_geometry(&mut self, bundle: &Bundle) -> Result<(), AppError> {
        if self.country_geometry.geometry_relative_path == bundle.manifest.geometry.relative_path {
            return Ok(());
        }

        log::info!(
            "rebuilding country geometry for a swapped-in bundle; [geometry={}]",
            bundle.manifest.geometry.relative_path,
        );

        self.country_geometry = create_country_geometry(&self.device, bundle)?;
        self.map_binding.resize_country_state(&self.device, self.country_geometry.spans.len());
        self.fill_colors = FillColors {
            buffer: create_fill_color_buffer(&self.device, self.country_geometry.positions.count),
            inputs: None,
        };

        Ok(())
    }

    /// Rewrites the fill-color buffer in place only when its inputs (active statistic, period, or the
    /// bundle) have changed since the last frame. A pan, zoom, or hover leaves them untouched, so the
    /// buffer keeps whatever was last uploaded.
    fn refresh_fill_colors(&mut self, bundle: &Arc<Bundle>, frame_state: &FrameState) {
        let inputs: FillColorInputs = FillColorInputs {
            statistic_kind: frame_state.active_statistic,
            period_start: frame_state.active_period_start,
            bundle: Arc::clone(bundle),
        };

        let is_current: bool = self.fill_colors.inputs.as_ref()
            .is_some_and(|current| current.matches(&inputs));
        if is_current {
            return;
        }

        let fill_vertices: Vec<FillVertexAttributes> = self.compute_fill_colors(bundle, frame_state);
        self.queue.write_buffer(&self.fill_colors.buffer, 0, bytemuck::cast_slice(&fill_vertices));
        self.fill_colors.inputs = Some(inputs);
    }

    fn compute_fill_colors(&self, bundle: &Bundle, frame_state: &FrameState) -> Vec<FillVertexAttributes> {
        let no_data_fill: FillVertexAttributes = color::CHOROPLETH_SCALE.no_data().to_gpu();
        let mut fill_vertices: Vec<FillVertexAttributes> = vec![no_data_fill; self.country_geometry.positions.count as usize];

        let active_shard_values: Option<&ShardValues> = bundle.shard_values_for(frame_state.active_statistic);
        let Some(shard_values) = active_shard_values
        else {
            return fill_vertices;
        };

        let Some((statistic_min, statistic_max)) = shard_values.value_range()
        else {
            return fill_vertices;
        };

        let transform: StatisticColorTransform = color::transform_for(frame_state.active_statistic);

        for span in &self.country_geometry.spans {
            let value: Option<f64> = shard_values.value(&span.region_code, frame_state.active_period_start);
            let fill: Rgba = match value {
                Some(value) => color::CHOROPLETH_SCALE.sample(transform.position(value, statistic_min, statistic_max)),
                None => color::CHOROPLETH_SCALE.no_data(),
            };
            let fill_vertex: FillVertexAttributes = fill.to_gpu();
            for vertex_index in span.vertex_start..(span.vertex_start + span.vertex_count) {
                fill_vertices[vertex_index as usize] = fill_vertex;
            }
        }

        fill_vertices
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

fn create_map_binding(device: &Device, region_count: usize) -> MapBinding {
    let layout: BindGroupLayout = pipeline::create_map_bind_group_layout(device);
    let viewport_buffer: Buffer = device.create_buffer(&BufferDescriptor {
        label: Some("eafora-viewport-uniform"),
        size: size_of::<ViewportUniform>() as BufferAddress,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let country_state_texture: Texture = create_country_state_texture(device, region_count);
    let bind_group: BindGroup = create_map_bind_group(device, &layout, &viewport_buffer, &country_state_texture);

    MapBinding {
        viewport_buffer,
        country_state_texture,
        emphasized_country_indices: Vec::new(),
        emphasis_inputs: None,
        bind_group,
        layout,
    }
}

/// One texel per region, laid out in rows because a texture dimension is capped far below the number of
/// regions a layer can hold. Zero-filled on creation, which is the un-emphasized state.
fn create_country_state_texture(device: &Device, region_count: usize) -> Texture {
    let rows: u32 = (region_count as u32).div_ceil(COUNTRY_STATE_TEXTURE_WIDTH).max(1);

    device.create_texture(&TextureDescriptor {
        label: Some("eafora-country-state"),
        size: Extent3d { width: COUNTRY_STATE_TEXTURE_WIDTH, height: rows, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rg32Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// No sampler entry: the shader reads the texture with `textureLoad`, and a sampler bound to the same unit
/// would override the NEAREST filtering an unfilterable float format requires, making every read return zero.
fn create_map_bind_group(
    device: &Device,
    layout: &BindGroupLayout,
    viewport_buffer: &Buffer,
    country_state_texture: &Texture,
) -> BindGroup {
    let country_state_view: TextureView = country_state_texture.create_view(&TextureViewDescriptor::default());

    device.create_bind_group(&BindGroupDescriptor {
        label: Some("eafora-map-bind-group"),
        layout,
        entries: &[
            BindGroupEntry { binding: 0, resource: viewport_buffer.as_entire_binding() },
            BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&country_state_view) },
        ],
    })
}

fn create_country_geometry(device: &Device, bundle: &Bundle) -> Result<CountryGeometry, AppError> {
    let country_meshes: Vec<CountryMesh> = country_mesh::build_country_meshes(&bundle.geometry)?;

    let mut positions: Vec<ProjectedVertexAttributes> = Vec::new();
    let mut emphasis_vertices: Vec<EmphasisVertexAttributes> = Vec::new();
    let mut fill_indices: Vec<u32> = Vec::new();
    let mut boundary_indices: Vec<u32> = Vec::new();
    let mut spans: Vec<CountrySpan> = Vec::new();
    for (country_index, country_mesh) in country_meshes.iter().enumerate() {
        let vertex_start: u32 = positions.len() as u32;
        let fill_index_start: u32 = fill_indices.len() as u32;

        positions.extend_from_slice(&country_mesh.vertices);
        emphasis_vertices.extend(country_mesh.outward_directions.iter().map(|&outward_direction| EmphasisVertexAttributes {
            outward_direction,
            country_index: country_index as u32,
        }));
        fill_indices.extend(country_mesh.fill_indices.iter().map(|&index| vertex_start + index));
        boundary_indices.extend(country_mesh.boundary_indices.iter().map(|&index| vertex_start + index));
        spans.push(CountrySpan {
            region_code: country_mesh.region_code.clone(),
            vertex_start,
            vertex_count: country_mesh.vertices.len() as u32,
            fill_index_start,
            fill_index_count: country_mesh.fill_indices.len() as u32,
        });
    }

    let positions_buffer: Buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("eafora-country-positions"),
        contents: bytemuck::cast_slice(&positions),
        usage: BufferUsages::VERTEX,
    });
    let emphasis_buffer: Buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("eafora-country-emphasis"),
        contents: bytemuck::cast_slice(&emphasis_vertices),
        usage: BufferUsages::VERTEX,
    });
    let fill_index_buffer: Buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("eafora-country-fill-indices"),
        contents: bytemuck::cast_slice(&fill_indices),
        usage: BufferUsages::INDEX,
    });
    let boundary_index_buffer: Buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("eafora-country-boundary-indices"),
        contents: bytemuck::cast_slice(&boundary_indices),
        usage: BufferUsages::INDEX,
    });

    Ok(CountryGeometry {
        positions: CountedBuffer { buffer: positions_buffer, count: positions.len() as u32 },
        emphasis: emphasis_buffer,
        fill: CountedBuffer { buffer: fill_index_buffer, count: fill_indices.len() as u32 },
        boundary: CountedBuffer { buffer: boundary_index_buffer, count: boundary_indices.len() as u32 },
        spans,
        geometry_relative_path: bundle.manifest.geometry.relative_path.clone(),
    })
}

fn create_fill_color_buffer(device: &Device, vertex_count: u32) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("eafora-fill-colors"),
        size: vertex_count as BufferAddress * std::mem::size_of::<FillVertexAttributes>() as BufferAddress,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Viewport {
    fn to_gpu(&self, surface_size: Vec2) -> ViewportUniform {
        ViewportUniform {
            projected_min: Vec2 { x: self.min.x as f32, y: self.min.y as f32 },
            projected_max: Vec2 { x: self.max.x as f32, y: self.max.y as f32 },
            surface_size,
            _padding: Vec2 { x: 0.0, y: 0.0 },
        }
    }
}

impl Rgba {
    fn to_gpu(&self) -> FillVertexAttributes {
        FillVertexAttributes { color: Vec4 { x: self.r, y: self.g, z: self.b, w: self.a } }
    }
}

fn is_antimeridian_wrap(viewport: Viewport) -> bool {
    viewport.min.x < -std::f64::consts::PI || viewport.max.x > std::f64::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::projection::ProjectedPoint;

    const TOLERANCE: f32 = 1e-6;

    fn emphasized_frame_state() -> FrameState {
        FrameState {
            active_statistic: StatisticKind::Tfr,
            active_period_start: NaiveDate::from_ymd_opt(2024, 1, 1).expect("a valid date"),
            selected_region: Some(RegionCode("pol".to_string())),
            hovered_region: Some(RegionCode("deu".to_string())),
            hover_lift_enabled: true,
        }
    }

    #[test]
    fn emphasis_inputs_matches_the_frame_it_was_taken_from() {
        let frame_state: FrameState = emphasized_frame_state();
        let emphasis_inputs: EmphasisInputs = EmphasisInputs::of(&frame_state);

        assert!(emphasis_inputs.matches(&frame_state));
    }

    #[test]
    fn emphasis_inputs_matches_a_frame_differing_only_outside_the_emphasis() {
        let emphasis_inputs: EmphasisInputs = EmphasisInputs::of(&emphasized_frame_state());

        let mut scrubbed: FrameState = emphasized_frame_state();
        scrubbed.active_period_start = NaiveDate::from_ymd_opt(2010, 1, 1).expect("a valid date");

        assert!(emphasis_inputs.matches(&scrubbed));
    }

    #[test]
    fn emphasis_inputs_does_not_match_a_moved_hover_or_selection() {
        let emphasis_inputs: EmphasisInputs = EmphasisInputs::of(&emphasized_frame_state());

        let mut hover_moved: FrameState = emphasized_frame_state();
        hover_moved.hovered_region = Some(RegionCode("fra".to_string()));

        let mut hover_left: FrameState = emphasized_frame_state();
        hover_left.hovered_region = None;

        let mut selection_cleared: FrameState = emphasized_frame_state();
        selection_cleared.selected_region = None;

        let mut lift_disabled: FrameState = emphasized_frame_state();
        lift_disabled.hover_lift_enabled = false;

        assert!(!emphasis_inputs.matches(&hover_moved));
        assert!(!emphasis_inputs.matches(&hover_left));
        assert!(!emphasis_inputs.matches(&selection_cleared));
        assert!(!emphasis_inputs.matches(&lift_disabled));
    }

    #[test]
    fn viewport_to_gpu_copies_the_projected_bounds() {
        let viewport: Viewport = Viewport {
            min: ProjectedPoint { x: -10.0, y: -1.5 },
            max: ProjectedPoint { x: 30.0, y: 1.5 },
        };

        let uniform: ViewportUniform = viewport.to_gpu(Vec2 { x: 800.0, y: 600.0 });

        assert!((uniform.projected_min.x - (-10.0)).abs() < TOLERANCE);
        assert!((uniform.projected_min.y - (-1.5)).abs() < TOLERANCE);
        assert!((uniform.projected_max.x - 30.0).abs() < TOLERANCE);
        assert!((uniform.projected_max.y - 1.5).abs() < TOLERANCE);
        assert!((uniform.surface_size.x - 800.0).abs() < TOLERANCE);
        assert!((uniform.surface_size.y - 600.0).abs() < TOLERANCE);
    }

    #[test]
    fn is_antimeridian_wrap_is_true_only_when_the_viewport_crosses_the_seam() {
        let seam: f64 = std::f64::consts::PI;

        let within_one_world: Viewport = Viewport {
            min: ProjectedPoint { x: -seam + 0.2, y: -1.0 },
            max: ProjectedPoint { x: seam - 0.2, y: 1.0 },
        };
        assert!(!is_antimeridian_wrap(within_one_world));

        let past_west_edge: Viewport = Viewport {
            min: ProjectedPoint { x: -seam - 0.2, y: -1.0 },
            max: ProjectedPoint { x: -0.2, y: 1.0 },
        };
        assert!(is_antimeridian_wrap(past_west_edge));

        let past_east_edge: Viewport = Viewport {
            min: ProjectedPoint { x: 0.2, y: -1.0 },
            max: ProjectedPoint { x: seam + 0.2, y: 1.0 },
        };
        assert!(is_antimeridian_wrap(past_east_edge));
    }
}

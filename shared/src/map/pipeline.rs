use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferAddress, BufferBindingType,
    BufferSize, ColorTargetState, ColorWrites, Device, ErrorFilter, ErrorScopeGuard, FragmentState, FrontFace,
    MultisampleState, PipelineCompilationOptions, PipelineLayout, PipelineLayoutDescriptor, PolygonMode,
    PrimitiveState, PrimitiveTopology, RenderPipeline, RenderPipelineDescriptor, ShaderModule, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, TextureFormat, VertexAttribute, VertexBufferLayout, VertexState, VertexStepMode,
};

use crate::error::AppError;
use crate::map::gpu_types::{CountryState, FillVertexAttributes, EmphasisVertexAttributes, ProjectedVertexAttributes, ViewportUniform, COUNTRY_STATE_CAP};

/// The compiled pipelines the renderer draws through, built against a known surface format and so
/// (re)created at attach time once that format is available.
pub struct RenderPipelines {
    /// Outlines each country as line segments.
    pub border: RenderPipeline,
    /// Paints the choropleth triangles.
    pub fill: RenderPipeline,
    /// The selected/hovered country's triangles, inflated and painted black, drawn behind its fill so
    /// only the extra rim shows as a uniform outline.
    pub outline: RenderPipeline,
}

impl RenderPipelines {
    pub async fn create(
        device: &Device,
        surface_format: TextureFormat,
        viewport_bind_group_layout: &BindGroupLayout,
    ) -> Result<RenderPipelines, AppError> {
        let error_scopes: [ErrorScopeGuard; 3] = [
            device.push_error_scope(ErrorFilter::OutOfMemory),
            device.push_error_scope(ErrorFilter::Internal),
            device.push_error_scope(ErrorFilter::Validation),
        ];

        let pipeline_layout: PipelineLayout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("eafora-map-pipeline-layout"),
            bind_group_layouts: &[Some(viewport_bind_group_layout)],
            immediate_size: 0,
        });
        let shader_module: ShaderModule = create_map_shader_module(device);
        let border: RenderPipeline = create_border_pipeline(device, &shader_module, &pipeline_layout, surface_format);
        let fill: RenderPipeline =
            create_triangle_pipeline(device, &shader_module, &pipeline_layout, surface_format, "eafora-fill-pipeline", "fill_vertex_main", "fill_fragment_main");
        let outline: RenderPipeline =
            create_triangle_pipeline(device, &shader_module, &pipeline_layout, surface_format, "eafora-outline-pipeline", "outline_vertex_main", "outline_fragment_main");

        if let Some(error) = drain_error_scopes(error_scopes).await {
            return Err(AppError::from(format!("building the render pipelines failed: {error}")));
        }

        Ok(RenderPipelines { border, fill, outline })
    }
}

/// Pops all three OOM/Internal/Validation scopes innermost-first (the reverse of the push order the
/// scope stack requires) and returns the first error captured. It deliberately drains every scope
/// rather than short-circuiting on the first error: leaving a scope un-popped unbalances the device's
/// error-scope stack.
async fn drain_error_scopes(error_scopes: [ErrorScopeGuard; 3]) -> Option<wgpu::Error> {
    let mut first_error: Option<wgpu::Error> = None;

    for error_scope in error_scopes.into_iter().rev() {
        let error: Option<wgpu::Error> = error_scope.pop().await;
        first_error = first_error.or(error);
    }

    first_error
}

fn create_map_shader_module(device: &Device) -> ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor {
        label: Some("eafora-map-shader-module"),
        source: ShaderSource::Wgsl(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/map/map.wgsl")).into(),
        ),
    })
}

pub(crate) fn create_map_bind_group_layout(device: &Device) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("eafora-map-bind-group-layout"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<ViewportUniform>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new((COUNTRY_STATE_CAP * std::mem::size_of::<CountryState>()) as u64),
                },
                count: None,
            },
        ],
    })
}

fn create_border_pipeline(
    device: &Device,
    shader_module: &ShaderModule,
    pipeline_layout: &PipelineLayout,
    surface_format: TextureFormat,
) -> RenderPipeline {
    let position_attributes: [VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
    let emphasis_attributes: [VertexAttribute; 2] = wgpu::vertex_attr_array![2 => Float32x2, 3 => Uint32];
    let vertex_buffers: [Option<VertexBufferLayout>; 2] = [
        Some(VertexBufferLayout {
            array_stride: std::mem::size_of::<ProjectedVertexAttributes>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &position_attributes,
        }),
        Some(VertexBufferLayout {
            array_stride: std::mem::size_of::<EmphasisVertexAttributes>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &emphasis_attributes,
        }),
    ];

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("eafora-border-pipeline"),
        layout: Some(pipeline_layout),
        vertex: VertexState {
            module: shader_module,
            entry_point: Some("border_vertex_main"),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &vertex_buffers,
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::LineList,
            strip_index_format: None,
            front_face: FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        // No MSAA in v1; revisit to anti-alias the jagged border and coastline edges.
        multisample: MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(FragmentState {
            module: shader_module,
            entry_point: Some("border_fragment_main"),
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[Some(ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_triangle_pipeline(
    device: &Device,
    shader_module: &ShaderModule,
    pipeline_layout: &PipelineLayout,
    surface_format: TextureFormat,
    label: &str,
    vertex_entry_point: &str,
    fragment_entry_point: &str,
) -> RenderPipeline {
    // Position, color, and emphasis are separate vertex buffers, not interleaved: positions are static
    // (uploaded once); colors are rebuilt when the active statistic or period changes; the emphasis
    // buffer (per-vertex boundary outward-direction + country index) is static. Keeping them apart lets the color
    // buffer be replaced without re-uploading geometry, and lets the border pipeline reuse the position
    // and emphasis buffers without the colors. The fill and outline pipelines share this layout; the
    // outline reads the color attribute's buffer too but ignores it (its fragment shader is constant).
    let position_attributes: [VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
    let color_attributes: [VertexAttribute; 1] = wgpu::vertex_attr_array![1 => Float32x4];
    let emphasis_attributes: [VertexAttribute; 2] = wgpu::vertex_attr_array![2 => Float32x2, 3 => Uint32];
    let vertex_buffers: [Option<VertexBufferLayout>; 3] = [
        Some(VertexBufferLayout {
            array_stride: std::mem::size_of::<ProjectedVertexAttributes>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &position_attributes,
        }),
        Some(VertexBufferLayout {
            array_stride: std::mem::size_of::<FillVertexAttributes>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &color_attributes,
        }),
        Some(VertexBufferLayout {
            array_stride: std::mem::size_of::<EmphasisVertexAttributes>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &emphasis_attributes,
        }),
    ];

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: VertexState {
            module: shader_module,
            entry_point: Some(vertex_entry_point),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &vertex_buffers,
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        // No MSAA in v1; revisit to anti-alias the jagged border and coastline edges.
        multisample: MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(FragmentState {
            module: shader_module,
            entry_point: Some(fragment_entry_point),
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[Some(ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use wgpu::{
        Adapter, Device, DeviceDescriptor, ExperimentalFeatures, Features, Instance, Limits, MemoryHints,
        PowerPreference, Queue, RequestAdapterOptions, TextureFormat, Trace,
    };

    use super::RenderPipelines;

    async fn headless_device() -> (Device, Queue) {
        let instance: Instance = Instance::default();
        let adapter: Adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .expect("a GPU adapter is available");

        adapter
            .request_device(&DeviceDescriptor {
                label: Some("eafora-test-device"),
                required_features: Features::empty(),
                required_limits: Limits::downlevel_webgl2_defaults(),
                experimental_features: ExperimentalFeatures::default(),
                memory_hints: MemoryHints::Performance,
                trace: Trace::Off,
            })
            .await
            .expect("a GPU device is available")
    }

    #[tokio::test]
    #[ignore = "needs a GPU adapter; run with `cargo test -p shared --features render -- --ignored`"]
    async fn render_pipelines_compile_against_a_headless_device() {
        let (device, _queue): (Device, Queue) = headless_device().await;
        let viewport_bind_group_layout: wgpu::BindGroupLayout = super::create_map_bind_group_layout(&device);

        let _pipelines: RenderPipelines = RenderPipelines::create(&device, TextureFormat::Bgra8UnormSrgb, &viewport_bind_group_layout)
            .await
            .expect("pipelines build");
    }
}

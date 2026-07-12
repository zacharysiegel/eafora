use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferAddress, BufferBindingType,
    ColorTargetState, ColorWrites, Device, FragmentState, FrontFace, MultisampleState, PipelineCompilationOptions,
    PipelineLayout, PipelineLayoutDescriptor, PolygonMode, PrimitiveState, PrimitiveTopology, RenderPipeline,
    RenderPipelineDescriptor, ShaderModule, ShaderModuleDescriptor, ShaderSource, ShaderStages, TextureFormat,
    VertexAttribute, VertexBufferLayout, VertexState, VertexStepMode,
};

use crate::render::gpu_types::{FillVertex, ProjectedVertex};

/// The compiled pipelines the renderer draws through, built against a known surface format and so
/// (re)created at attach time once that format is available.
pub struct RenderPipelines {
    /// Outlines each country as line segments.
    pub border: RenderPipeline,
    /// Paints the choropleth triangles.
    pub fill: RenderPipeline,
    /// The layout of the viewport uniform binding, retained so the renderer can build the matching
    /// bind group once the viewport buffer exists.
    pub viewport_bind_group_layout: BindGroupLayout,
}

impl RenderPipelines {
    pub fn create(device: &Device, surface_format: TextureFormat) -> RenderPipelines {
        let viewport_bind_group_layout: BindGroupLayout = create_viewport_bind_group_layout(device);
        let pipeline_layout: PipelineLayout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("eafora-map-pipeline-layout"),
            bind_group_layouts: &[Some(&viewport_bind_group_layout)],
            immediate_size: 0,
        });

        let shader_module: ShaderModule = create_map_shader_module(device);

        let border: RenderPipeline = create_border_pipeline(device, &shader_module, &pipeline_layout, surface_format);
        let fill: RenderPipeline = create_fill_pipeline(device, &shader_module, &pipeline_layout, surface_format);

        RenderPipelines { border, fill, viewport_bind_group_layout }
    }
}

fn create_map_shader_module(device: &Device) -> ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor {
        label: Some("eafora-map-shader-module"),
        source: ShaderSource::Wgsl(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/render/shaders/map.wgsl")).into(),
        ),
    })
}

fn create_viewport_bind_group_layout(device: &Device) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("eafora-viewport-bind-group-layout"),
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn create_border_pipeline(
    device: &Device,
    shader_module: &ShaderModule,
    pipeline_layout: &PipelineLayout,
    surface_format: TextureFormat,
) -> RenderPipeline {
    let position_attributes: [VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
    let vertex_buffers: [Option<VertexBufferLayout>; 1] = [Some(VertexBufferLayout {
        array_stride: std::mem::size_of::<ProjectedVertex>() as BufferAddress,
        step_mode: VertexStepMode::Vertex,
        attributes: &position_attributes,
    })];

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("eafora-border-pipeline"),
        layout: Some(pipeline_layout),
        vertex: VertexState {
            module: shader_module,
            entry_point: Some("border_vs_main"),
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
        multisample: MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(FragmentState {
            module: shader_module,
            entry_point: Some("border_fs_main"),
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

fn create_fill_pipeline(
    device: &Device,
    shader_module: &ShaderModule,
    pipeline_layout: &PipelineLayout,
    surface_format: TextureFormat,
) -> RenderPipeline {
    let position_attributes: [VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
    let color_attributes: [VertexAttribute; 1] = wgpu::vertex_attr_array![1 => Float32x4];
    let vertex_buffers: [Option<VertexBufferLayout>; 2] = [
        Some(VertexBufferLayout {
            array_stride: std::mem::size_of::<ProjectedVertex>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &position_attributes,
        }),
        Some(VertexBufferLayout {
            array_stride: std::mem::size_of::<FillVertex>() as BufferAddress,
            step_mode: VertexStepMode::Vertex,
            attributes: &color_attributes,
        }),
    ];

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("eafora-fill-pipeline"),
        layout: Some(pipeline_layout),
        vertex: VertexState {
            module: shader_module,
            entry_point: Some("fill_vs_main"),
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
        multisample: MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(FragmentState {
            module: shader_module,
            entry_point: Some("fill_fs_main"),
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

        let _pipelines: RenderPipelines = RenderPipelines::create(&device, TextureFormat::Bgra8UnormSrgb);
    }
}

use crate::render::vertex::{FillVertex, MapVertex};

/// The compiled pipelines the renderer draws through: `borders` outlines each country as line
/// segments, `fills` paints the choropleth triangles. Built against a known surface format, so they
/// are (re)created at attach time when that format is available.
pub struct RenderPipelines {
    pub borders: wgpu::RenderPipeline,
    pub fills: wgpu::RenderPipeline,
    pub viewport_bind_group_layout: wgpu::BindGroupLayout,
}

impl RenderPipelines {
    pub fn create(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> RenderPipelines {
        let viewport_bind_group_layout: wgpu::BindGroupLayout = create_viewport_bind_group_layout(device);
        let pipeline_layout: wgpu::PipelineLayout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("eafora-map-pipeline-layout"),
            bind_group_layouts: &[Some(&viewport_bind_group_layout)],
            immediate_size: 0,
        });

        let borders: wgpu::RenderPipeline = create_borders_pipeline(device, &pipeline_layout, surface_format);
        let fills: wgpu::RenderPipeline = create_fills_pipeline(device, &pipeline_layout, surface_format);

        RenderPipelines { borders, fills, viewport_bind_group_layout }
    }
}

fn create_viewport_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("eafora-viewport-bind-group-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn create_borders_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    surface_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader: wgpu::ShaderModule = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("eafora-borders-shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/render/shaders/borders.wgsl")).into(),
        ),
    });

    let position_attributes: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
    let vertex_buffers: [Option<wgpu::VertexBufferLayout>; 1] = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MapVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &position_attributes,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("eafora-borders-pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &vertex_buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_fills_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    surface_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader: wgpu::ShaderModule = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("eafora-fills-shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/render/shaders/fills.wgsl")).into(),
        ),
    });

    let position_attributes: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
    let color_attributes: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![1 => Float32x4];
    let vertex_buffers: [Option<wgpu::VertexBufferLayout>; 2] = [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MapVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &position_attributes,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FillVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &color_attributes,
        }),
    ];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("eafora-fills-pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &vertex_buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn headless_device() -> (wgpu::Device, wgpu::Queue) {
        let instance: wgpu::Instance = wgpu::Instance::default();
        let adapter: wgpu::Adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .expect("a GPU adapter is available");

        adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("eafora-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("a GPU device is available")
    }

    #[tokio::test]
    #[ignore = "needs a GPU adapter; run with `cargo test -p shared --features render -- --ignored`"]
    async fn render_pipelines_compile_against_a_headless_device() {
        let (device, _queue): (wgpu::Device, wgpu::Queue) = headless_device().await;

        let _pipelines: RenderPipelines = RenderPipelines::create(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
    }
}

mod text;
mod vertex;
pub use text::TextSpec;
use text::TextState;
use vertex::Uniform;
pub use vertex::Vertex;

use lyon_tessellation::VertexBuffers;
use wayland_client::Proxy;
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::protocol::wl_surface::WlSurface;
use wgpu::util::DeviceExt;

const INITIAL_BUFFER_SIZE: u64 = 4096;
const RENDER_SCALE: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Vulkan,
    OpenGL,
}

impl std::str::FromStr for Backend {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "vulkan" => Ok(Self::Vulkan),
            "opengl" | "gl" => Ok(Self::OpenGL),
            _ => Err("backend must be vulkan or opengl"),
        }
    }
}

#[derive(Debug)]
pub struct Geometry {
    vertex_buffers: VertexBuffers<Vertex, u32>,
}

impl Geometry {
    pub fn new(vertex_buffers: VertexBuffers<Vertex, u32>) -> Self {
        Self { vertex_buffers }
    }

    pub fn empty() -> Self {
        Self::new(VertexBuffers::new())
    }

    pub fn append(&mut self, other: Self) {
        let base = self.vertex_buffers.vertices.len() as u32;
        self.vertex_buffers
            .vertices
            .extend(other.vertex_buffers.vertices);
        self.vertex_buffers.indices.extend(
            other
                .vertex_buffers
                .indices
                .into_iter()
                .map(|index| index + base),
        );
    }

    pub fn translated(&self, offset: [f32; 2]) -> Self {
        Self::new(VertexBuffers {
            vertices: self
                .vertex_buffers
                .vertices
                .iter()
                .map(|vertex| Vertex {
                    position: [
                        vertex.position[0] + offset[0],
                        vertex.position[1] + offset[1],
                    ],
                    ..*vertex
                })
                .collect(),
            indices: self.vertex_buffers.indices.clone(),
        })
    }

    fn is_empty(&self) -> bool {
        self.vertex_buffers.indices.is_empty()
    }
}

struct GrowingBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

struct RenderTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl RenderTarget {
    fn new(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("supersampled annotations"),
            size: wgpu::Extent3d {
                width: config.width * RENDER_SCALE,
                height: config.height * RENDER_SCALE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("supersampled annotations"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Self {
            _texture: texture,
            view,
            bind_group,
        }
    }
}

impl GrowingBuffer {
    fn new(device: &wgpu::Device, usage: wgpu::BufferUsages, label: &'static str) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: INITIAL_BUFFER_SIZE,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity: INITIAL_BUFFER_SIZE,
            usage,
            label,
        }
    }

    fn ensure_capacity(&mut self, device: &wgpu::Device, required: u64) {
        if required <= self.capacity {
            return;
        }
        self.capacity = required.next_power_of_two().max(INITIAL_BUFFER_SIZE);
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: self.capacity,
            usage: self.usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }
}

pub struct WgpuState {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    render_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    downsample_layout: wgpu::BindGroupLayout,
    downsample_sampler: wgpu::Sampler,
    render_target: Option<RenderTarget>,
    committed_vertices: GrowingBuffer,
    committed_indices: GrowingBuffer,
    committed_index_count: u32,
    preview_vertices: GrowingBuffer,
    preview_indices: GrowingBuffer,
    packed_vertices: Vec<Vertex>,
    packed_indices: Vec<u32>,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    text: Option<TextState>,
}

impl WgpuState {
    pub fn new(
        display: &WlDisplay,
        surface: &WlSurface,
        width: u32,
        height: u32,
        force_backend: Option<Backend>,
    ) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: match force_backend {
                Some(Backend::Vulkan) => wgpu::Backends::VULKAN,
                Some(Backend::OpenGL) => wgpu::Backends::GL,
                None => wgpu::Backends::all(),
            },
            ..Default::default()
        });

        let raw_display_handle =
            wgpu::rwh::RawDisplayHandle::Wayland(wgpu::rwh::WaylandDisplayHandle::new(
                std::ptr::NonNull::new(display.id().as_ptr() as *mut _).unwrap(),
            ));
        let raw_window_handle =
            wgpu::rwh::RawWindowHandle::Wayland(wgpu::rwh::WaylandWindowHandle::new(
                std::ptr::NonNull::new(surface.id().as_ptr() as *mut _).unwrap(),
            ));
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .unwrap();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .unwrap();
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .find(|format| format.is_srgb())
            .copied()
            .unwrap_or(capabilities.formats[0]);
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .find(|mode| matches!(mode, wgpu::CompositeAlphaMode::PreMultiplied))
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .unwrap();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let uniform = Uniform {
            screen_size: [width as f32, height as f32],
        };
        let screen_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("screen dimensions"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen dimensions"),
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
        });
        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen dimensions"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("annotations.wgsl"));
        let shader_constants = &[("target_is_srgb", format.is_srgb() as u8 as f64)];
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("annotation pipeline"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("annotation pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: shader_constants,
                    ..Default::default()
                },
                buffers: &[Vertex::DESC],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let downsample_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("supersampled annotations"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let downsample_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("downsample sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let downsample_shader = device.create_shader_module(wgpu::include_wgsl!("downsample.wgsl"));
        let downsample_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("downsample pipeline"),
                bind_group_layouts: &[&downsample_layout],
                immediate_size: 0,
            });
        let downsample_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("downsample pipeline"),
            layout: Some(&downsample_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &downsample_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &downsample_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            surface,
            surface_config,
            committed_vertices: GrowingBuffer::new(
                &device,
                wgpu::BufferUsages::VERTEX,
                "committed annotation vertices",
            ),
            committed_indices: GrowingBuffer::new(
                &device,
                wgpu::BufferUsages::INDEX,
                "committed annotation indices",
            ),
            preview_vertices: GrowingBuffer::new(
                &device,
                wgpu::BufferUsages::VERTEX,
                "preview vertices",
            ),
            preview_indices: GrowingBuffer::new(
                &device,
                wgpu::BufferUsages::INDEX,
                "preview indices",
            ),
            committed_index_count: 0,
            packed_vertices: Vec::new(),
            packed_indices: Vec::new(),
            render_pipeline,
            downsample_pipeline,
            downsample_layout,
            downsample_sampler,
            render_target: None,
            screen_buffer,
            screen_bind_group,
            text: None,
            device,
            queue,
        }
    }

    pub fn upload_committed<'a>(&mut self, geometries: impl IntoIterator<Item = &'a Geometry>) {
        self.committed_index_count = Self::upload_geometry(
            &self.device,
            &self.queue,
            &mut self.committed_vertices,
            &mut self.committed_indices,
            geometries,
            &mut self.packed_vertices,
            &mut self.packed_indices,
        );
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0
            || height == 0
            || (width == self.surface_config.width && height == self.surface_config.height)
        {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.render_target = None;
        self.queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::bytes_of(&Uniform {
                screen_size: [width as f32, height as f32],
            }),
        );
    }

    pub fn prepare_text(&mut self, text_specs: &[TextSpec<'_>]) {
        // Font discovery can be relatively slow, so do it before acquiring a swapchain image.
        if !text_specs.is_empty() && self.text.is_none() {
            self.text = Some(TextState::new(
                &self.device,
                &self.queue,
                self.surface_config.format,
            ));
        }
        if let Some(text) = &mut self.text {
            text.prepare(
                &self.device,
                &self.queue,
                self.surface_config.width,
                self.surface_config.height,
                text_specs,
            );
        }
    }

    pub fn text_layout_size(&self, key: u64) -> Option<[f32; 2]> {
        self.text.as_ref()?.layout_size(key)
    }

    pub fn text_cursor_x(&mut self, key: u64, index: usize) -> Option<f32> {
        self.text.as_mut()?.cursor_x(key, index)
    }

    pub fn render(&mut self, previews: &[Geometry], overlays: &[Geometry]) -> bool {
        let preview_index_count = previews
            .iter()
            .map(|geometry| geometry.vertex_buffers.indices.len() as u32)
            .sum();
        let total_index_count = Self::upload_geometry(
            &self.device,
            &self.queue,
            &mut self.preview_vertices,
            &mut self.preview_indices,
            previews.iter().chain(overlays),
            &mut self.packed_vertices,
            &mut self.packed_indices,
        );

        self.render_surface(preview_index_count, total_index_count - preview_index_count)
    }

    fn render_geometry(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &'static str,
        committed: bool,
        indices: std::ops::Range<u32>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.render_target.as_ref().unwrap().view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.render_pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        if committed {
            draw_buffer(
                &mut pass,
                &self.committed_vertices.buffer,
                &self.committed_indices.buffer,
                0..self.committed_index_count,
            );
        }
        draw_buffer(
            &mut pass,
            &self.preview_vertices.buffer,
            &self.preview_indices.buffer,
            indices,
        );
    }

    fn downsample(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        label: &'static str,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.downsample_pipeline);
        pass.set_bind_group(0, &self.render_target.as_ref().unwrap().bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn render_surface(&mut self, preview_index_count: u32, overlay_index_count: u32) -> bool {
        if self.render_target.is_none() {
            self.render_target = Some(RenderTarget::new(
                &self.device,
                &self.surface_config,
                &self.downsample_layout,
                &self.downsample_sampler,
            ));
        }
        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    Ok(output) => output,
                    Err(error) => {
                        eprintln!("vellum: surface retry failed: {error}");
                        return false;
                    }
                }
            }
            Err(wgpu::SurfaceError::Timeout) => return false,
            Err(error) => panic!("surface error: {error}"),
        };

        let swapchain_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.render_geometry(&mut encoder, "annotations", true, 0..preview_index_count);
        self.downsample(
            &mut encoder,
            &swapchain_view,
            "downsample annotations",
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        if let Some(text) = &self.text {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("annotation text"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &swapchain_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            text.render(&mut pass);
        }
        if overlay_index_count != 0 {
            self.render_geometry(
                &mut encoder,
                "editor overlay",
                false,
                preview_index_count..preview_index_count + overlay_index_count,
            );
            self.downsample(
                &mut encoder,
                &swapchain_view,
                "downsample editor overlay",
                wgpu::LoadOp::Load,
            );
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
        if let Some(text) = &mut self.text {
            text.trim();
        }
        true
    }

    pub fn release_render_target(&mut self) {
        self.render_target = None;
    }

    fn upload_geometry<'a>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &mut GrowingBuffer,
        indices: &mut GrowingBuffer,
        geometries: impl IntoIterator<Item = &'a Geometry>,
        packed_vertices: &mut Vec<Vertex>,
        packed_indices: &mut Vec<u32>,
    ) -> u32 {
        packed_vertices.clear();
        packed_indices.clear();
        for geometry in geometries
            .into_iter()
            .filter(|geometry| !geometry.is_empty())
        {
            let buffers = &geometry.vertex_buffers;
            let base_vertex = packed_vertices.len() as u32;
            packed_vertices.extend_from_slice(&buffers.vertices);
            packed_indices.extend(buffers.indices.iter().map(|index| index + base_vertex));
        }
        vertices.ensure_capacity(
            device,
            (packed_vertices.len() * std::mem::size_of::<Vertex>()).max(1) as u64,
        );
        indices.ensure_capacity(
            device,
            (packed_indices.len() * std::mem::size_of::<u32>()).max(1) as u64,
        );
        if !packed_vertices.is_empty() {
            queue.write_buffer(&vertices.buffer, 0, bytemuck::cast_slice(packed_vertices));
            queue.write_buffer(&indices.buffer, 0, bytemuck::cast_slice(packed_indices));
        }
        packed_indices.len() as u32
    }
}

fn draw_buffer<'pass>(
    pass: &mut wgpu::RenderPass<'pass>,
    vertices: &'pass wgpu::Buffer,
    indices: &'pass wgpu::Buffer,
    indices_range: std::ops::Range<u32>,
) {
    if indices_range.is_empty() {
        return;
    }
    pass.set_vertex_buffer(0, vertices.slice(..));
    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(indices_range, 0, 0..1);
}

mod text;
mod vertex;
pub use text::TextSpec;
use text::TextState;
use vertex::Uniform;
pub use vertex::Vertex;

use crate::cli::Backend;
use lyon_tessellation::VertexBuffers;
use wayland_client::Proxy;
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::protocol::wl_surface::WlSurface;
use wgpu::util::DeviceExt;

const INITIAL_BUFFER_SIZE: u64 = 4096;
const RENDER_SCALE: u32 = 2;
const PICKER_RENDER_SCALE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ResolveInfo {
    origin: [f32; 2],
    _padding: [f32; 2],
}

#[derive(Debug)]
pub struct Geometry {
    vertex_buffers: VertexBuffers<Vertex, u32>,
}

pub struct LocalGeometry {
    geometry: Geometry,
    origin: [f32; 2],
    size: [u32; 2],
}

impl LocalGeometry {
    pub fn new(geometry: Geometry, origin: [f32; 2], size: [u32; 2]) -> Self {
        Self {
            geometry,
            origin,
            size,
        }
    }
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
    resolve_buffer: wgpu::Buffer,
}

struct PickerRenderTarget {
    target: RenderTarget,
    _screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    size: [u32; 2],
    origin: [f32; 2],
}

impl RenderTarget {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        scale: u32,
        origin: [f32; 2],
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("supersampled annotations"),
            size: wgpu::Extent3d {
                width: size[0] * scale,
                height: size[1] * scale,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let resolve_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("downsample dimensions"),
            contents: bytemuck::bytes_of(&ResolveInfo {
                origin,
                _padding: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: resolve_buffer.as_entire_binding(),
                },
            ],
        });
        Self {
            _texture: texture,
            view,
            bind_group,
            resolve_buffer,
        }
    }
}

impl PickerRenderTarget {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        origin: [f32; 2],
        screen_layout: &wgpu::BindGroupLayout,
        downsample_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let screen_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("picker dimensions"),
            contents: bytemuck::bytes_of(&Uniform {
                screen_size: [size[0] as f32, size[1] as f32],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("picker dimensions"),
            layout: screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        Self {
            target: RenderTarget::new(
                device,
                format,
                size,
                PICKER_RENDER_SCALE,
                origin,
                downsample_layout,
                sampler,
            ),
            _screen_buffer: screen_buffer,
            screen_bind_group,
            size,
            origin,
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
    picker_downsample_pipeline: wgpu::RenderPipeline,
    downsample_layout: wgpu::BindGroupLayout,
    downsample_sampler: wgpu::Sampler,
    screen_layout: wgpu::BindGroupLayout,
    render_target: Option<RenderTarget>,
    picker_render_target: Option<PickerRenderTarget>,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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
        let downsample_pipeline = create_downsample_pipeline(
            &device,
            &downsample_pipeline_layout,
            &downsample_shader,
            format,
            false,
            RENDER_SCALE,
        );
        let picker_downsample_pipeline = create_downsample_pipeline(
            &device,
            &downsample_pipeline_layout,
            &downsample_shader,
            format,
            true,
            PICKER_RENDER_SCALE,
        );
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
            picker_downsample_pipeline,
            downsample_layout,
            downsample_sampler,
            screen_layout: bind_group_layout,
            render_target: None,
            picker_render_target: None,
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

    pub fn render(&mut self, previews: &[Geometry], picker: Option<&LocalGeometry>) -> bool {
        let preview_index_count = previews
            .iter()
            .map(|geometry| geometry.vertex_buffers.indices.len() as u32)
            .sum();
        let total_index_count = Self::upload_geometry(
            &self.device,
            &self.queue,
            &mut self.preview_vertices,
            &mut self.preview_indices,
            previews
                .iter()
                .chain(picker.into_iter().map(|picker| &picker.geometry)),
            &mut self.packed_vertices,
            &mut self.packed_indices,
        );

        self.render_surface(
            preview_index_count,
            total_index_count - preview_index_count,
            picker,
        )
    }

    fn render_geometry(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &'static str,
        target: &wgpu::TextureView,
        screen_bind_group: &wgpu::BindGroup,
        committed: bool,
        indices: std::ops::Range<u32>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
        pass.set_bind_group(0, screen_bind_group, &[]);
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
        load: wgpu::LoadOp<wgpu::Color>,
        source: &RenderTarget,
        viewport: Option<[f32; 4]>,
        exact: bool,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(if exact {
                "downsample picker"
            } else {
                "downsample geometry"
            }),
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
        pass.set_pipeline(if exact {
            &self.picker_downsample_pipeline
        } else {
            &self.downsample_pipeline
        });
        if let Some([x, y, width, height]) = viewport {
            pass.set_viewport(x, y, width, height, 0.0, 1.0);
        }
        pass.set_bind_group(0, &source.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn render_surface(
        &mut self,
        preview_index_count: u32,
        picker_index_count: u32,
        picker: Option<&LocalGeometry>,
    ) -> bool {
        if self.render_target.is_none() {
            self.render_target = Some(RenderTarget::new(
                &self.device,
                self.surface_config.format,
                [self.surface_config.width, self.surface_config.height],
                RENDER_SCALE,
                [0.0; 2],
                &self.downsample_layout,
                &self.downsample_sampler,
            ));
        }
        if let Some(picker) = picker
            && self
                .picker_render_target
                .as_ref()
                .is_none_or(|target| target.size != picker.size)
        {
            self.picker_render_target = Some(PickerRenderTarget::new(
                &self.device,
                self.surface_config.format,
                picker.size,
                picker.origin,
                &self.screen_layout,
                &self.downsample_layout,
                &self.downsample_sampler,
            ));
        }
        if let Some(picker) = picker {
            let target = self.picker_render_target.as_mut().unwrap();
            if target.origin != picker.origin {
                self.queue.write_buffer(
                    &target.target.resolve_buffer,
                    0,
                    bytemuck::bytes_of(&ResolveInfo {
                        origin: picker.origin,
                        _padding: [0.0; 2],
                    }),
                );
                target.origin = picker.origin;
            }
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
        let render_target = self.render_target.as_ref().unwrap();
        self.render_geometry(
            &mut encoder,
            "annotations",
            &render_target.view,
            &self.screen_bind_group,
            true,
            0..preview_index_count,
        );
        self.downsample(
            &mut encoder,
            &swapchain_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            render_target,
            None,
            false,
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
        if let Some(picker) = picker {
            let picker_target = self.picker_render_target.as_ref().unwrap();
            self.render_geometry(
                &mut encoder,
                "picker",
                &picker_target.target.view,
                &picker_target.screen_bind_group,
                false,
                preview_index_count..preview_index_count + picker_index_count,
            );
            let left = picker.origin[0].max(0.0);
            let top = picker.origin[1].max(0.0);
            let right =
                (picker.origin[0] + picker.size[0] as f32).min(self.surface_config.width as f32);
            let bottom =
                (picker.origin[1] + picker.size[1] as f32).min(self.surface_config.height as f32);
            if right > left && bottom > top {
                self.downsample(
                    &mut encoder,
                    &swapchain_view,
                    wgpu::LoadOp::Load,
                    &picker_target.target,
                    Some([left, top, right - left, bottom - top]),
                    true,
                );
            }
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
        self.picker_render_target = None;
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

fn create_downsample_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    exact: bool,
    render_scale: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if exact {
            "picker downsample pipeline"
        } else {
            "downsample pipeline"
        }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[
                    ("exact", exact as u8 as f64),
                    ("render_scale", render_scale as f64),
                ],
                ..Default::default()
            },
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
    })
}

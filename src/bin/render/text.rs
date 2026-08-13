use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use glyphon::{
    Attrs, Buffer, Cache, Color, Cursor, Edit, Editor, Family, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

pub struct TextSpec<'a> {
    pub key: u64,
    pub content: &'a str,
    pub left: f32,
    pub top: f32,
    pub size: f32,
    pub color: [f32; 4],
}

struct CachedText {
    content: String,
    size: f32,
    layout_size: [f32; 2],
    buffer: Buffer,
}

pub(super) struct TextState {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    buffers: HashMap<u64, CachedText>,
    prepared: u64,
}

impl TextState {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            viewport,
            atlas,
            renderer,
            buffers: HashMap::new(),
            prepared: 0,
        }
    }

    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        specs: &[TextSpec<'_>],
    ) {
        let mut hasher = DefaultHasher::new();
        (width, height).hash(&mut hasher);
        for spec in specs {
            spec.key.hash(&mut hasher);
            spec.content.hash(&mut hasher);
            for value in [spec.left, spec.top, spec.size]
                .into_iter()
                .chain(spec.color)
            {
                value.to_bits().hash(&mut hasher);
            }
        }
        let prepared = hasher.finish();
        if self.prepared == prepared {
            return;
        }

        self.viewport.update(queue, Resolution { width, height });
        self.buffers
            .retain(|key, _| specs.iter().any(|spec| spec.key == *key));

        for spec in specs {
            let stale = self
                .buffers
                .get(&spec.key)
                .is_none_or(|cached| cached.content != spec.content || cached.size != spec.size);
            if stale {
                let mut buffer = Buffer::new(
                    &mut self.font_system,
                    Metrics::new(spec.size, spec.size * 1.25),
                );
                buffer.set_wrap(&mut self.font_system, Wrap::None);
                buffer.set_size(&mut self.font_system, None, Some(spec.size * 1.25));
                buffer.set_text(
                    &mut self.font_system,
                    spec.content,
                    &Attrs::new().family(Family::SansSerif),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);
                let layout_width = buffer
                    .layout_runs()
                    .fold(0.0_f32, |width, run| width.max(run.line_w));
                self.buffers.insert(
                    spec.key,
                    CachedText {
                        content: spec.content.to_owned(),
                        size: spec.size,
                        layout_size: [layout_width, spec.size * 1.25],
                        buffer,
                    },
                );
            }
        }

        let areas = specs.iter().filter_map(|spec| {
            self.buffers.get(&spec.key).map(|cached| TextArea {
                buffer: &cached.buffer,
                left: spec.left,
                top: spec.top,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: color(spec.color),
                custom_glyphs: &[],
            })
        });
        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash_cache,
            )
            .expect("prepare annotation text");
        self.prepared = prepared;
    }

    pub(super) fn layout_size(&self, key: u64) -> Option<[f32; 2]> {
        self.buffers.get(&key).map(|cached| cached.layout_size)
    }

    pub(super) fn cursor_x(&mut self, key: u64, index: usize) -> Option<f32> {
        let cached = self.buffers.get_mut(&key)?;
        let mut editor = Editor::new(&mut cached.buffer);
        editor.set_cursor(Cursor::new(0, index));
        editor.cursor_position().map(|(x, _)| x as f32)
    }

    pub(super) fn render<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        self.renderer
            .render(&self.atlas, &self.viewport, pass)
            .expect("render annotation text");
    }

    pub(super) fn trim(&mut self) {
        self.atlas.trim();
    }
}

fn color(color: [f32; 4]) -> Color {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::rgba(
        channel(color[0]),
        channel(color[1]),
        channel(color[2]),
        channel(color[3]),
    )
}

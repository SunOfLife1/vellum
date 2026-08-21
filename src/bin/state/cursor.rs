use std::fs::File;
use std::io::{self, Write};
use std::os::fd::AsFd;

use rustix::fs::{MemfdFlags, memfd_create};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_shm::{Format, WlShm};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Proxy, QueueHandle};

use super::State;
use super::draw::{Tool, ToolCursor, eraser_radius};

const SUPERSAMPLE: usize = 4;

pub(super) struct CursorSurface {
    surface: WlSurface,
    buffer: Option<WlBuffer>,
    preview: Option<ToolCursor>,
    hotspot: [i32; 2],
}

impl CursorSurface {
    pub(super) fn new(compositor: &WlCompositor, qhandle: &QueueHandle<State>) -> Self {
        Self {
            surface: compositor.create_surface(qhandle, ()),
            buffer: None,
            preview: None,
            hotspot: [0; 2],
        }
    }

    pub(super) fn surface(&self) -> &WlSurface {
        &self.surface
    }

    pub(super) fn hotspot(&self) -> [i32; 2] {
        self.hotspot
    }

    pub(super) fn update(
        &mut self,
        preview: ToolCursor,
        shm: &WlShm,
        qhandle: &QueueHandle<State>,
    ) -> io::Result<()> {
        if self.preview == Some(preview) {
            return Ok(());
        }
        let image = render(preview);
        let fd = memfd_create(c"vellum-cursor", MemfdFlags::CLOEXEC)?;
        let mut file = File::from(fd);
        file.write_all(&image.pixels)?;
        file.flush()?;

        let byte_len = i32::try_from(image.pixels.len())
            .map_err(|_| io::Error::other("cursor buffer is too large"))?;
        let width = i32::try_from(image.width)
            .map_err(|_| io::Error::other("cursor width is too large"))?;
        let height = i32::try_from(image.height)
            .map_err(|_| io::Error::other("cursor height is too large"))?;
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| io::Error::other("cursor stride overflow"))?;
        let pool = shm.create_pool(file.as_fd(), byte_len, qhandle, ());
        let buffer = pool.create_buffer(0, width, height, stride, Format::Argb8888, qhandle, ());
        pool.destroy();

        self.surface.attach(Some(&buffer), 0, 0);
        if self.surface.version() >= 4 {
            self.surface.damage_buffer(0, 0, width, height);
        } else {
            self.surface.damage(0, 0, width, height);
        }
        self.surface.commit();
        if let Some(previous) = self.buffer.replace(buffer) {
            previous.destroy();
        }
        self.preview = Some(preview);
        self.hotspot = image.hotspot;
        Ok(())
    }
}

impl Drop for CursorSurface {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            buffer.destroy();
        }
        self.surface.destroy();
    }
}

struct CursorImage {
    width: usize,
    height: usize,
    hotspot: [i32; 2],
    pixels: Vec<u8>,
}

fn render(preview: ToolCursor) -> CursorImage {
    let (size, radius) = match preview.tool {
        Tool::Pen => {
            let size = ((preview.width.ceil() as usize + 4).max(11)) | 1;
            (size, (f64::from(preview.width) * 0.5).max(1.0))
        }
        Tool::Eraser => {
            let radius = f64::from(eraser_radius(preview.width)).max(1.0);
            let size = ((radius * 2.0).ceil() as usize + 4).max(11) | 1;
            (size, radius)
        }
        _ => unreachable!("only pen and eraser use rendered cursors"),
    };
    let mut pen_color = preview.color;
    if preview.tool == Tool::Pen {
        // Keep low-opacity cursors perceptually distinct instead of letting them
        // disappear against the desktop before a stroke is drawn.
        pen_color[3] = pen_color[3].sqrt();
    }
    let hotspot = [size as i32 / 2; 2];
    let mut pixels = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let mut accumulated = [0.0; 4];
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let point = [
                        x as f64 + (sx as f64 + 0.5) / SUPERSAMPLE as f64 - f64::from(hotspot[0]),
                        y as f64 + (sy as f64 + 0.5) / SUPERSAMPLE as f64 - f64::from(hotspot[1]),
                    ];
                    let (distance, color) = if preview.tool == Tool::Eraser {
                        let center_distance = point[0].hypot(point[1]);
                        let color = if radius - center_distance < 0.75 {
                            [0.0, 0.0, 0.0, 1.0]
                        } else {
                            [1.0, 1.0, 1.0, 1.0]
                        };
                        (center_distance - radius, color)
                    } else {
                        (point[0].hypot(point[1]) - radius, pen_color)
                    };
                    let color = fill_color(distance, color);
                    for index in 0..4 {
                        accumulated[index] += color[index];
                    }
                }
            }
            let samples = (SUPERSAMPLE * SUPERSAMPLE) as f32;
            for value in &mut accumulated {
                *value /= samples;
            }
            let [r, g, b, a] = accumulated.map(|value| (value * 255.0).round() as u8);
            pixels.extend_from_slice(&[b, g, r, a]);
        }
    }
    CursorImage {
        width: size,
        height: size,
        hotspot,
        pixels,
    }
}

fn fill_color(distance: f64, color: [f32; 4]) -> [f32; 4] {
    if distance > 0.0 {
        return [0.0; 4];
    }
    let alpha = color[3];
    [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha]
}

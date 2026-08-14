mod editor;
mod freehand;
mod history;
mod picker;
mod scene;
mod selection;
mod text_edit;
mod tool;

use crate::render::{Geometry, TextSpec, Vertex, WgpuState};
use std::time::{Duration, Instant};

pub(crate) use self::editor::{Action, CursorMove};
use self::editor::{Damage, Editor, EditorEffect};
use self::scene::ElementKind;
pub(super) use self::scene::Point;
pub(crate) use self::selection::CursorHint;
pub(crate) use self::tool::Tool;

const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(530);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

pub struct DrawState {
    editor: Editor,
    damage: Damage,
    feedback: Option<(String, Point)>,
    property_feedback_anchor: Option<Point>,
    feedback_until: Option<Instant>,
    feedback_duration: Duration,
    caret_visible: bool,
    caret_until: Option<Instant>,
    previews: Vec<Geometry>,
    overlays: Vec<Geometry>,
}

impl DrawState {
    pub fn new(
        stroke_width: f32,
        stroke_color: crate::protocol::Rgb,
        default_tool: Tool,
        remember_last_tool: bool,
        palette: Vec<crate::protocol::Rgb>,
        feedback_duration: Duration,
    ) -> Self {
        Self {
            editor: Editor::new(
                stroke_width,
                stroke_color,
                default_tool,
                remember_last_tool,
                palette,
            ),
            damage: Damage::Scene,
            feedback: None,
            property_feedback_anchor: None,
            feedback_until: None,
            feedback_duration,
            caret_visible: true,
            caret_until: None,
            previews: Vec::new(),
            overlays: Vec::new(),
        }
    }

    pub fn activate(&mut self) -> bool {
        let damage = self.editor.activate();
        self.record(damage)
    }

    pub fn deactivate(&mut self) -> bool {
        let mut damage = self.editor.deactivate();
        if self.feedback.take().is_some()
            | self.property_feedback_anchor.take().is_some()
            | self.feedback_until.take().is_some()
        {
            damage = damage.max(Damage::Preview);
        }
        self.caret_until = None;
        self.record(damage)
    }

    pub fn is_editing_text(&self) -> bool {
        self.editor.is_editing_text()
    }

    pub fn is_drawing_pen(&self) -> bool {
        self.editor.is_drawing_pen()
    }

    pub fn handle_action(&mut self, action: Action) -> EditorEffect {
        let mut effect = self.editor.handle_action(action);
        if self.editor.is_editing_text() {
            if self.show_caret() {
                effect.damage = effect.damage.max(Damage::Preview);
            }
        } else {
            self.caret_until = None;
        }
        self.damage.merge(effect.damage);
        effect
    }

    pub fn pointer_down(
        &mut self,
        point: Point,
        modifiers: Modifiers,
        temporary_eraser: bool,
    ) -> bool {
        let damage = self.editor.pointer_down(point, modifiers, temporary_eraser);
        if self.editor.is_editing_text() {
            self.show_caret();
        } else {
            self.caret_until = None;
        }
        self.record(damage)
    }

    pub fn pointer_motion(&mut self, point: Point, modifiers: Modifiers) -> bool {
        let damage = self.editor.pointer_motion(point, modifiers);
        self.record(damage)
    }

    pub fn pointer_up(&mut self, point: Point, modifiers: Modifiers) -> bool {
        let damage = self.editor.pointer_up(point, modifiers);
        self.record(damage)
    }

    pub fn picker_active(&self) -> bool {
        self.editor.picker_active()
    }

    pub fn cursor_hint(&self, point: Point) -> CursorHint {
        self.editor.cursor_hint(point)
    }

    pub fn open_tool_picker(&mut self, center: Point) -> bool {
        let damage = self.editor.open_tool_picker(center);
        self.record(damage)
    }

    pub fn picker_motion(&mut self, point: Point) -> bool {
        let damage = self.editor.picker_motion(point);
        self.record(damage)
    }

    pub fn picker_release(&mut self, point: Point) -> bool {
        let damage = self.editor.picker_release(point);
        self.record(damage)
    }

    pub fn dismiss_picker(&mut self) -> bool {
        let damage = self.editor.dismiss_picker();
        self.record(damage)
    }

    pub fn double_click_at(&mut self, point: Point) -> bool {
        let damage = self.editor.double_click_at(point);
        if self.editor.is_editing_text() {
            self.show_caret();
        }
        self.record(damage)
    }

    pub fn adjust(&mut self, steps: f32, at: Point, modifiers: Modifiers) -> bool {
        let (damage, feedback) = if modifiers.shift && !self.editor.is_editing_text() {
            self.editor.adjust_roundness(steps)
        } else if modifiers.ctrl {
            self.editor.adjust_opacity(steps)
        } else {
            self.editor.adjust_size(steps)
        };
        if damage.changed() {
            let anchor = *self.property_feedback_anchor.get_or_insert(at);
            self.feedback = Some((feedback, anchor));
            self.feedback_until = Some(Instant::now() + self.feedback_duration);
            self.damage.merge(damage);
        }
        damage.changed()
    }

    pub fn needs_render(&self) -> bool {
        self.damage.changed()
    }

    pub fn damage_scene(&mut self) {
        self.damage.merge(Damage::Scene);
    }

    fn record(&mut self, damage: Damage) -> bool {
        self.damage.merge(damage);
        damage.changed()
    }

    pub fn next_wakeup(&self) -> Option<Instant> {
        [self.feedback_until, self.caret_until]
            .into_iter()
            .flatten()
            .min()
    }

    pub fn handle_timeouts(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if self.feedback_until.is_some_and(|until| now >= until) {
            self.feedback = None;
            self.property_feedback_anchor = None;
            self.feedback_until = None;
            changed = true;
        }
        if self.caret_until.is_some_and(|until| now >= until) {
            if self.editor.is_editing_text() {
                self.caret_visible = !self.caret_visible;
                self.caret_until = Some(now + CARET_BLINK_INTERVAL);
                changed = true;
            } else {
                self.caret_until = None;
            }
        }
        if changed {
            self.damage.merge(Damage::Preview);
        }
        changed
    }

    pub fn render(&mut self, wgpu: &mut WgpuState) {
        if !self.damage.changed() {
            return;
        }
        if self.damage == Damage::Scene {
            wgpu.upload_committed(
                self.editor
                    .elements()
                    .iter()
                    .filter(|element| !self.editor.element_is_previewed(element.id))
                    .map(|element| &element.geometry)
                    .chain(self.editor.cached_freehand_geometry()),
            );
        }

        let editing_id = self.editor.active_text().and_then(|edit| edit.id);
        let mut caret = None;
        {
            let active_text = self.editor.active_text();
            let mut text_specs = Vec::new();
            for element in self.editor.elements() {
                if Some(element.id) == editing_id {
                    continue;
                }
                let ElementKind::Text {
                    origin,
                    content,
                    font_size,
                } = &element.kind
                else {
                    continue;
                };
                let offset = self.editor.moving_offset(element.id).unwrap_or_default();
                text_specs.push(TextSpec {
                    key: element.id,
                    content,
                    left: origin.x + offset.x,
                    top: origin.y + offset.y,
                    size: *font_size,
                    color: element.style.color,
                });
            }
            if let Some(edit) = active_text {
                let key = edit.id.unwrap_or(0);
                text_specs.push(TextSpec {
                    key,
                    content: &edit.content,
                    left: edit.origin.x,
                    top: edit.origin.y,
                    size: edit.font_size,
                    color: edit.style.color,
                });
                caret = Some((key, edit.cursor, edit.origin, edit.font_size));
            }
            if let Some((content, at)) = &self.feedback {
                text_specs.push(TextSpec {
                    key: u64::MAX - 30,
                    content,
                    left: at.x + 16.0,
                    top: at.y + 16.0,
                    size: 18.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                });
            }
            wgpu.prepare_text(&text_specs);
        }
        self.editor
            .update_text_bounds(|id| wgpu.text_layout_size(id));

        self.previews.clear();
        if self.caret_visible
            && let Some((key, cursor, origin, font_size)) = caret
            && let Some(x) = wgpu.text_cursor_x(key, cursor)
        {
            self.previews
                .push(text_caret(origin.x + x, origin.y, font_size));
        }
        self.editor.append_preview_geometry(&mut self.previews);
        self.editor
            .append_selection_geometry(self.property_feedback_anchor.is_none(), &mut self.previews);
        self.overlays.clear();
        self.overlays.extend(self.editor.picker_geometry());
        if wgpu.render(&self.previews, &self.overlays) {
            self.damage = Damage::None;
        }
    }

    pub fn force_render(&mut self, wgpu: &mut WgpuState) {
        self.damage = Damage::Scene;
        self.render(wgpu);
    }

    fn show_caret(&mut self) -> bool {
        let changed = !self.caret_visible;
        self.caret_visible = true;
        self.caret_until = Some(Instant::now() + CARET_BLINK_INTERVAL);
        changed
    }
}

fn text_caret(left: f32, top: f32, font_size: f32) -> Geometry {
    let bottom = top + font_size * 1.25;
    let black = [0.0, 0.0, 0.0, 1.0];
    let white = [1.0, 1.0, 1.0, 1.0];
    Geometry::new(lyon_tessellation::VertexBuffers {
        vertices: vec![
            Vertex::at([left - 1.0, top - 1.0], black),
            Vertex::at([left + 2.0, top - 1.0], black),
            Vertex::at([left + 2.0, bottom + 1.0], black),
            Vertex::at([left - 1.0, bottom + 1.0], black),
            Vertex::at([left, top], white),
            Vertex::at([left + 1.0, top], white),
            Vertex::at([left + 1.0, bottom], white),
            Vertex::at([left, bottom], white),
        ],
        indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
    })
}

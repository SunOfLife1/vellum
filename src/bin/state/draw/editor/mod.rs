mod interaction;
mod properties;

use self::interaction::Interaction;
use self::properties::{DEFAULT_ERASER_WIDTH, DEFAULT_TEXT_SIZE, ToolPropertySet};
use super::Modifiers;
use super::history::{Entry as HistoryEntry, History};
use super::picker::{Choice, Picker, ShapeFills, choice, picker_geometry};
use super::scene::{Element, HIT_SLOP, geometry};
use super::scene::{ElementId, ElementKind, EndMarker, Point, Style};
use super::selection;
pub(crate) use super::text_edit::CursorMove;
use super::text_edit::TextEdit;
use super::tool::Tool;
use crate::render::Geometry;

const MIN_STROKE_WIDTH: f32 = 1.0;
const MAX_STROKE_WIDTH: f32 = 64.0;

pub(crate) enum Action {
    Undo,
    Redo,
    SelectAll,
    ToggleEraser,
    ToggleFill,
    Delete,
    Clear,
    Cancel,
    CommitText,
    Backspace,
    BackspaceWord,
    MoveCursor(CursorMove),
    InsertText(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Damage {
    #[default]
    None,
    Preview,
    Scene,
}

impl Damage {
    pub fn merge(&mut self, other: Self) {
        *self = (*self).max(other);
    }

    pub fn changed(self) -> bool {
        self != Self::None
    }

    fn from_preview(changed: bool) -> Self {
        if changed { Self::Preview } else { Self::None }
    }

    fn from_scene(changed: bool) -> Self {
        if changed { Self::Scene } else { Self::None }
    }
}

#[derive(Default)]
pub struct EditorEffect {
    pub damage: Damage,
    pub deactivate: bool,
    pub feedback: Option<String>,
}

pub struct Editor {
    tool: Tool,
    style: Style,
    elements: Vec<Element>,
    selected: Vec<ElementId>,
    interaction: Option<Interaction>,
    history: History,
    next_id: ElementId,
    picker: Option<Picker>,
    default_width: f32,
    default_text_size: f32,
    default_tool: Tool,
    last_non_eraser_tool: Tool,
    tool_properties: ToolPropertySet,
    remember_last_tool: bool,
    palette: Vec<[f32; 3]>,
}

impl Editor {
    pub fn new(
        width: f32,
        rgb: crate::Rgb,
        default_tool: Tool,
        remember_last_tool: bool,
        default_fill_shapes: bool,
        palette: Vec<crate::Rgb>,
    ) -> Self {
        let width = width.clamp(MIN_STROKE_WIDTH, MAX_STROKE_WIDTH);
        let opacity = 1.0;
        let tool_properties = ToolPropertySet::new(width, default_fill_shapes);
        let active = tool_properties.properties(default_tool).copied();
        Self {
            tool: default_tool,
            style: Style {
                width: if default_tool == Tool::Eraser {
                    DEFAULT_ERASER_WIDTH
                } else {
                    width
                },
                color: [rgb[0], rgb[1], rgb[2], opacity],
                roundness: active.map_or(0.5, |properties| properties.roundness),
                filled: active.is_some_and(|properties| properties.filled),
            },
            elements: Vec::new(),
            selected: Vec::new(),
            interaction: None,
            history: History::default(),
            next_id: 1,
            picker: None,
            default_width: width,
            default_text_size: DEFAULT_TEXT_SIZE,
            default_tool,
            last_non_eraser_tool: if default_tool == Tool::Eraser {
                Tool::Pen
            } else {
                default_tool
            },
            tool_properties,
            remember_last_tool,
            palette,
        }
    }

    pub fn activate(&mut self) -> Damage {
        if self.remember_last_tool || self.tool == self.default_tool {
            return Damage::None;
        }
        self.switch_tool(self.default_tool)
    }

    pub fn deactivate(&mut self) -> Damage {
        let damage = if self.is_editing_text() {
            self.commit_text()
        } else {
            let restore_scene = match &self.interaction {
                Some(Interaction::Moving { .. } | Interaction::Resizing { .. }) => true,
                Some(Interaction::Freehand(stroke)) => !stroke.cached().is_empty(),
                _ => false,
            };
            let changed = self.interaction.take().is_some();
            if restore_scene {
                Damage::Scene
            } else {
                Damage::from_preview(changed)
            }
        };
        let clear_preview =
            !std::mem::take(&mut self.selected).is_empty() | self.picker.take().is_some();
        damage.max(Damage::from_preview(clear_preview))
    }

    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    pub fn update_text_bounds(
        &mut self,
        mut layout_size: impl FnMut(ElementId) -> Option<[f32; 2]>,
    ) {
        for element in &mut self.elements {
            if !matches!(element.kind, ElementKind::Text { .. }) {
                continue;
            }
            if let Some(size) = layout_size(element.id) {
                element.update_text_bounds(size);
            }
        }
    }

    pub fn is_editing_text(&self) -> bool {
        matches!(self.interaction, Some(Interaction::EditingText(_)))
    }

    pub fn is_drawing_pen(&self) -> bool {
        matches!(self.interaction, Some(Interaction::Freehand(_)))
    }

    fn text_edit(&self) -> Option<&TextEdit> {
        match &self.interaction {
            Some(Interaction::EditingText(edit)) => Some(edit),
            _ => None,
        }
    }

    fn text_edit_mut(&mut self) -> Option<&mut TextEdit> {
        match &mut self.interaction {
            Some(Interaction::EditingText(edit)) => Some(edit),
            _ => None,
        }
    }

    pub fn current_color(&self) -> [f32; 4] {
        if let Some(edit) = self.text_edit() {
            edit.style.color
        } else if let Some(element) = self.selected.last().and_then(|id| self.element(*id)) {
            element.style.color
        } else {
            self.style.color
        }
    }

    pub fn picker_active(&self) -> bool {
        self.picker.is_some()
    }

    pub fn handle_action(&mut self, action: Action) -> EditorEffect {
        let mut effect = EditorEffect::default();
        let closed_picker = self.picker.take().is_some();
        if closed_picker && matches!(action, Action::Cancel) {
            effect.damage = Damage::Preview;
            return effect;
        }
        match action {
            Action::Undo if !self.is_editing_text() => effect.damage = self.undo(),
            Action::Redo if !self.is_editing_text() => effect.damage = self.redo(),
            Action::SelectAll => effect.damage = self.select_all(),
            Action::ToggleEraser => effect.damage = self.toggle_eraser(),
            Action::ToggleFill => {
                let (damage, feedback) = self.toggle_fill();
                effect.damage = damage;
                effect.feedback = (!feedback.is_empty()).then_some(feedback);
            }
            Action::Delete => {
                if let Some(edit) = self.text_edit_mut() {
                    effect.damage = Damage::from_preview(edit.delete());
                } else {
                    effect.damage = self.delete_selection();
                }
            }
            Action::Clear => effect.damage = self.clear(),
            Action::Cancel => {
                let cancelled = self.cancel_interaction();
                if cancelled.changed() || !std::mem::take(&mut self.selected).is_empty() {
                    effect.damage = cancelled.max(Damage::Preview);
                } else {
                    effect.deactivate = true;
                }
            }
            Action::CommitText => effect.damage = self.commit_text(),
            Action::Backspace => {
                if let Some(edit) = self.text_edit_mut() {
                    effect.damage = Damage::from_preview(edit.backspace());
                }
            }
            Action::BackspaceWord => {
                if let Some(edit) = self.text_edit_mut() {
                    effect.damage = Damage::from_preview(edit.backspace_word());
                }
            }
            Action::MoveCursor(movement) => {
                if let Some(edit) = self.text_edit_mut() {
                    effect.damage = Damage::from_preview(edit.move_cursor(movement));
                }
            }
            Action::InsertText(text) => {
                if let Some(edit) = self.text_edit_mut() {
                    edit.insert(&text);
                    effect.damage = Damage::Preview;
                }
            }
            _ => {}
        }
        effect.damage = effect.damage.max(Damage::from_preview(closed_picker));
        effect
    }

    pub fn open_picker(&mut self, center: Point) -> Damage {
        self.picker = Some(Picker {
            center,
            hovered: None,
        });
        Damage::Preview
    }

    pub fn picker_motion(&mut self, point: Point) -> Damage {
        let Some(picker) = &mut self.picker else {
            return Damage::None;
        };
        let choice = choice(picker.center, point, self.palette.len());
        let changed = picker.hovered != choice;
        picker.hovered = choice;
        Damage::from_preview(changed)
    }

    pub fn picker_release(&mut self, point: Point, latch_center: bool) -> Damage {
        let Some(picker) = self.picker else {
            return Damage::None;
        };
        let choice = choice(picker.center, point, self.palette.len());
        if choice.is_none() && latch_center {
            return Damage::None;
        }
        self.picker = None;
        match choice {
            Some(Choice::Color(index)) => Damage::Preview.max(self.apply_rgb(self.palette[index])),
            Some(Choice::Tool(tool)) => Damage::Preview.max(self.switch_tool(tool)),
            None => Damage::Preview,
        }
    }

    pub fn dismiss_picker(&mut self) -> Damage {
        Damage::from_preview(self.picker.take().is_some())
    }

    fn toggle_eraser(&mut self) -> Damage {
        let tool = if self.tool == Tool::Eraser {
            self.last_non_eraser_tool
        } else {
            Tool::Eraser
        };
        self.switch_tool(tool)
    }

    fn picker_tool(&self) -> Tool {
        if self.tool == Tool::Eraser {
            self.last_non_eraser_tool
        } else {
            self.tool
        }
    }

    pub fn append_preview_geometry(&self, output: &mut Vec<Geometry>) {
        match &self.interaction {
            Some(Interaction::Freehand(stroke)) => output.push(stroke.tail_geometry()),
            Some(Interaction::Drawing {
                tool,
                start,
                current,
                modifiers,
            }) => output.push(geometry(
                &drawing_kind(*tool, *start, *current, *modifiers),
                self.style,
            )),
            Some(Interaction::Moving {
                ids,
                start,
                current,
            }) => output.extend(ids.iter().filter_map(|id| {
                let delta = *current - *start;
                self.element(*id)
                    .map(|element| element.geometry.translated([delta.x, delta.y]))
            })),
            Some(Interaction::Resizing { id, current, .. }) => {
                if let Some(element) = self.element(*id) {
                    output.push(geometry(current, element.style));
                }
            }
            _ => {}
        }
    }

    pub fn append_selection_geometry(&self, show_handles: bool, output: &mut Vec<Geometry>) {
        if self.tool != Tool::Select {
            return;
        }
        if self.selected.len() > 1 {
            let mut bounds: Option<(Point, Point)> = None;
            for id in &self.selected {
                let Some(element) = self.element(*id) else {
                    continue;
                };
                let preview = match &self.interaction {
                    Some(Interaction::Moving {
                        ids,
                        start,
                        current,
                    }) if ids.contains(id) => Some(element.kind.translated(*current - *start)),
                    _ => None,
                };
                let kind = preview.as_ref().unwrap_or(&element.kind);
                let element_bounds = element.preview_bounds(kind);
                let (min, max) = (element_bounds.min, element_bounds.max);
                bounds = Some(bounds.map_or((min, max), |(current_min, current_max)| {
                    (
                        Point::new(current_min.x.min(min.x), current_min.y.min(min.y)),
                        Point::new(current_max.x.max(max.x), current_max.y.max(max.y)),
                    )
                }));
            }
            if let Some((min, max)) = bounds {
                output.push(selection::outline(min, max));
            }
            return;
        }
        if let Some(id) = self.selected.first() {
            self.append_selection_geometry_for(
                *id,
                show_handles && self.interaction.is_none(),
                output,
            );
        }
    }

    pub fn cached_freehand_geometry(&self) -> &[Geometry] {
        match &self.interaction {
            Some(Interaction::Freehand(stroke)) => stroke.cached(),
            _ => &[],
        }
    }

    fn append_selection_geometry_for(
        &self,
        id: ElementId,
        show_handles: bool,
        output: &mut Vec<Geometry>,
    ) {
        let Some(element) = self.element(id) else {
            return;
        };
        let preview = match &self.interaction {
            Some(Interaction::Moving {
                ids,
                start,
                current,
            }) if ids.contains(&id) => Some(element.kind.translated(*current - *start)),
            Some(Interaction::Resizing {
                id: resizing_id,
                current,
                ..
            }) if *resizing_id == id => Some(current.clone()),
            _ => None,
        };
        let kind = preview.as_ref().unwrap_or(&element.kind);
        if !matches!(
            kind,
            ElementKind::Path { smooth: false, .. } | ElementKind::Triangle { .. }
        ) {
            let bounds = element.preview_bounds(kind);
            output.push(selection::outline(bounds.min, bounds.max));
        }
        if !show_handles {
            return;
        }
        selection::append_handles(kind, element.style, output);
    }

    pub fn picker_geometry(&self) -> Option<crate::render::LocalGeometry> {
        let picker = self.picker?;
        let active = self.picker_tool();
        Some(picker_geometry(
            picker.center,
            picker.hovered,
            active,
            self.current_color(),
            ShapeFills {
                triangle: self.tool_fill(Tool::Triangle),
                rectangle: self.tool_fill(Tool::Rectangle),
                ellipse: self.tool_fill(Tool::Ellipse),
            },
            &self.palette,
        ))
    }

    pub(super) fn active_text(&self) -> Option<&TextEdit> {
        self.text_edit()
    }

    pub fn element_is_previewed(&self, id: ElementId) -> bool {
        match &self.interaction {
            Some(Interaction::Moving { ids, .. }) => ids.contains(&id),
            Some(Interaction::Resizing { id: resized, .. }) => *resized == id,
            _ => false,
        }
    }

    pub fn moving_offset(&self, id: ElementId) -> Option<Point> {
        let Some(Interaction::Moving {
            ids,
            start,
            current,
        }) = &self.interaction
        else {
            return None;
        };
        ids.contains(&id).then_some(*current - *start)
    }

    pub fn double_click_at(&mut self, point: Point) -> Damage {
        if self.tool != Tool::Select {
            return Damage::None;
        }
        let [id] = self.selected.as_slice() else {
            return Damage::None;
        };
        let id = *id;
        let Some(element) = self.element(id) else {
            return Damage::None;
        };
        if matches!(element.kind, ElementKind::Text { .. }) && element.hit_test(point) {
            return self.begin_text_edit(id);
        }
        Damage::None
    }

    pub fn hit_test(&self, point: Point) -> Option<ElementId> {
        self.elements
            .iter()
            .rev()
            .find(|element| {
                element.bounds.expanded(HIT_SLOP).contains(point) && element.hit_test(point)
            })
            .map(|element| element.id)
    }

    pub fn undo(&mut self) -> Damage {
        let cancelled = self.cancel_interaction();
        if !self.history.undo(&mut self.elements) {
            return cancelled;
        }
        self.selected.clear();
        Damage::Scene
    }

    pub fn redo(&mut self) -> Damage {
        let cancelled = self.cancel_interaction();
        if !self.history.redo(&mut self.elements) {
            return cancelled;
        }
        self.selected.clear();
        Damage::Scene
    }

    fn select_all(&mut self) -> Damage {
        let cancelled = self.cancel_interaction();
        if self.elements.is_empty() {
            return cancelled;
        }
        let damage = cancelled.max(self.switch_tool(Tool::Select));
        let selected = self.elements.iter().map(|element| element.id).collect();
        if self.selected == selected {
            return damage;
        }
        self.selected = selected;
        damage.max(Damage::Preview)
    }

    fn clear(&mut self) -> Damage {
        let cancelled = self.cancel_interaction();
        if self.elements.is_empty() {
            return cancelled;
        }
        let elements = std::mem::take(&mut self.elements);
        self.history.record(HistoryEntry::Clear(elements));
        self.selected.clear();
        Damage::Scene
    }

    fn delete_selection(&mut self) -> Damage {
        let selected = std::mem::take(&mut self.selected);
        if selected.is_empty() {
            return Damage::None;
        }
        let cancelled = self.cancel_interaction();
        if selected.len() == self.elements.len() {
            let elements = std::mem::take(&mut self.elements);
            self.history.record(HistoryEntry::Clear(elements));
            return cancelled.max(Damage::Scene);
        }
        cancelled.max(Damage::from_scene(self.remove_ids(&selected)))
    }

    fn remove_ids(&mut self, ids: &[ElementId]) -> bool {
        let mut removed = Vec::with_capacity(ids.len());
        for index in (0..self.elements.len()).rev() {
            if ids.contains(&self.elements[index].id) {
                removed.push((index, self.elements.remove(index)));
            }
        }
        if removed.is_empty() {
            return false;
        }
        removed.reverse();
        self.history.record(HistoryEntry::Remove(removed));
        self.selected.retain(|selected| !ids.contains(selected));
        true
    }

    fn insert_kind(&mut self, kind: ElementKind, style: Style) {
        self.insert_kind_with_geometry(kind, style, None);
    }

    fn insert_kind_with_geometry(
        &mut self,
        kind: ElementKind,
        style: Style,
        geometry: Option<Geometry>,
    ) {
        let element = match geometry {
            Some(geometry) => Element::with_geometry(self.next_id, kind, style, geometry),
            None => Element::new(self.next_id, kind, style),
        };
        self.next_id += 1;
        let index = self.elements.len();
        let id = element.id;
        self.elements.push(element);
        self.history.record(HistoryEntry::Insert(vec![(index, id)]));
    }

    fn remove_id(&mut self, id: ElementId) -> bool {
        self.remove_ids(&[id])
    }

    fn erase_at(&mut self, point: Point) -> bool {
        let radius = super::eraser_radius(self.eraser_width());
        let hit = self
            .elements
            .iter()
            .rev()
            .find(|element| element.erase_hit_test(point, radius))
            .map(|element| element.id);
        hit.is_some_and(|id| self.remove_id(id))
    }

    fn commit_text(&mut self) -> Damage {
        let Some(Interaction::EditingText(TextEdit {
            id,
            origin,
            content,
            font_size,
            style,
            ..
        })) = self.interaction.take()
        else {
            return Damage::None;
        };
        if content.is_empty() {
            return id.map_or(Damage::Preview, |id| Damage::from_scene(self.remove_id(id)));
        }
        let kind = ElementKind::Text {
            origin,
            content,
            font_size,
        };
        if let Some(id) = id {
            let element = self.element_mut(id).expect("editing text exists");
            if element.kind == kind && element.style == style {
                return Damage::Preview;
            }
            let (kind, style) = element.replace(kind, style);
            self.history
                .record(HistoryEntry::Update(vec![(id, kind, style)]));
        } else {
            self.insert_kind(kind, style);
        }
        Damage::Scene
    }

    fn begin_text_edit(&mut self, id: ElementId) -> Damage {
        let Some(element) = self.element(id) else {
            return Damage::None;
        };
        let ElementKind::Text {
            origin,
            content,
            font_size,
        } = &element.kind
        else {
            return Damage::None;
        };
        self.interaction = Some(Interaction::EditingText(TextEdit {
            id: Some(id),
            origin: *origin,
            content: content.clone(),
            cursor: content.len(),
            font_size: *font_size,
            style: element.style,
        }));
        Damage::Scene
    }

    fn switch_tool(&mut self, tool: Tool) -> Damage {
        if self.tool == tool {
            return Damage::None;
        }
        let damage = self.finish_interaction().max(Damage::Preview);
        self.selected.clear();
        self.tool = tool;
        if tool != Tool::Eraser {
            self.last_non_eraser_tool = tool;
        }
        self.sync_active_style();
        damage
    }

    fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements.iter().find(|element| element.id == id)
    }

    fn element_mut(&mut self, id: ElementId) -> Option<&mut Element> {
        self.elements.iter_mut().find(|element| element.id == id)
    }
}

fn drawing_kind(tool: Tool, start: Point, current: Point, modifiers: Modifiers) -> ElementKind {
    match tool {
        Tool::Line | Tool::Arrow => ElementKind::Path {
            points: vec![
                start,
                selection::constrained_endpoint(start, current, modifiers.shift),
            ],
            smooth: false,
            end_marker: (tool == Tool::Arrow).then_some(EndMarker::Arrow),
        },
        Tool::Triangle => ElementKind::Triangle {
            vertices: selection::triangle_from_drag(start, current, modifiers),
        },
        Tool::Rectangle => {
            let (min, max) =
                selection::constrained_box(start, current, modifiers.shift, modifiers.alt);
            ElementKind::Rectangle { min, max }
        }
        Tool::Ellipse => {
            let (min, max) =
                selection::constrained_box(start, current, modifiers.shift, modifiers.alt);
            ElementKind::Ellipse {
                center: min.midpoint(max),
                radii: Point::new((max.x - min.x) * 0.5, (max.y - min.y) * 0.5),
            }
        }
        Tool::Pen | Tool::Text | Tool::Eraser | Tool::Select => unreachable!(),
    }
}

use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::WEnum;

use wayland_client::protocol::wl_pointer::WlPointer;

use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;

use super::super::State;
use super::super::draw::{Action, CursorHint, Point};

const EVDEV_LEFT: u32 = 272;
const EVDEV_RIGHT: u32 = 273;
const EVDEV_MIDDLE: u32 = 274;
const EVDEV_SIDE: u32 = 275;
const EVDEV_EXTRA: u32 = 276;
const EVDEV_FORWARD: u32 = 277;
const EVDEV_BACK: u32 = 278;
const LEFT: u8 = 1;
const RIGHT: u8 = 2;
const MIDDLE: u8 = 4;
const UNDO: u8 = 8;
const REDO: u8 = 16;

#[derive(Default)]
pub(in crate::state) struct PointerState {
    event_sequence: EventSequence,

    cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    cursor_serial: Option<u32>,

    position: Option<(f64, f64)>,
    left_button_held: bool,
    right_button_held: bool,
    middle_button_held: bool,
    left_button_in_picker: bool,
    left_press_pos: Option<(f64, f64)>,
    last_left_click: Option<(u32, (f64, f64))>,
    scroll_remainder: f64,
}

impl PointerState {
    pub(in crate::state) fn set_cursor_shape_device(
        &mut self,
        cursor_shape_device: WpCursorShapeDeviceV1,
    ) {
        self.cursor_shape_device = Some(cursor_shape_device)
    }

    pub(in crate::state) fn clear_pointer(&mut self) {
        *self = Self::default();
    }

    pub(in crate::state) fn cancel_gesture(&mut self) -> bool {
        let interaction_active = self.left_button_held || self.middle_button_held;
        self.left_button_held = false;
        self.right_button_held = false;
        self.middle_button_held = false;
        self.left_button_in_picker = false;
        self.left_press_pos = None;
        self.last_left_click = None;
        self.scroll_remainder = 0.0;
        interaction_active
    }

    fn update_state(&mut self, sequence: EventSequence) {
        if let Some(new_pos) = sequence.motion {
            self.position = Some(new_pos);
        }

        if let Some(serial) = sequence.enter_serial {
            self.cursor_serial = Some(serial);
        }

        if sequence.leave_serial.is_some() {
            self.position = None;
            self.cursor_serial = None;
        }

        update_button(&mut self.left_button_held, sequence, LEFT);
        update_button(&mut self.right_button_held, sequence, RIGHT);
        update_button(&mut self.middle_button_held, sequence, MIDDLE);
    }

    fn scroll_steps(&mut self, sequence: EventSequence) -> f32 {
        self.scroll_remainder -= sequence.scroll_steps();
        let steps = self.scroll_remainder.trunc();
        self.scroll_remainder -= steps;
        if sequence.vertical_axis_stopped {
            self.scroll_remainder = 0.0;
        }
        steps as f32
    }
}

impl Dispatch<WlPointer, (), State> for PointerState {
    fn event(
        state: &mut State,
        _pointer: &WlPointer,
        event: <WlPointer as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<State>,
    ) {
        if let Some(sequence) = state.pointer.event_sequence.dispatch(event) {
            state.pointer.update_state(sequence);

            if sequence.leave_serial.is_some() {
                state.cancel_pointer_gesture();
                return;
            }
            let left_pressed = sequence.pressed(LEFT);
            let left_released = sequence.released(LEFT);
            let right_pressed = sequence.pressed(RIGHT);
            let right_released = sequence.released(RIGHT);
            let middle_pressed = sequence.pressed(MIDDLE);
            let middle_released = sequence.released(MIDDLE);

            if sequence.pressed(UNDO) {
                state.apply_action(Action::Undo);
            }
            if sequence.pressed(REDO) {
                state.apply_action(Action::Redo);
            }

            if (sequence.enter_serial.is_some() || sequence.motion.is_some())
                && let Some((x, y)) = state.pointer.position
                && let Some(ref device) = state.pointer.cursor_shape_device
                && let Some(serial) = state.pointer.cursor_serial
            {
                device.set_shape(
                    serial,
                    cursor_shape(state.draw.cursor_hint(Point::new(x as f32, y as f32))),
                );
            }

            let modifiers = state.modifiers();
            if left_pressed && let Some(pos) = state.pointer.position {
                state.pointer.left_press_pos = Some(pos);
                state.pointer.left_button_in_picker = state.draw.picker_active();
                let double_click = !state.pointer.left_button_in_picker
                    && sequence.left_press_time.is_some_and(|time| {
                        state
                            .pointer
                            .last_left_click
                            .is_some_and(|(previous, previous_pos)| {
                                time.wrapping_sub(previous) <= 400
                                    && distance_squared(pos, previous_pos) <= 36.0
                            })
                    });
                if !double_click || !state.double_click_at(pos) {
                    state.pointer_down(pos, modifiers, false);
                }
                state.pointer.last_left_click = sequence
                    .left_press_time
                    .map(|time| (time, pos))
                    .filter(|_| !double_click);
            }
            if right_pressed && let Some(pos) = state.pointer.position {
                state.toggle_picker(pos);
            }
            if middle_pressed && let Some(pos) = state.pointer.position {
                state.dismiss_picker();
                state.pointer_down(pos, modifiers, true);
            }
            if sequence.motion.is_some()
                && let Some(pos) = state.pointer.position
            {
                if (state.pointer.left_button_held
                    || (left_released && state.pointer.left_press_pos.is_some()))
                    && state
                        .pointer
                        .left_press_pos
                        .is_some_and(|start| distance_squared(pos, start) > 36.0)
                {
                    state.pointer.last_left_click = None;
                }
                if (state.draw.picker_active()
                    || state.pointer.left_button_held
                    || state.pointer.right_button_held
                    || state.pointer.middle_button_held)
                    && !left_pressed
                    && !right_pressed
                    && !middle_pressed
                {
                    state.pointer_motion(pos, modifiers);
                }
            }
            if let Some(pos) = state.pointer.position {
                if left_released {
                    if !state.pointer.left_button_in_picker || state.draw.picker_active() {
                        state.pointer_up(pos, modifiers, false);
                    }
                    state.pointer.left_button_in_picker = false;
                    state.pointer.left_press_pos = None;
                }
                if right_released {
                    state.pointer_up(pos, modifiers, true);
                }
                if middle_released {
                    state.pointer_up(pos, modifiers, false);
                }
                if state.draw.picker_active() {
                    if sequence.vertical_axis_stopped {
                        state.pointer.scroll_remainder = 0.0;
                    }
                } else {
                    let steps = state.pointer.scroll_steps(sequence);
                    if steps != 0.0 {
                        state.adjust(steps, pos, modifiers);
                    }
                }
            }
        }
    }
}

fn cursor_shape(hint: CursorHint) -> Shape {
    match hint {
        CursorHint::Crosshair => Shape::Crosshair,
        CursorHint::Move => Shape::Move,
        CursorHint::NsResize => Shape::NsResize,
        CursorHint::EwResize => Shape::EwResize,
        CursorHint::NwseResize => Shape::NwseResize,
        CursorHint::NeswResize => Shape::NeswResize,
    }
}

#[derive(Default, Clone, Copy)]
struct EventSequence {
    motion: Option<(f64, f64)>,

    pressed: u8,
    released: u8,
    left_press_time: Option<u32>,
    axis_vertical: f64,
    axis_discrete: i32,
    axis_value120: i32,
    vertical_axis_stopped: bool,

    enter_serial: Option<u32>,
    leave_serial: Option<u32>,
}

impl EventSequence {
    fn pressed(self, button: u8) -> bool {
        self.pressed & button != 0
    }

    fn released(self, button: u8) -> bool {
        self.released & button != 0
    }

    fn scroll_steps(self) -> f64 {
        if self.axis_value120 != 0 {
            self.axis_value120 as f64 / 120.0
        } else if self.axis_discrete != 0 {
            self.axis_discrete as f64
        } else {
            self.axis_vertical / 10.0
        }
    }

    fn dispatch(&mut self, event: <WlPointer as Proxy>::Event) -> Option<Self> {
        use wayland_client::protocol::wl_pointer::Event;
        match event {
            Event::Enter {
                serial,
                surface: _,
                surface_x,
                surface_y,
            } => {
                self.enter_serial = Some(serial);
                self.motion = Some((surface_x, surface_y));
                None
            }
            Event::Leave { serial, surface: _ } => {
                self.leave_serial = Some(serial);
                None
            }
            Event::Motion {
                time: _,
                surface_x,
                surface_y,
            } => {
                self.motion = Some((surface_x, surface_y));
                None
            }
            Event::Button {
                serial: _,
                time,
                button,
                state: button_state,
            } => {
                use wayland_client::protocol::wl_pointer::ButtonState;
                let transition = match button_state {
                    WEnum::Value(ButtonState::Pressed) => &mut self.pressed,
                    WEnum::Value(ButtonState::Released) => &mut self.released,
                    _ => return None,
                };
                let mask = match button {
                    EVDEV_LEFT => LEFT,
                    EVDEV_RIGHT => RIGHT,
                    EVDEV_MIDDLE => MIDDLE,
                    EVDEV_SIDE | EVDEV_BACK => UNDO,
                    EVDEV_EXTRA | EVDEV_FORWARD => REDO,
                    _ => return None,
                };
                *transition |= mask;
                if mask == LEFT && matches!(button_state, WEnum::Value(ButtonState::Pressed)) {
                    self.left_press_time = Some(time);
                }
                None
            }
            Event::Axis {
                axis: WEnum::Value(wayland_client::protocol::wl_pointer::Axis::VerticalScroll),
                value,
                ..
            } => {
                self.axis_vertical += value;
                None
            }
            Event::AxisDiscrete {
                axis: WEnum::Value(wayland_client::protocol::wl_pointer::Axis::VerticalScroll),
                discrete,
            } => {
                self.axis_discrete += discrete;
                None
            }
            Event::AxisValue120 {
                axis: WEnum::Value(wayland_client::protocol::wl_pointer::Axis::VerticalScroll),
                value120,
            } => {
                self.axis_value120 += value120;
                None
            }
            Event::AxisStop {
                axis: WEnum::Value(wayland_client::protocol::wl_pointer::Axis::VerticalScroll),
                ..
            } => {
                self.vertical_axis_stopped = true;
                None
            }
            Event::Frame => {
                let mut tmp = Self::default();
                std::mem::swap(self, &mut tmp);
                Some(tmp)
            }
            _ => None,
        }
    }
}

fn update_button(held: &mut bool, sequence: EventSequence, button: u8) {
    if sequence.pressed(button) {
        *held = true;
    }
    if sequence.released(button) {
        *held = false;
    }
}

fn distance_squared(first: (f64, f64), second: (f64, f64)) -> f64 {
    (first.0 - second.0).powi(2) + (first.1 - second.1).powi(2)
}

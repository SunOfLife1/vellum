use std::collections::HashMap;

use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::WEnum;
use wayland_client::backend::ObjectId;

use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;

use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_v2::ZwpTabletPadV2;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::ZwpTabletSeatV2;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_tool_v2::ZwpTabletToolV2;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_v2::ZwpTabletV2;

use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE;

use super::super::State;

const ERASER_BUTTON: u32 = 331;
const PEN: u8 = 1;
const BUTTON: u8 = 2;

#[derive(Default)]
pub(in crate::state) struct TabletState {
    event_sequence: EventSequence,

    _tablet_seat: Option<ZwpTabletSeatV2>,
    tablet_cursor_shape_devices: HashMap<ObjectId, WpCursorShapeDeviceV1>,

    pos: Option<(f64, f64)>,
    pen_held: bool,
    button_held: bool,
}

impl TabletState {
    pub(in crate::state) fn set_tablet_seat(&mut self, tablet_seat: ZwpTabletSeatV2) {
        self._tablet_seat = Some(tablet_seat);
    }

    fn update_state(&mut self, sequence: EventSequence) {
        if let Some(new_pos) = sequence.motion {
            self.pos = Some(new_pos);
        }

        update_held(&mut self.pen_held, sequence, PEN);
        update_held(&mut self.button_held, sequence, BUTTON);
    }
}

impl Dispatch<ZwpTabletSeatV2, (), State> for TabletState {
    fn event(
        state: &mut State,
        _tablet_seat: &ZwpTabletSeatV2,
        event: <ZwpTabletSeatV2 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qhandle: &QueueHandle<State>,
    ) {
        use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::Event;
        if let Event::ToolAdded { id } = event
            && let Some(manager) = &state.wayland.cursor_shape_manager
        {
            let device = manager.get_tablet_tool_v2(&id, qhandle, ());
            state
                .tablet
                .tablet_cursor_shape_devices
                .insert(id.id(), device);
        }
    }

    wayland_client::event_created_child!(State, ZwpTabletSeatV2, [
        EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<ZwpTabletToolV2, (), State> for TabletState {
    fn event(
        state: &mut State,
        tablet_tool: &ZwpTabletToolV2,
        event: <ZwpTabletToolV2 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<State>,
    ) {
        use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_tool_v2::Event;
        if matches!(&event, Event::Removed) {
            state
                .tablet
                .tablet_cursor_shape_devices
                .remove(&tablet_tool.id());
            return;
        }
        if let Some(sequence) = state.tablet.event_sequence.dispatch(event) {
            state.tablet.update_state(sequence);
            let pen_pressed = sequence.pressed(PEN);
            let pen_released = sequence.released(PEN);
            let button_pressed = sequence.pressed(BUTTON);
            let button_released = sequence.released(BUTTON);

            if let Some(device) = state
                .tablet
                .tablet_cursor_shape_devices
                .get(&tablet_tool.id())
                && let Some(serial) = sequence.enter_serial
            {
                device.set_shape(serial, Shape::Crosshair);
            }

            let modifiers = state.modifiers();
            if pen_pressed && let Some(pos) = state.tablet.pos {
                state.pointer_down(pos, modifiers, state.tablet.button_held);
            }
            if button_pressed
                && state.tablet.pen_held
                && let Some(pos) = state.tablet.pos
            {
                state.pointer_up(pos, modifiers, false);
                state.pointer_down(pos, modifiers, true);
            }
            if button_released
                && state.tablet.pen_held
                && let Some(pos) = state.tablet.pos
            {
                state.pointer_up(pos, modifiers, false);
                state.pointer_down(pos, modifiers, false);
            }
            if !button_pressed
                && !button_released
                && sequence.motion.is_some()
                && state.tablet.pen_held
                && let Some(pos) = state.tablet.pos
            {
                state.pointer_motion(pos, modifiers);
            }
            if pen_released && let Some(pos) = state.tablet.pos {
                state.pointer_up(pos, modifiers, false);
            }
        }
    }
}

#[derive(Default, Clone, Copy)]
struct EventSequence {
    motion: Option<(f64, f64)>,

    pressed: u8,
    released: u8,

    enter_serial: Option<u32>,
}

impl EventSequence {
    fn pressed(self, input: u8) -> bool {
        self.pressed & input != 0
    }

    fn released(self, input: u8) -> bool {
        self.released & input != 0
    }

    fn dispatch(&mut self, event: <ZwpTabletToolV2 as Proxy>::Event) -> Option<Self> {
        use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_tool_v2::Event;
        match event {
            Event::ProximityIn {
                serial,
                tablet: _,
                surface: _,
            } => {
                self.enter_serial = Some(serial);
                None
            }
            Event::Down { serial: _ } => {
                self.pressed |= PEN;
                None
            }
            Event::Up => {
                self.released |= PEN;
                None
            }
            Event::Motion { x, y } => {
                self.motion = Some((x, y));
                None
            }
            Event::Button {
                serial: _,
                button,
                state: button_state,
            } => {
                use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_tool_v2::ButtonState;
                if button == ERASER_BUTTON {
                    match button_state {
                        WEnum::Value(ButtonState::Pressed) => self.pressed |= BUTTON,
                        WEnum::Value(ButtonState::Released) => self.released |= BUTTON,
                        _ => {}
                    }
                }
                None
            }
            Event::Frame { time: _ } => {
                let mut tmp = Self::default();
                std::mem::swap(self, &mut tmp);
                Some(tmp)
            }
            _ => None,
        }
    }
}

fn update_held(held: &mut bool, sequence: EventSequence, input: u8) {
    if sequence.pressed(input) {
        *held = true;
    }
    if sequence.released(input) {
        *held = false;
    }
}

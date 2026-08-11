use std::os::fd::OwnedFd;

use wayland_client::protocol::wl_keyboard::{KeyState, KeymapFormat, WlKeyboard};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use xkbcommon::xkb;

use super::super::State;
use super::super::draw::{Action, CursorMove, Modifiers};

const KEYCODE_OFFSET: u32 = 8;
const UNDO_KEY: &str = "z";

#[derive(Debug, Clone, PartialEq, Eq)]
enum LogicalKey {
    Character(String),
    Escape,
    Delete,
    Backspace,
    Enter,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyChord {
    key: LogicalKey,
    modifiers: Modifiers,
}

fn resolve_keybinding(chord: &KeyChord, editing_text: bool) -> Option<Action> {
    use LogicalKey::*;

    if editing_text {
        return match &chord.key {
            Escape => Some(Action::Cancel),
            Delete => Some(Action::DeleteForward),
            Backspace => Some(Action::Backspace),
            Enter => Some(Action::CommitText),
            ArrowLeft => Some(Action::MoveCursor(CursorMove::Left)),
            ArrowRight => Some(Action::MoveCursor(CursorMove::Right)),
            Home => Some(Action::MoveCursor(CursorMove::Home)),
            End => Some(Action::MoveCursor(CursorMove::End)),
            Character(text) if !chord.modifiers.ctrl && !text.chars().any(char::is_control) => {
                Some(Action::InsertText(text.clone()))
            }
            _ => None,
        };
    }

    match &chord.key {
        Escape => Some(Action::Cancel),
        Character(character) if chord.modifiers.ctrl => {
            let character = character.to_lowercase();
            match character.as_str() {
                UNDO_KEY if chord.modifiers.shift => Some(Action::Redo),
                UNDO_KEY => Some(Action::Undo),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(in crate::state) struct KeyboardState {
    context: xkb::Context,
    state: Option<xkb::State>,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self {
            context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            state: None,
        }
    }
}

impl KeyboardState {
    fn set_keymap(&mut self, fd: OwnedFd, size: u32) -> std::io::Result<()> {
        // SAFETY: Wayland transfers ownership of a valid keymap fd and supplies its mapping size.
        let keymap = unsafe {
            xkb::Keymap::new_from_fd(
                &self.context,
                fd,
                size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::COMPILE_NO_FLAGS,
            )?
        };
        self.state = keymap.as_ref().map(xkb::State::new);
        Ok(())
    }

    fn update_modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if let Some(state) = &mut self.state {
            state.update_mask(depressed, latched, locked, 0, 0, group);
        }
    }

    pub(in crate::state) fn modifiers(&self) -> Modifiers {
        let Some(state) = &self.state else {
            return Modifiers::default();
        };
        Modifiers {
            shift: state.mod_name_is_active(xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE),
            ctrl: state.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE),
            alt: state.mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE),
        }
    }

    fn chord(&self, evdev_key: u32) -> Option<KeyChord> {
        let state = self.state.as_ref()?;
        let keycode = (evdev_key + KEYCODE_OFFSET).into();
        let keysym = state.key_get_one_sym(keycode);
        let modifiers = self.modifiers();
        let key = match keysym {
            value if value.raw() == xkb::keysyms::KEY_Escape => LogicalKey::Escape,
            value if value.raw() == xkb::keysyms::KEY_Delete => LogicalKey::Delete,
            value if value.raw() == xkb::keysyms::KEY_BackSpace => LogicalKey::Backspace,
            value
                if matches!(
                    value.raw(),
                    xkb::keysyms::KEY_Return | xkb::keysyms::KEY_KP_Enter
                ) =>
            {
                LogicalKey::Enter
            }
            value if value.raw() == xkb::keysyms::KEY_Left => LogicalKey::ArrowLeft,
            value if value.raw() == xkb::keysyms::KEY_Right => LogicalKey::ArrowRight,
            value if value.raw() == xkb::keysyms::KEY_Home => LogicalKey::Home,
            value if value.raw() == xkb::keysyms::KEY_End => LogicalKey::End,
            _ => {
                let text = if modifiers.ctrl {
                    xkb::keysym_to_utf8(keysym)
                } else {
                    state.key_get_utf8(keycode)
                };
                if text.is_empty() {
                    LogicalKey::Other
                } else {
                    LogicalKey::Character(text)
                }
            }
        };
        Some(KeyChord { key, modifiers })
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _keyboard: &WlKeyboard,
        event: <WlKeyboard as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_keyboard::Event;

        match event {
            Event::Keymap {
                format: WEnum::Value(KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                if let Err(error) = state.keyboard.set_keymap(fd, size) {
                    eprintln!("vellum: failed to load XKB keymap: {error}");
                }
            }
            Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => state
                .keyboard
                .update_modifiers(mods_depressed, mods_latched, mods_locked, group),
            Event::Leave { .. } => state.cancel_pointer_gesture(),
            Event::Key {
                key,
                state: WEnum::Value(KeyState::Pressed),
                ..
            } if state.active => {
                if let Some(chord) = state.keyboard.chord(key)
                    && let Some(action) = resolve_keybinding(&chord, state.draw.is_editing_text())
                {
                    state.apply_action(action);
                }
            }
            _ => {}
        }
    }
}

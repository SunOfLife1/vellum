use super::scene::{ElementId, Point, Style};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorMove {
    Left,
    Right,
    Home,
    End,
}

#[derive(Debug)]
pub(super) struct TextEdit {
    pub(super) id: Option<ElementId>,
    pub(super) origin: Point,
    pub(super) content: String,
    pub(super) cursor: usize,
    pub(super) font_size: f32,
    pub(super) style: Style,
}

impl TextEdit {
    pub(super) fn insert(&mut self, text: &str) {
        self.content.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub(super) fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let previous = previous_boundary(&self.content, self.cursor);
        self.content.drain(previous..self.cursor);
        self.cursor = previous;
        true
    }

    pub(super) fn delete(&mut self) -> bool {
        if self.cursor == self.content.len() {
            return false;
        }
        let next = next_boundary(&self.content, self.cursor);
        self.content.drain(self.cursor..next);
        true
    }

    pub(super) fn move_cursor(&mut self, movement: CursorMove) {
        self.cursor = match movement {
            CursorMove::Left => previous_boundary(&self.content, self.cursor),
            CursorMove::Right => next_boundary(&self.content, self.cursor),
            CursorMove::Home => 0,
            CursorMove::End => self.content.len(),
        };
    }
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .grapheme_indices(true)
        .nth(1)
        .map_or(text.len(), |(index, _)| cursor + index)
}

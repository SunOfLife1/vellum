use super::scene::{ElementId, Point, Style};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorMove {
    Left,
    Right,
    WordLeft,
    WordRight,
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

    pub(super) fn backspace_word(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let previous = previous_word_boundary(&self.content, self.cursor);
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

    pub(super) fn move_cursor(&mut self, movement: CursorMove) -> bool {
        let cursor = match movement {
            CursorMove::Left => previous_boundary(&self.content, self.cursor),
            CursorMove::Right => next_boundary(&self.content, self.cursor),
            CursorMove::WordLeft => previous_word_boundary(&self.content, self.cursor),
            CursorMove::WordRight => next_word_boundary(&self.content, self.cursor),
            CursorMove::Home => 0,
            CursorMove::End => self.content.len(),
        };
        let changed = cursor != self.cursor;
        self.cursor = cursor;
        changed
    }
}

fn previous_word_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .unicode_word_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_word_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .unicode_word_indices()
        .next()
        .map_or(text.len(), |(index, word)| cursor + index + word.len())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(content: &str, cursor: usize) -> TextEdit {
        TextEdit {
            id: None,
            origin: Point::new(0.0, 0.0),
            content: content.into(),
            cursor,
            font_size: 16.0,
            style: Style {
                width: 1.0,
                color: [0.0; 4],
                roundness: 0.0,
            },
        }
    }

    #[test]
    fn moves_by_unicode_words() {
        let mut edit = edit("one café three", "one café three".len());
        assert!(edit.move_cursor(CursorMove::WordLeft));
        assert_eq!(&edit.content[edit.cursor..], "three");
        assert!(edit.move_cursor(CursorMove::WordLeft));
        assert_eq!(&edit.content[edit.cursor..], "café three");
        assert!(edit.move_cursor(CursorMove::WordRight));
        assert_eq!(&edit.content[..edit.cursor], "one café");
        edit.move_cursor(CursorMove::End);
        assert!(!edit.move_cursor(CursorMove::Right));
    }

    #[test]
    fn backspaces_a_word_and_leading_separator() {
        let mut edit = edit("one, two", "one, two".len());
        assert!(edit.backspace_word());
        assert_eq!(edit.content, "one, ");
        assert!(edit.backspace_word());
        assert_eq!(edit.content, "");
    }
}

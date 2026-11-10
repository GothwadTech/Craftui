//! Complex script shaping, font discovery, layout metrics, and IME editing powered by `parley`.

use craftui_core::{Color, Rect, Size};
use parley::editing::{Cursor, Selection};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, GenericFamily,
    Layout, LayoutContext, LineHeight, StyleProperty,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Text horizontal alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// Text styling configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_size: f32,
    pub font_weight: f32,
    pub italic: bool,
    pub font_family: Option<String>,
    pub color: Color,
    pub line_height: f32,
    pub align: TextAlign,
    pub underline: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            font_weight: 400.0,
            italic: false,
            font_family: None,
            color: Color::WHITE,
            line_height: 1.3,
            align: TextAlign::Start,
            underline: false,
        }
    }
}

impl TextStyle {
    pub fn with_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.font_weight = weight;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
}

/// Shared font database and shaping scratchpad.
pub struct TextEngine {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<Color>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    /// Register custom font bytes (e.g., bundled app icons or fallback font on minimal systems).
    pub fn register_font_bytes(&mut self, bytes: Vec<u8>) {
        self.font_cx.collection.register_fonts(bytes.into(), None);
    }

    /// Shape and break text lines into a [`parley::Layout<Color>`].
    pub fn build_layout(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> Layout<Color> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(style.font_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(
            style.font_weight,
        )));
        builder.push_default(StyleProperty::FontStyle(if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        builder.push_default(StyleProperty::Brush(style.color));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        )));
        builder.push_default(StyleProperty::Underline(style.underline));

        if let Some(family) = style
            .font_family
            .as_deref()
            .filter(|f| !f.trim().is_empty())
        {
            builder.push_default(StyleProperty::FontFamily(FontFamily::from(family)));
        } else {
            builder.push_default(StyleProperty::FontFamily(FontFamily::from(
                GenericFamily::SansSerif,
            )));
        }

        let mut layout = builder.build(text);
        layout.break_all_lines(max_width);
        let parley_align = match style.align {
            TextAlign::Start => Alignment::Start,
            TextAlign::Center => Alignment::Center,
            TextAlign::End => Alignment::End,
            TextAlign::Justify => Alignment::Justify,
        };
        layout.align(parley_align, AlignmentOptions::default());
        layout
    }

    /// Measure bounding size of shaped text under optional `max_width`.
    pub fn measure(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>) -> Size {
        if text.is_empty() {
            return Size::new(0.0, style.font_size * style.line_height);
        }
        let layout = self.build_layout(text, style, max_width);
        Size::new(
            layout.width().ceil(),
            layout
                .height()
                .max(style.font_size * style.line_height)
                .ceil(),
        )
    }

    /// Compute cursor rectangle in local layout coordinates.
    pub fn cursor_rect(&self, layout: &Layout<Color>, byte_index: usize) -> Rect {
        let cursor = Cursor::from_byte_index(layout, byte_index, parley::Affinity::Downstream);
        let bbox = cursor.geometry(layout, 2.0);
        Rect::new(
            bbox.x0 as f32,
            bbox.y0 as f32,
            (bbox.x1 - bbox.x0).max(1.5) as f32,
            (bbox.y1 - bbox.y0).max(12.0) as f32,
        )
    }

    /// Compute selection highlight rectangles in local layout coordinates.
    pub fn selection_rects(
        &self,
        layout: &Layout<Color>,
        anchor_byte: usize,
        focus_byte: usize,
    ) -> Vec<Rect> {
        let anchor = Cursor::from_byte_index(layout, anchor_byte, parley::Affinity::Downstream);
        let focus = Cursor::from_byte_index(layout, focus_byte, parley::Affinity::Downstream);
        let sel = Selection::new(anchor, focus);
        sel.geometry(layout)
            .into_iter()
            .map(|(b, _)| {
                Rect::new(
                    b.x0 as f32,
                    b.y0 as f32,
                    (b.x1 - b.x0) as f32,
                    (b.y1 - b.y0) as f32,
                )
            })
            .collect()
    }

    /// Map local point `(x, y)` to nearest UTF-8 byte index in `layout`.
    pub fn hit_test_point(&self, layout: &Layout<Color>, x: f32, y: f32) -> usize {
        let cursor = Cursor::from_point(layout, x, y);
        cursor.index()
    }
}

/// Interactive single/multi-line text buffer state supporting selection and IME preedit.
#[derive(Debug, Clone, Default)]
pub struct TextEditorState {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
    pub preedit: Option<String>,
    pub preedit_cursor: Option<(usize, usize)>,
}

impl TextEditorState {
    pub fn new(initial: impl Into<String>) -> Self {
        let text = initial.into();
        let len = text.len();
        Self {
            text,
            cursor: len,
            anchor: len,
            preedit: None,
            preedit_cursor: None,
        }
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        if self.cursor == self.anchor {
            None
        } else {
            Some((self.cursor.min(self.anchor), self.cursor.max(self.anchor)))
        }
    }

    pub fn selected_text(&self) -> &str {
        if let Some((start, end)) = self.selection_range() {
            &self.text[start..end]
        } else {
            ""
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        let idx = self.clamp_boundary(self.cursor);
        self.text.insert_str(idx, s);
        self.cursor = idx + s.len();
        self.anchor = self.cursor;
    }

    pub fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection_range() {
            self.text.replace_range(start..end, "");
            self.cursor = start;
            self.anchor = start;
            true
        } else {
            false
        }
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur == 0 {
            return;
        }
        let prev = self.prev_char_boundary(cur);
        self.text.replace_range(prev..cur, "");
        self.cursor = prev;
        self.anchor = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur >= self.text.len() {
            return;
        }
        let next = self.next_char_boundary(cur);
        self.text.replace_range(cur..next, "");
        self.anchor = self.cursor;
    }

    pub fn move_left(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.prev_char_boundary(cur);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    pub fn move_right(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.next_char_boundary(cur);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    pub fn move_home(&mut self, extend_selection: bool) {
        self.cursor = 0;
        if !extend_selection {
            self.anchor = 0;
        }
    }

    pub fn move_end(&mut self, extend_selection: bool) {
        self.cursor = self.text.len();
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    /// Move cursor vertically up one logical `\n`-separated line keeping approximate column offset.
    pub fn move_up(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        let before = &self.text[..cur];
        let Some(curr_line_start_nl) = before.rfind('\n') else {
            self.move_home(extend_selection);
            return;
        };
        let col_chars = self.text[curr_line_start_nl + 1..cur].chars().count();
        let prev_line_start = self.text[..curr_line_start_nl]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let target_byte = prev_line_start
            + self.text[prev_line_start..curr_line_start_nl]
                .char_indices()
                .nth(col_chars)
                .map(|(off, _)| off)
                .unwrap_or(curr_line_start_nl - prev_line_start);
        self.set_cursor_at(target_byte, extend_selection);
    }

    /// Move cursor vertically down one logical `\n`-separated line keeping approximate column offset.
    pub fn move_down(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        let curr_line_start = self.text[..cur].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let col_chars = self.text[curr_line_start..cur].chars().count();
        let Some(rel_nl) = self.text[cur..].find('\n') else {
            self.move_end(extend_selection);
            return;
        };
        let next_line_start = cur + rel_nl + 1;
        let next_line_end = self.text[next_line_start..]
            .find('\n')
            .map(|off| next_line_start + off)
            .unwrap_or(self.text.len());
        let target_byte = next_line_start
            + self.text[next_line_start..next_line_end]
                .char_indices()
                .nth(col_chars)
                .map(|(off, _)| off)
                .unwrap_or(next_line_end - next_line_start);
        self.set_cursor_at(target_byte, extend_selection);
    }

    pub fn set_cursor_at(&mut self, byte_index: usize, extend_selection: bool) {
        self.cursor = self.clamp_boundary(byte_index);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    /// String displayed with active IME preedit inserted at cursor.
    pub fn display_text(&self) -> Cow<'_, str> {
        match &self.preedit {
            Some(pre) if !pre.is_empty() => {
                let cur = self.clamp_boundary(self.cursor);
                let mut out = String::with_capacity(self.text.len() + pre.len());
                out.push_str(&self.text[..cur]);
                out.push_str(pre);
                out.push_str(&self.text[cur..]);
                Cow::Owned(out)
            }
            _ => Cow::Borrowed(&self.text),
        }
    }

    fn clamp_boundary(&self, idx: usize) -> usize {
        let mut i = idx.min(self.text.len());
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn prev_char_boundary(&self, idx: usize) -> usize {
        if idx == 0 {
            return 0;
        }
        let mut i = idx - 1;
        while i > 0 && !self.text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn next_char_boundary(&self, idx: usize) -> usize {
        if idx >= self.text.len() {
            return self.text.len();
        }
        let mut i = idx + 1;
        while i < self.text.len() && !self.text.is_char_boundary(i) {
            i += 1;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_latin_and_unicode_scripts() {
        let mut engine = TextEngine::new();
        let style = TextStyle::default().with_size(18.0);
        let size = engine.measure("Hello CraftUI • नमस्ते • مرحبا", &style, Some(300.0));
        assert!(size.width > 0.0);
        assert!(size.height >= 18.0);
    }

    #[test]
    fn editor_selection_and_ime_preedit() {
        let mut ed = TextEditorState::new("Craft");
        ed.insert_str("UI");
        assert_eq!(ed.text, "CraftUI");
        ed.move_left(true);
        ed.move_left(true);
        assert_eq!(ed.selected_text(), "UI");
        ed.insert_str("Kit");
        assert_eq!(ed.text, "CraftKit");
        ed.preedit = Some("!".into());
        assert_eq!(ed.display_text(), "CraftKit!");
    }
}

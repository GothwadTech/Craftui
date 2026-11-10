//! Complex script shaping, font discovery, layout caching, metrics, and IME editing powered by `parley`.

use craftui_core::{Color, Rect, Size};
use parley::editing::{Cursor, Selection};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, GenericFamily,
    Layout, LayoutContext, LineHeight, StyleProperty,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "embedded-fallback-font")]
const EMBEDDED_FALLBACK_FONT: &[u8] = include_bytes!("../assets/Vera.ttf");

const DEFAULT_LAYOUT_CACHE_CAPACITY: usize = 1024;

/// Text horizontal alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutCacheKey {
    text: String,
    font_size_bits: u32,
    font_weight_bits: u32,
    italic: bool,
    font_family: Option<String>,
    line_height_bits: u32,
    align: TextAlign,
    underline: bool,
    max_width_bits: Option<u32>,
}

impl LayoutCacheKey {
    fn from_inputs(text: &str, style: &TextStyle, max_width: Option<f32>) -> Self {
        let norm_max_w = max_width
            .filter(|w| w.is_finite())
            .map(|w| if w == 0.0 { 0.0f32 } else { w.max(0.0) }.to_bits());
        let family = style
            .font_family
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(str::to_string);
        Self {
            text: text.to_string(),
            font_size_bits: style.font_size.to_bits(),
            font_weight_bits: style.font_weight.to_bits(),
            italic: style.italic,
            font_family: family,
            line_height_bits: style.line_height.to_bits(),
            align: style.align,
            underline: style.underline,
            max_width_bits: norm_max_w,
        }
    }
}

struct ShapedLayoutCache {
    entries: HashMap<LayoutCacheKey, (u64, Arc<Layout<Color>>)>,
    clock: u64,
    capacity: usize,
    hits: u64,
    misses: u64,
}

impl ShapedLayoutCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity.min(256)),
            clock: 0,
            capacity: capacity.max(16),
            hits: 0,
            misses: 0,
        }
    }

    fn get(&mut self, key: &LayoutCacheKey) -> Option<Arc<Layout<Color>>> {
        if let Some((tick, layout)) = self.entries.get_mut(key) {
            self.clock = self.clock.wrapping_add(1);
            *tick = self.clock;
            self.hits = self.hits.saturating_add(1);
            Some(Arc::clone(layout))
        } else {
            self.misses = self.misses.saturating_add(1);
            None
        }
    }

    fn insert(&mut self, key: LayoutCacheKey, layout: Arc<Layout<Color>>) {
        if self.entries.len() >= self.capacity {
            let evict_count = (self.capacity / 4).max(1);
            let mut order: Vec<(u64, LayoutCacheKey)> = self
                .entries
                .iter()
                .map(|(k, (t, _))| (*t, k.clone()))
                .collect();
            order.select_nth_unstable_by_key(evict_count.saturating_sub(1), |(t, _)| *t);
            for (_, k) in order.into_iter().take(evict_count) {
                self.entries.remove(&k);
            }
        }
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(key, (self.clock, layout));
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Shared font database, LRU shaped-layout cache, and shaping scratchpad.
pub struct TextEngine {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<Color>,
    cache: ShapedLayoutCache,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            cache: ShapedLayoutCache::new(DEFAULT_LAYOUT_CACHE_CAPACITY),
        };
        engine.install_embedded_fallback_font();
        engine
    }

    /// Create a [`TextEngine`] without discovering system fonts (relies solely on
    /// registered or embedded fallback fonts — useful for deterministic headless tests and Web/WASM).
    pub fn new_without_system_fonts() -> Self {
        let collection = fontique::Collection::new(fontique::CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let font_cx = FontContext {
            collection,
            source_cache: fontique::SourceCache::default(),
        };
        let mut engine = Self {
            font_cx,
            layout_cx: LayoutContext::new(),
            cache: ShapedLayoutCache::new(DEFAULT_LAYOUT_CACHE_CAPACITY),
        };
        engine.install_embedded_fallback_font();
        engine
    }

    fn install_embedded_fallback_font(&mut self) {
        #[cfg(feature = "embedded-fallback-font")]
        {
            let blob = fontique::Blob::new(Arc::new(EMBEDDED_FALLBACK_FONT));
            let registered = self.font_cx.collection.register_fonts(blob, None);
            let ids: Vec<_> = registered.into_iter().map(|(id, _)| id).collect();
            if !ids.is_empty() {
                self.font_cx
                    .collection
                    .append_generic_families(GenericFamily::SansSerif, ids.iter().copied());
                self.font_cx
                    .collection
                    .append_generic_families(GenericFamily::SystemUi, ids.iter().copied());
            }
        }
    }

    /// Register custom font bytes (e.g., bundled app icons or fallback font on minimal systems).
    pub fn register_font_bytes(&mut self, bytes: Vec<u8>) {
        self.font_cx.collection.register_fonts(bytes.into(), None);
        self.cache.clear();
    }

    /// Number of cache hits and misses recorded by the shaped layout LRU cache.
    pub fn cache_stats(&self) -> (u64, u64) {
        (self.cache.hits, self.cache.misses)
    }

    /// Clear all cached shaped text layouts.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    fn shape_uncached(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        brush_color: Color,
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
        builder.push_default(StyleProperty::Brush(brush_color));
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

    /// Shape and break text lines into a cached shared [`Arc<parley::Layout<Color>>`].
    ///
    /// Shaping geometry is independent of color, so hover/theme color transitions hit the cache cleanly;
    /// renderers should paint glyph runs using the caller's `style.color`.
    pub fn build_layout_cached(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> Arc<Layout<Color>> {
        let key = LayoutCacheKey::from_inputs(text, style, max_width);
        if let Some(cached) = self.cache.get(&key) {
            return cached;
        }
        let layout = Arc::new(self.shape_uncached(text, style, max_width, Color::WHITE));
        self.cache.insert(key, Arc::clone(&layout));
        layout
    }

    /// Shape and break text lines into a [`parley::Layout<Color>`].
    pub fn build_layout(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> Layout<Color> {
        let shared = self.build_layout_cached(text, style, max_width);
        (*shared).clone()
    }

    /// Measure bounding size of shaped text under optional `max_width` using the LRU cache.
    pub fn measure(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>) -> Size {
        if text.is_empty() {
            return Size::new(0.0, style.font_size * style.line_height);
        }
        let layout = self.build_layout_cached(text, style, max_width);
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextSnapshot {
    text: String,
    cursor: usize,
    anchor: usize,
}

const MAX_UNDO_HISTORY: usize = 128;

/// Interactive single/multi-line text buffer state supporting selection, word navigation, undo/redo, and IME preedit.
#[derive(Debug, Clone, Default)]
pub struct TextEditorState {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
    pub preedit: Option<String>,
    pub preedit_cursor: Option<(usize, usize)>,
    undo_stack: Vec<TextSnapshot>,
    redo_stack: Vec<TextSnapshot>,
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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn push_undo(&mut self) {
        let snap = TextSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        };
        if self.undo_stack.last() == Some(&snap) {
            return;
        }
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snap);
        self.redo_stack.clear();
    }

    /// Synchronize external signal updates while preserving cursor/selection clamped on valid UTF-8 boundaries.
    pub fn sync_external(&mut self, next: &str) -> bool {
        if self.text == next {
            return false;
        }
        self.push_undo();
        self.text.clear();
        self.text.push_str(next);
        self.cursor = self.clamp_boundary(self.cursor);
        self.anchor = self.clamp_boundary(self.anchor);
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(TextSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        self.text = prev.text;
        self.cursor = self.clamp_boundary(prev.cursor);
        self.anchor = self.clamp_boundary(prev.anchor);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(TextSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        self.text = next.text;
        self.cursor = self.clamp_boundary(next.cursor);
        self.anchor = self.clamp_boundary(next.anchor);
        true
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
        self.push_undo();
        self.delete_selection_inner();
        let idx = self.clamp_boundary(self.cursor);
        self.text.insert_str(idx, s);
        self.cursor = idx + s.len();
        self.anchor = self.cursor;
    }

    fn delete_selection_inner(&mut self) -> bool {
        if let Some((start, end)) = self.selection_range() {
            self.text.replace_range(start..end, "");
            self.cursor = start;
            self.anchor = start;
            true
        } else {
            false
        }
    }

    pub fn delete_selection(&mut self) -> bool {
        if self.selection_range().is_some() {
            self.push_undo();
            self.delete_selection_inner()
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
        self.push_undo();
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
        self.push_undo();
        let next = self.next_char_boundary(cur);
        self.text.replace_range(cur..next, "");
        self.anchor = self.cursor;
    }

    pub fn delete_word_backward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur == 0 {
            return;
        }
        self.push_undo();
        let prev = self.prev_word_boundary(cur);
        self.text.replace_range(prev..cur, "");
        self.cursor = prev;
        self.anchor = prev;
    }

    pub fn delete_word_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.clamp_boundary(self.cursor);
        if cur >= self.text.len() {
            return;
        }
        self.push_undo();
        let next = self.next_word_boundary(cur);
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

    pub fn move_word_left(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.prev_word_boundary(cur);
        if !extend_selection {
            self.anchor = self.cursor;
        }
    }

    pub fn move_word_right(&mut self, extend_selection: bool) {
        let cur = self.clamp_boundary(self.cursor);
        self.cursor = self.next_word_boundary(cur);
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

    fn prev_word_boundary(&self, idx: usize) -> usize {
        let mut cur = idx.min(self.text.len());
        let mut iter = self.text[..cur].char_indices().rev().peekable();
        // Skip trailing whitespace / punctuation before cursor
        while let Some(&(pos, ch)) = iter.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                break;
            }
            cur = pos;
            iter.next();
        }
        // Walk backwards across word characters
        for (pos, ch) in iter {
            if !(ch.is_alphanumeric() || ch == '_') {
                break;
            }
            cur = pos;
        }
        cur
    }

    fn next_word_boundary(&self, idx: usize) -> usize {
        let start = idx.min(self.text.len());
        let mut iter = self.text[start..].char_indices().peekable();
        while let Some(&(_, ch)) = iter.peek() {
            if !(ch.is_alphanumeric() || ch == '_') {
                break;
            }
            iter.next();
        }
        while let Some(&(off, ch)) = iter.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                return start + off;
            }
            iter.next();
        }
        self.text.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_latin_and_unicode_scripts_with_cache_hits() {
        let mut engine = TextEngine::new();
        let style = TextStyle::default().with_size(18.0);
        let size = engine.measure("Hello CraftUI • नमस्ते • مرحبا", &style, Some(300.0));
        assert!(size.width > 0.0);
        assert!(size.height >= 18.0);

        // Same text with different color hits the layout cache!
        let style2 = style.clone().with_color(Color::rgb(255, 0, 128));
        let size2 = engine.measure("Hello CraftUI • नमस्ते • مرحبا", &style2, Some(300.0));
        assert_eq!(size, size2);
        let (hits, misses) = engine.cache_stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
    }

    #[test]
    fn embedded_fallback_font_shapes_without_system_fonts() {
        let mut engine = TextEngine::new_without_system_fonts();
        let style = TextStyle::default().with_size(16.0);
        let layout = engine.build_layout_cached("CraftUI Fallback Font", &style, None);
        assert!(layout.width() > 50.0);
        assert!(layout.lines().count() >= 1);
    }

    #[test]
    fn editor_selection_word_motion_undo_redo_and_cursor_clamp() {
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
        ed.preedit = None;

        // Undo & Redo
        assert!(ed.undo());
        assert_eq!(ed.text, "CraftUI");
        assert!(ed.redo());
        assert_eq!(ed.text, "CraftKit");

        // Word jump and word delete
        let mut doc = TextEditorState::new("hello world_123 test");
        doc.move_word_left(false);
        assert_eq!(doc.cursor, 16);
        doc.move_word_left(false);
        assert_eq!(doc.cursor, 6);
        doc.delete_word_backward();
        assert_eq!(doc.text, "world_123 test");
        assert!(doc.undo());
        assert_eq!(doc.text, "hello world_123 test");

        // External sync preserves mid-string cursor cleanly clamped
        doc.set_cursor_at(5, false);
        assert!(doc.sync_external("hello updated string"));
        assert_eq!(doc.cursor, 5);
        assert!(doc.sync_external("hi"));
        assert_eq!(doc.cursor, 2);
    }
}

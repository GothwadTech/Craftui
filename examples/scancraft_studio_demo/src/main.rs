//! ScanCraft Studio Dogfooding Demo (PRD §7, Design Spec §2.1, Roadmap M5):
//!
//! 1. Document Library Screen: searchable `VirtualList` grid with live document thumbnails,
//!    theme toggle (`Dark` / `Light` / `AMOLED`), and `Ctrl+Shift+I` Inspector trigger.
//! 2. Filter Studio Screen: stack-routed document editor with live `PixelBufferHandle`
//!    zero-allocation filter pipeline (`Original`, `Magic Color`, `B&W`, `No Shadow`, `Warm`),
//!    Brightness/Contrast reactive sliders, and interactive 4-corner `Canvas` crop polygon + loupe.

use craftui::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub mod aot_cui {
    include!(concat!(env!("OUT_DIR"), "/filter_studio.rs"));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Library,
    FilterStudio(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterPreset {
    Original = 0,
    MagicColor = 1,
    BlackAndWhite = 2,
    NoShadow = 3,
    Warm = 4,
}

impl FilterPreset {
    pub const ALL: [Self; 5] = [
        Self::Original,
        Self::MagicColor,
        Self::BlackAndWhite,
        Self::NoShadow,
        Self::Warm,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::MagicColor => "Magic Color",
            Self::BlackAndWhite => "B&W",
            Self::NoShadow => "No Shadow",
            Self::Warm => "Warm",
        }
    }
}

#[derive(Clone)]
pub struct ScannedDoc {
    pub title: &'static str,
    pub category: &'static str,
    pub pages: usize,
    pub date: &'static str,
    pub accent: Color,
    pub seed: u32,
    pub thumb_texture: TextureHandle,
    pub source_rgba: Vec<u8>,
    pub src_width: u32,
    pub src_height: u32,
}

#[derive(Clone)]
pub struct StudioController {
    pub router: Router<Route>,
    pub search_query: Signal<String>,
    pub theme_mode_idx: Signal<usize>,
    pub inspector_enabled: Signal<bool>,
    pub docs: Rc<Vec<ScannedDoc>>,
    pub selected_filter: Signal<FilterPreset>,
    pub brightness: Signal<f32>,
    pub contrast: Signal<f32>,
    pub preview_texture: Signal<Option<TextureId>>,
    pub preview_buffer: PixelBufferHandle,
    pub status_message: Signal<String>,
    /// Normalized `(0..1, 0..1)` 4-corner crop polygon vertices (TL, TR, BR, BL)
    pub crop_corners: Signal<[Point; 4]>,
    pub active_corner: Signal<Option<usize>>,
    pub canvas_rect_cache: Rc<RefCell<Rect>>,
}

impl StudioController {
    pub fn new(cx: Scope, textures: &mut TextureRegistry) -> Self {
        let specs = [
            (
                "Q4 Tax Invoice #1042",
                "Finance",
                3,
                "Today",
                Color::rgb(46, 125, 255),
                11,
            ),
            (
                "Passport Bio Page",
                "Identity",
                1,
                "Yesterday",
                Color::rgb(0, 186, 102),
                23,
            ),
            (
                "System Architecture Notes",
                "Whiteboard",
                2,
                "Nov 08",
                Color::rgb(245, 158, 11),
                37,
            ),
            (
                "Signed NDA Agreement",
                "Legal",
                5,
                "Nov 05",
                Color::rgb(168, 85, 247),
                53,
            ),
            (
                "Lab Diagnostic Report",
                "Medical",
                2,
                "Nov 02",
                Color::rgb(239, 68, 68),
                71,
            ),
            (
                "Shipping Manifest #881",
                "Logistics",
                1,
                "Oct 29",
                Color::rgb(14, 165, 233),
                89,
            ),
            (
                "Expense Receipt — Zurich",
                "Finance",
                1,
                "Oct 25",
                Color::rgb(16, 185, 129),
                103,
            ),
            (
                "Patent Claim Draft v3",
                "Legal",
                8,
                "Oct 21",
                Color::rgb(249, 115, 22),
                127,
            ),
            (
                "University Transcript",
                "Records",
                2,
                "Oct 14",
                Color::rgb(99, 102, 241),
                149,
            ),
            (
                "Workshop Wiring Diagram",
                "Engineering",
                1,
                "Oct 10",
                Color::rgb(236, 72, 153),
                163,
            ),
            (
                "Lease Addendum",
                "Legal",
                4,
                "Oct 03",
                Color::rgb(20, 184, 166),
                179,
            ),
            (
                "Quarterly Board Deck",
                "Executive",
                12,
                "Sep 28",
                Color::rgb(234, 179, 8),
                197,
            ),
        ];

        let mut docs = Vec::with_capacity(specs.len());
        for (title, category, pages, date, accent, seed) in specs {
            let w = 240;
            let h = 300;
            let rgba = generate_synthetic_document_rgba(w, h, accent, seed);
            let thumb = textures.create_from_rgba(w, h, rgba.clone());
            docs.push(ScannedDoc {
                title,
                category,
                pages,
                date,
                accent,
                seed,
                thumb_texture: thumb,
                source_rgba: rgba,
                src_width: w,
                src_height: h,
            });
        }

        let preview_w = 240;
        let preview_h = 300;
        let initial_preview = docs[0].source_rgba.clone();
        let preview_tex = textures.create_from_rgba(preview_w, preview_h, initial_preview);
        let preview_buf = preview_tex.buffer.clone();

        let router = Router::new(cx, Route::Library);
        let selected_filter = cx.signal(FilterPreset::Original);
        let brightness = cx.signal(0.0f32);
        let contrast = cx.signal(0.0f32);
        let preview_texture = cx.signal(Some(preview_tex.id));
        let docs_rc = Rc::new(docs);

        // Live effect recomputing filtered RGBA pixels directly inside PixelBufferHandle (zero extra allocation)
        let docs_eff = docs_rc.clone();
        let pbuf_eff = preview_buf.clone();
        cx.effect(move || {
            let doc_idx = match router.current() {
                Route::FilterStudio(i) => i.min(docs_eff.len().saturating_sub(1)),
                Route::Library => 0,
            };
            let preset = selected_filter.get();
            let br = brightness.get();
            let ct = contrast.get();
            let doc = &docs_eff[doc_idx];
            pbuf_eff.with_pixels_mut(|_, _, dst| {
                apply_document_filter(
                    &doc.source_rgba,
                    dst,
                    doc.src_width,
                    doc.src_height,
                    preset,
                    br,
                    ct,
                );
            });
            pbuf_eff.commit_damage(Rect::new(
                0.0,
                0.0,
                doc.src_width as f32,
                doc.src_height as f32,
            ));
            preview_texture.set(Some(pbuf_eff.id()));
        });

        Self {
            router,
            search_query: cx.signal(String::new()),
            theme_mode_idx: cx.signal(0),
            inspector_enabled: cx.signal(false),
            docs: docs_rc,
            selected_filter,
            brightness,
            contrast,
            preview_texture,
            preview_buffer: preview_buf,
            status_message: cx.signal("Ready".to_string()),
            crop_corners: cx.signal([
                Point::new(0.10, 0.08),
                Point::new(0.90, 0.10),
                Point::new(0.88, 0.90),
                Point::new(0.12, 0.88),
            ]),
            active_corner: cx.signal(None),
            canvas_rect_cache: Rc::new(RefCell::new(Rect::ZERO)),
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.search_query.get().trim().to_ascii_lowercase();
        if q.is_empty() {
            (0..self.docs.len()).collect()
        } else {
            self.docs
                .iter()
                .enumerate()
                .filter(|(_, d)| {
                    d.title.to_ascii_lowercase().contains(&q)
                        || d.category.to_ascii_lowercase().contains(&q)
                })
                .map(|(i, _)| i)
                .collect()
        }
    }

    pub fn open_studio(&self, doc_index: usize) {
        self.selected_filter.set(FilterPreset::Original);
        self.brightness.set(0.0);
        self.contrast.set(0.0);
        self.crop_corners.set([
            Point::new(0.10, 0.08),
            Point::new(0.90, 0.10),
            Point::new(0.88, 0.90),
            Point::new(0.12, 0.88),
        ]);
        self.status_message
            .set("Drag corners to crop or pick filter".to_string());
        self.router.push(Route::FilterStudio(doc_index));
    }
}

/// Generate a synthetic paper document with header accent bar, simulated crisp text lines,
/// diagonal ambient shadow vignette, and a colored circular validation stamp.
pub fn generate_synthetic_document_rgba(
    width: u32,
    height: u32,
    accent: Color,
    seed: u32,
) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let ny = y as f32 / height.max(1) as f32;
        for x in 0..width {
            let nx = x as f32 / width.max(1) as f32;
            // Subtle scanned paper diagonal shading
            let shade = 1.0 - 0.24 * ((nx - 0.15).max(0.0) * 0.7 + (ny - 0.2).max(0.0) * 0.6);
            let mut r = (244.0 * shade).clamp(0.0, 255.0);
            let mut g = (241.0 * shade).clamp(0.0, 255.0);
            let mut b = (232.0 * shade).clamp(0.0, 255.0);

            // Header bar
            if ny > 0.07 && ny < 0.14 && nx > 0.08 && nx < 0.92 {
                r = accent.r as f32 * shade;
                g = accent.g as f32 * shade;
                b = accent.b as f32 * shade;
            }

            // Simulated dark ink text rows
            let row_slot = (ny * 26.0).floor() as u32;
            let row_frac = (ny * 26.0) - row_slot as f32;
            if (5..=21).contains(&row_slot)
                && row_frac > 0.25
                && row_frac < 0.52
                && row_slot % 2 == 1
            {
                let line_len = 0.55 + (((row_slot.wrapping_mul(37) ^ seed) % 35) as f32 / 100.0);
                if nx > 0.10 && nx < line_len {
                    r = 38.0 * shade;
                    g = 42.0 * shade;
                    b = 50.0 * shade;
                }
            }

            // Circular colored seal stamp bottom-right
            let dx = nx - 0.74;
            let dy = ny - 0.76;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > 0.08 && dist < 0.105 {
                r = accent.r as f32;
                g = accent.g as f32;
                b = accent.b as f32;
            }

            let idx = ((y * width + x) * 4) as usize;
            out[idx] = r.clamp(0.0, 255.0) as u8;
            out[idx + 1] = g.clamp(0.0, 255.0) as u8;
            out[idx + 2] = b.clamp(0.0, 255.0) as u8;
            out[idx + 3] = 255;
        }
    }
    out
}

/// Pure-Rust real-time document enhancement filter pipeline updating `dst_rgba` cleanly.
pub fn apply_document_filter(
    src_rgba: &[u8],
    dst_rgba: &mut [u8],
    width: u32,
    height: u32,
    preset: FilterPreset,
    brightness: f32,
    contrast: f32,
) {
    let n_pixels = (width * height) as usize;
    let br_offset = brightness * 1.1;
    let ct_factor = ((100.0 + contrast) / 100.0).clamp(0.2, 2.5);

    for i in 0..n_pixels {
        let base = i * 4;
        if base + 3 >= src_rgba.len() || base + 3 >= dst_rgba.len() {
            break;
        }
        let mut r = src_rgba[base] as f32;
        let mut g = src_rgba[base + 1] as f32;
        let mut b = src_rgba[base + 2] as f32;

        match preset {
            FilterPreset::Original => {}
            FilterPreset::MagicColor => {
                // Boost contrast and lift bright paper highlights
                let luma = 0.299 * r + 0.587 * g + 0.114 * b;
                if luma > 165.0 {
                    r = (r * 1.12).min(255.0);
                    g = (g * 1.12).min(255.0);
                    b = (b * 1.12).min(255.0);
                } else {
                    r = ((r - 110.0) * 1.35 + 110.0).clamp(0.0, 255.0);
                    g = ((g - 110.0) * 1.35 + 110.0).clamp(0.0, 255.0);
                    b = ((b - 110.0) * 1.35 + 110.0).clamp(0.0, 255.0);
                }
            }
            FilterPreset::BlackAndWhite => {
                let luma = 0.299 * r + 0.587 * g + 0.114 * b;
                let bw = if luma > 148.0 { 255.0 } else { 18.0 };
                r = bw;
                g = bw;
                b = bw;
            }
            FilterPreset::NoShadow => {
                let px = (i as u32 % width) as f32 / width.max(1) as f32;
                let py = (i as u32 / width) as f32 / height.max(1) as f32;
                let inv_shade = 1.0
                    / (1.0 - 0.24 * ((px - 0.15).max(0.0) * 0.7 + (py - 0.2).max(0.0) * 0.6))
                        .max(0.5);
                r = (r * inv_shade).clamp(0.0, 255.0);
                g = (g * inv_shade).clamp(0.0, 255.0);
                b = (b * inv_shade).clamp(0.0, 255.0);
            }
            FilterPreset::Warm => {
                r = (r * 1.08 + 12.0).clamp(0.0, 255.0);
                g = (g * 1.01 + 4.0).clamp(0.0, 255.0);
                b = (b * 0.86).clamp(0.0, 255.0);
            }
        }

        // Manual Brightness + Contrast
        r = ((r - 128.0) * ct_factor + 128.0 + br_offset).clamp(0.0, 255.0);
        g = ((g - 128.0) * ct_factor + 128.0 + br_offset).clamp(0.0, 255.0);
        b = ((b - 128.0) * ct_factor + 128.0 + br_offset).clamp(0.0, 255.0);

        dst_rgba[base] = r as u8;
        dst_rgba[base + 1] = g as u8;
        dst_rgba[base + 2] = b as u8;
        dst_rgba[base + 3] = 255;
    }
}

pub fn mount_scancraft_app(
    cx: Scope,
    tree: &mut WidgetTree,
    textures: &mut TextureRegistry,
) -> (NodeId, StudioController) {
    let ctrl = StudioController::new(cx, textures);
    let ctrl_key = ctrl.clone();
    let ctrl_view = ctrl.clone();

    let root = tree.dynamic_slot(
        cx,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_safe_area(true),
        move || ctrl_key.router.current(),
        move |scope, route, t| match route {
            Route::Library => build_library_screen(scope, t, &ctrl_view),
            Route::FilterStudio(doc_idx) => {
                build_filter_studio_screen(scope, t, &ctrl_view, *doc_idx)
            }
        },
    );
    tree.set_root(root);
    (root, ctrl)
}

fn build_library_screen(scope: Scope, tree: &mut WidgetTree, ctrl: &StudioController) -> NodeId {
    let theme_sig = tree.theme_signal;
    let page = tree.column(scope);
    tree.set_layout(
        page,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_padding(Insets::all(16.0))
            .with_gap(12.0),
    );
    tree.set_box_style(
        page,
        BoxStyle {
            background: Some(ReactiveValue::Dynamic(Rc::new(move || {
                Brush::Solid(theme_sig.get().background)
            }))),
            ..BoxStyle::new()
        },
    );

    // Top bar: Title + Theme switcher
    let header = tree.row(scope);
    tree.set_layout(
        header,
        LayoutStyle::row()
            .with_size(pct(100), Length::Dp(44.0))
            .with_align(Alignment::Center)
            .with_gap(12.0),
    );
    let title = tree.text(
        scope,
        "ScanCraft Library",
        ReactiveValue::Dynamic(Rc::new(move || {
            TextStyle::default()
                .with_size(22.0)
                .with_weight(700.0)
                .with_color(theme_sig.get().text_primary)
        })),
    );
    let spacer = tree.spacer(scope);
    let modes = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::Amoled];
    let mode_idx = ctrl.theme_mode_idx;
    let theme_btn = tree.button(
        scope,
        move || match modes[mode_idx.get() % modes.len()] {
            ThemeMode::Dark => "Theme: Dark".to_string(),
            ThemeMode::Light => "Theme: Light".to_string(),
            ThemeMode::Amoled => "Theme: AMOLED".to_string(),
            ThemeMode::HighContrast => "Theme: High Contrast".to_string(),
        },
        move || {
            let next = (mode_idx.get_untracked() + 1) % modes.len();
            mode_idx.set(next);
            theme_sig.set(ThemeTokens::from_mode(modes[next]));
        },
    );
    tree.append_child(header, title);
    tree.append_child(header, spacer);
    tree.append_child(header, theme_btn);

    // Search Bar
    let search_bar = tree.text_input(
        scope,
        "",
        "Search scans by title or category...",
        Some(ctrl.search_query),
    );
    tree.set_layout(
        search_bar,
        LayoutStyle::row()
            .with_size(pct(100), Length::Dp(42.0))
            .with_padding(Insets::symmetric(12.0, 8.0)),
    );

    // Filtered Virtualized 2-Column Document Grid inside dynamic_slot
    let ctrl_q = ctrl.clone();
    let ctrl_grid = ctrl.clone();
    let grid_slot = tree.dynamic_slot(
        scope,
        LayoutStyle::column()
            .with_size(pct(100), auto())
            .with_flex_grow(1.0),
        move || ctrl_q.search_query.get(),
        move |s, _query, t| {
            let indices = ctrl_grid.filtered_indices();
            let count = indices.len();
            let docs = ctrl_grid.docs.clone();
            let ctrl_card = ctrl_grid.clone();
            let theme_c = t.theme_signal;

            t.virtual_list(
                s,
                Axis::Vertical,
                2,
                190.0,
                count,
                LayoutStyle::column().with_size(pct(100), pct(100)),
                Rc::new(move |item_scope, idx, inner_tree| {
                    let doc_idx = indices[idx];
                    let doc = &docs[doc_idx];
                    let ctrl_tap = ctrl_card.clone();

                    let card = inner_tree.container(
                        item_scope,
                        LayoutStyle::row()
                            .with_padding(Insets::all(10.0))
                            .with_margin(Insets::all(6.0))
                            .with_gap(12.0),
                        BoxStyle {
                            background: Some(ReactiveValue::Dynamic(Rc::new(move || {
                                Brush::Solid(theme_c.get().surface)
                            }))),
                            border: Some(craftui::core::Border {
                                width: 1.0,
                                color: doc.accent.with_alpha(0.45),
                                radius: CornerRadii::all(12.0),
                            }),
                            radius: CornerRadii::all(12.0),
                            ..BoxStyle::new()
                        },
                    );
                    inner_tree.set_on_tap(card, move || ctrl_tap.open_studio(doc_idx));

                    let thumb = inner_tree.image(
                        item_scope,
                        Some(doc.thumb_texture.id),
                        Fit::Contain,
                        CornerRadii::all(8.0),
                    );
                    inner_tree.set_layout(
                        thumb,
                        LayoutStyle::column().with_size(Length::Dp(108.0), Length::Dp(148.0)),
                    );

                    let meta = inner_tree.column(item_scope);
                    inner_tree.set_layout(
                        meta,
                        LayoutStyle::column().with_flex_grow(1.0).with_gap(6.0),
                    );
                    let t_title = inner_tree.text(
                        item_scope,
                        doc.title,
                        ReactiveValue::Dynamic(Rc::new(move || {
                            TextStyle::default()
                                .with_size(15.0)
                                .with_weight(600.0)
                                .with_color(theme_c.get().text_primary)
                        })),
                    );
                    let t_sub = inner_tree.text(
                        item_scope,
                        format!(
                            "{}  •  {} page(s)  •  {}",
                            doc.category, doc.pages, doc.date
                        ),
                        ReactiveValue::Dynamic(Rc::new(move || {
                            TextStyle::default()
                                .with_size(12.0)
                                .with_color(theme_c.get().text_secondary)
                        })),
                    );
                    let sp = inner_tree.spacer(item_scope);
                    let ctrl_btn = ctrl_card.clone();
                    let edit_btn = inner_tree.button(item_scope, "Open Filter Studio", move || {
                        ctrl_btn.open_studio(doc_idx);
                    });

                    inner_tree.append_child(meta, t_title);
                    inner_tree.append_child(meta, t_sub);
                    inner_tree.append_child(meta, sp);
                    inner_tree.append_child(meta, edit_btn);
                    inner_tree.append_child(card, thumb);
                    inner_tree.append_child(card, meta);
                    card
                }),
            )
        },
    );

    tree.append_child(page, header);
    tree.append_child(page, search_bar);
    tree.append_child(page, grid_slot);
    page
}

fn build_filter_studio_screen(
    scope: Scope,
    tree: &mut WidgetTree,
    ctrl: &StudioController,
    doc_idx: usize,
) -> NodeId {
    let theme_sig = tree.theme_signal;
    let doc = &ctrl.docs[doc_idx.min(ctrl.docs.len() - 1)];

    let page = tree.column(scope);
    tree.set_layout(
        page,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_padding(Insets::all(16.0))
            .with_gap(12.0),
    );
    tree.set_box_style(
        page,
        BoxStyle {
            background: Some(ReactiveValue::Dynamic(Rc::new(move || {
                Brush::Solid(theme_sig.get().background)
            }))),
            ..BoxStyle::new()
        },
    );

    // Top Navigation Bar
    let top_bar = tree.row(scope);
    tree.set_layout(
        top_bar,
        LayoutStyle::row()
            .with_size(pct(100), Length::Dp(42.0))
            .with_align(Alignment::Center)
            .with_gap(12.0),
    );
    let r_back = ctrl.router;
    let back_btn = tree.button(scope, "< Library", move || {
        r_back.pop();
    });
    let doc_label = tree.text(
        scope,
        format!("Filter Studio — {}", doc.title),
        ReactiveValue::Dynamic(Rc::new(move || {
            TextStyle::default()
                .with_size(18.0)
                .with_weight(700.0)
                .with_color(theme_sig.get().text_primary)
        })),
    );
    let sp = tree.spacer(scope);
    let status_sig = ctrl.status_message;
    let preset_sig = ctrl.selected_filter;
    let save_btn = tree.button(scope, "Save Scan", move || {
        status_sig.set(format!(
            "Saved scan ({})",
            preset_sig.get_untracked().label()
        ));
    });
    tree.append_child(top_bar, back_btn);
    tree.append_child(top_bar, doc_label);
    tree.append_child(top_bar, sp);
    tree.append_child(top_bar, save_btn);

    // Center Stack: Live PixelBuffer ImageView + Interactive 4-Corner Crop Canvas Overlay
    let viewport = tree.stack(scope);
    tree.set_layout(
        viewport,
        LayoutStyle::stack()
            .with_size(pct(100), auto())
            .with_flex_grow(1.0),
    );
    tree.set_box_style(
        viewport,
        BoxStyle {
            background: Some(ReactiveValue::Static(Brush::Solid(Color::rgb(12, 16, 22)))),
            radius: CornerRadii::all(12.0),
            clip: true,
            ..BoxStyle::new()
        },
    );

    let img = tree.image(
        scope,
        ctrl.preview_texture,
        Fit::Contain,
        CornerRadii::all(8.0),
    );
    tree.set_layout(img, LayoutStyle::column().with_size(pct(100), pct(100)));

    let corners_paint = ctrl.crop_corners;
    let active_paint = ctrl.active_corner;
    let rect_cache_paint = ctrl.canvas_rect_cache.clone();
    let corners_ptr = ctrl.crop_corners;
    let active_ptr = ctrl.active_corner;
    let status_ptr = ctrl.status_message;

    let overlay = tree.canvas(
        scope,
        LayoutStyle::column().with_size(pct(100), pct(100)),
        move |rect, scene| {
            *rect_cache_paint.borrow_mut() = rect;
            let pts = corners_paint.get();
            let active = active_paint.get();
            let to_screen =
                |p: Point| Point::new(rect.x + p.x * rect.width, rect.y + p.y * rect.height);
            let spts = [
                to_screen(pts[0]),
                to_screen(pts[1]),
                to_screen(pts[2]),
                to_screen(pts[3]),
            ];

            // Crop polygon outline
            let mut quad = kurbo::BezPath::new();
            quad.move_to(spts[0].to_kurbo());
            quad.line_to(spts[1].to_kurbo());
            quad.line_to(spts[2].to_kurbo());
            quad.line_to(spts[3].to_kurbo());
            quad.close_path();
            scene.push(PaintCommand::StrokePath {
                path: quad,
                color: Color::rgb(0, 210, 120),
                width: 2.5,
            });

            // Corner grip handles
            for (idx, sp) in spts.iter().enumerate() {
                let is_active = active == Some(idx);
                let r = if is_active { 9.0 } else { 7.0 };
                let handle_rect = Rect::new(sp.x - r, sp.y - r, r * 2.0, r * 2.0);
                scene.fill_rect(
                    handle_rect,
                    if is_active {
                        Color::rgb(255, 196, 0)
                    } else {
                        Color::rgb(0, 210, 120)
                    },
                    CornerRadii::all(r),
                );
                scene.stroke_rect(handle_rect, Color::WHITE, 2.0, CornerRadii::all(r));
            }

            // Magnifier Loupe reticle when dragging a corner handle
            if let Some(a_idx) = active {
                let target = spts[a_idx];
                let loupe = Rect::new(rect.x + 14.0, rect.y + 14.0, 74.0, 74.0);
                scene.fill_rect(loupe, Color::rgba(16, 22, 32, 230), CornerRadii::all(37.0));
                scene.stroke_rect(loupe, Color::rgb(255, 196, 0), 2.0, CornerRadii::all(37.0));
                scene.draw_text(
                    Point::new(loupe.x + 12.0, loupe.y + 28.0),
                    format!("C{} {:.0},{:.0}", a_idx + 1, target.x, target.y),
                    TextStyle::default()
                        .with_size(11.0)
                        .with_color(Color::WHITE),
                    Some(60.0),
                );
            }
        },
        Some(Rc::new(RefCell::new(
            move |rect: Rect, pe: &PointerEvent| {
                let nx = ((pe.position.x - rect.x) / rect.width.max(1.0)).clamp(0.02, 0.98);
                let ny = ((pe.position.y - rect.y) / rect.height.max(1.0)).clamp(0.02, 0.98);
                match pe.phase {
                    PointerPhase::Down => {
                        let pts = corners_ptr.get_untracked();
                        let mut best_idx = None;
                        let mut best_dist = 28.0f32;
                        for (i, p) in pts.iter().enumerate() {
                            let px = rect.x + p.x * rect.width;
                            let py = rect.y + p.y * rect.height;
                            let d = Point::new(px, py).distance_to(pe.position);
                            if d < best_dist {
                                best_dist = d;
                                best_idx = Some(i);
                            }
                        }
                        active_ptr.set(best_idx);
                    }
                    PointerPhase::Move => {
                        if let Some(idx) = active_ptr.get_untracked() {
                            corners_ptr.update(|arr| {
                                arr[idx] = Point::new(nx, ny);
                            });
                            status_ptr.set(format!(
                                "Adjusted crop corner {} ({:.2}, {:.2})",
                                idx + 1,
                                nx,
                                ny
                            ));
                        }
                    }
                    PointerPhase::Up | PointerPhase::Cancel => {
                        active_ptr.set(None);
                    }
                }
            },
        ))),
    );
    tree.append_child(viewport, img);
    tree.append_child(viewport, overlay);

    // Brightness & Contrast Sliders
    let sliders_col = tree.column(scope);
    tree.set_layout(
        sliders_col,
        LayoutStyle::column()
            .with_size(pct(100), auto())
            .with_gap(6.0),
    );
    let br_slider = tree.slider(scope, ctrl.brightness, -100.0, 100.0, "Brightness");
    let ct_slider = tree.slider(scope, ctrl.contrast, -100.0, 100.0, "Contrast");
    tree.append_child(sliders_col, br_slider);
    tree.append_child(sliders_col, ct_slider);

    // Horizontal Filter Strip
    let strip = tree.scroll_view(
        scope,
        Axis::Horizontal,
        LayoutStyle::row().with_size(pct(100), Length::Dp(52.0)),
    );
    let strip_row = tree.row(scope);
    tree.set_layout(strip_row, LayoutStyle::row().with_gap(10.0));
    for preset in FilterPreset::ALL {
        let p_sig = ctrl.selected_filter;
        let st_sig = ctrl.status_message;
        let chip = tree.button(scope, preset.label(), move || {
            p_sig.set(preset);
            st_sig.set(format!("Filter: {}", preset.label()));
        });
        tree.append_child(strip_row, chip);
    }
    tree.append_child(strip, strip_row);

    let st_line = ctrl.status_message;
    let footer = tree.text(
        scope,
        move || st_line.get(),
        ReactiveValue::Dynamic(Rc::new(move || {
            TextStyle::default()
                .with_size(13.0)
                .with_color(theme_sig.get().primary)
        })),
    );

    tree.append_child(page, top_bar);
    tree.append_child(page, viewport);
    tree.append_child(page, sliders_col);
    tree.append_child(page, strip);
    tree.append_child(page, footer);
    page
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "ScanCraft Studio — CraftUI Demo".to_string(),
            width: 960.0,
            height: 760.0,
            ..Default::default()
        },
        |cx, tree, ctx| {
            let (root, _ctrl) = mount_scancraft_app(cx, tree, &mut ctx.textures);
            root
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::reactive::reset_runtime;

    #[test]
    fn scancraft_two_screen_navigation_filter_and_crop_drag_work_and_render() {
        reset_runtime();
        let cx = Scope::root();
        let mut textures = TextureRegistry::new();
        let mut tree = WidgetTree::new(cx);
        let (_root, ctrl) = mount_scancraft_app(cx, &mut tree, &mut textures);

        let metrics = WindowMetrics {
            logical_size: Size::new(900.0, 720.0),
            scale_factor: 1.0,
            ..Default::default()
        };

        // Screen 1: Document Library
        tree.layout(metrics);
        let mut scene1 = PaintScene::new();
        tree.paint(&mut scene1);
        let lib_rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene1,
            900,
            720,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(lib_rgba.len(), 900 * 720 * 4);
        assert!(lib_rgba.iter().any(|&b| b > 100));

        // Search filter reduces items
        ctrl.search_query.set("Passport".to_string());
        assert_eq!(ctrl.filtered_indices(), vec![1]);
        ctrl.search_query.set(String::new());

        // Navigate to Screen 2: Filter Studio
        ctrl.open_studio(0);
        tree.layout(metrics);

        let v0 = ctrl.preview_buffer.version();
        ctrl.selected_filter.set(FilterPreset::BlackAndWhite);
        ctrl.brightness.set(15.0);
        ctrl.contrast.set(25.0);
        let v1 = ctrl.preview_buffer.version();
        assert!(
            v1 > v0,
            "filter pipeline should commit damaged pixels to PixelBufferHandle"
        );

        // Verify pixels actually binarized under B&W
        let (_, _, _, rgba_buf) = ctrl.preview_buffer.snapshot();
        assert!(
            rgba_buf
                .as_chunks::<4>()
                .0
                .iter()
                .all(|px| px[0] == px[1] && px[1] == px[2])
        );

        // Paint once so Canvas bounds are recorded
        let mut scene2 = PaintScene::new();
        tree.paint(&mut scene2);
        let cr = *ctrl.canvas_rect_cache.borrow();
        assert!(cr.width > 100.0 && cr.height > 100.0);
        let p0_orig = ctrl.crop_corners.get()[0];
        let c0_screen = Point::new(cr.x + p0_orig.x * cr.width, cr.y + p0_orig.y * cr.height);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: c0_screen,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Move,
            position: Point::new(c0_screen.x + 25.0, c0_screen.y + 18.0),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 30,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: Point::new(c0_screen.x + 25.0, c0_screen.y + 18.0),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 50,
        }));
        let p0_new = ctrl.crop_corners.get()[0];
        assert!(p0_new.x > p0_orig.x && p0_new.y > p0_orig.y);

        // Render Screen 2 snapshot headlessly via VelloCpu
        tree.layout(metrics);
        tree.paint(&mut scene2);
        let studio_rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene2,
            900,
            720,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(studio_rgba.len(), 900 * 720 * 4);
        assert_ne!(lib_rgba, studio_rgba);

        // Verify AOT compiled .cui FilterStudioTemplate instantiates cleanly
        let tpl = aot_cui::FilterStudioTemplate::new(cx);
        let mut aot_tree = WidgetTree::new(cx);
        let aot_root = tpl.build(cx, &mut aot_tree);
        aot_tree.set_root(aot_root);
        aot_tree.layout(metrics);
        assert_eq!(tpl.selected_filter.get(), 0);
    }
}

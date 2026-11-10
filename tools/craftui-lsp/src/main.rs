use craftui_dsl::{
    BUILTIN_EVENTS, BUILTIN_PROPS, BUILTIN_WIDGETS, Severity, SyntaxKind, parse, resolve_source,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct CuiBackend {
    client: Client,
    docs: Arc<RwLock<HashMap<Url, String>>>,
}

impl CuiBackend {
    fn new(client: Client) -> Self {
        Self {
            client,
            docs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn update_document(&self, uri: Url, text: String) {
        let diags = analyze_cui_document(Some(&uri), &text);
        self.docs.write().await.insert(uri.clone(), text);
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for CuiBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), ":".into(), " ".into()]),
                    ..Default::default()
                }),
                color_provider: Some(ColorProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "craftui-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "CraftUI .cui Language Server initialized",
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.update_document(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.update_document(params.text_document.uri, change.text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.write().await.remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let text = self.docs.read().await.get(uri).cloned().unwrap_or_default();
        Ok(Some(CompletionResponse::Array(build_completions(
            &text, pos,
        ))))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let text = self.docs.read().await.get(uri).cloned().unwrap_or_default();
        Ok(lookup_hover(&text, pos))
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        let text = self
            .docs
            .read()
            .await
            .get(&params.text_document.uri)
            .cloned()
            .unwrap_or_default();
        Ok(extract_color_information(&text))
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        let c = params.color;
        let r = (c.red * 255.0).round() as u8;
        let g = (c.green * 255.0).round() as u8;
        let b = (c.blue * 255.0).round() as u8;
        let a = (c.alpha * 255.0).round() as u8;
        let hex = if a == 255 {
            format!("#{r:02X}{g:02X}{b:02X}")
        } else {
            format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
        };
        Ok(vec![ColorPresentation {
            label: hex.clone(),
            text_edit: Some(TextEdit {
                range: params.range,
                new_text: hex,
            }),
            additional_text_edits: None,
        }])
    }
}

pub fn analyze_cui_diagnostics(text: &str) -> Vec<Diagnostic> {
    analyze_cui_document(None, text)
}

pub fn analyze_cui_document(uri: Option<&Url>, text: &str) -> Vec<Diagnostic> {
    let base_dir: Option<PathBuf> = uri
        .and_then(|u| u.to_file_path().ok())
        .and_then(|p| p.parent().map(PathBuf::from));
    let resolved = resolve_source(text, base_dir.as_deref());

    resolved
        .diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            range: Range {
                start: offset_to_position(text, d.span.start.min(text.len())),
                end: offset_to_position(text, d.span.end.min(text.len())),
            },
            severity: Some(match d.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
            }),
            source: Some("craftui-dsl".to_string()),
            message: d.message,
            ..Default::default()
        })
        .collect()
}

pub fn extract_color_information(text: &str) -> Vec<ColorInformation> {
    let parsed = parse(text);
    let mut out = Vec::new();
    for elem in parsed.cst.descendants_with_tokens() {
        if let Some(tok) = elem.as_token()
            && tok.kind() == SyntaxKind::ColorHex
            && let Some(c) = craftui_core::Color::from_hex(tok.text())
        {
            let range_bytes = tok.text_range();
            let start = usize::from(range_bytes.start());
            let end = usize::from(range_bytes.end());
            out.push(ColorInformation {
                range: Range {
                    start: offset_to_position(text, start),
                    end: offset_to_position(text, end),
                },
                color: Color {
                    red: c.r as f32 / 255.0,
                    green: c.g as f32 / 255.0,
                    blue: c.b as f32 / 255.0,
                    alpha: c.a as f32 / 255.0,
                },
            });
        }
    }
    out
}

pub fn build_completions(text: &str, pos: Position) -> Vec<CompletionItem> {
    let offset = position_to_offset(text, pos);
    let prefix = &text[..offset.min(text.len())];

    if prefix.ends_with("Theme.") {
        return [
            ("background", "Background surface color token"),
            ("surface", "Card/panel surface color token"),
            ("surface_variant", "Muted container surface color token"),
            ("border", "Default subtle border color"),
            ("primary", "Accent / action color token"),
            ("on_primary", "Foreground text color on primary"),
            ("text_primary", "Primary body/heading text color"),
            ("text_secondary", "Muted caption/label text color"),
            ("danger", "Destructive/error accent color"),
        ]
        .into_iter()
        .map(|(k, d)| CompletionItem {
            label: k.to_string(),
            kind: Some(CompletionItemKind::COLOR),
            detail: Some(d.to_string()),
            ..Default::default()
        })
        .collect();
    }

    let mut items = Vec::new();
    for &w in BUILTIN_WIDGETS {
        items.push(CompletionItem {
            label: w.to_string(),
            kind: Some(CompletionItemKind::STRUCT),
            detail: Some("CraftUI Built-in Widget".to_string()),
            ..Default::default()
        });
    }
    for &p in BUILTIN_PROPS {
        items.push(CompletionItem {
            label: p.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some("CraftUI Layout / Style Property".to_string()),
            ..Default::default()
        });
    }
    for &ev in BUILTIN_EVENTS {
        items.push(CompletionItem {
            label: ev.to_string(),
            kind: Some(CompletionItemKind::EVENT),
            detail: Some("CraftUI Widget Event Handler".to_string()),
            ..Default::default()
        });
    }
    let parsed = parse(text);
    for comp in &parsed.file.components {
        items.push(CompletionItem {
            label: comp.name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("User .cui Component".to_string()),
            ..Default::default()
        });
    }
    for st in &parsed.file.structs {
        items.push(CompletionItem {
            label: st.name.clone(),
            kind: Some(CompletionItemKind::STRUCT),
            detail: Some("User .cui Struct".to_string()),
            ..Default::default()
        });
    }
    for en in &parsed.file.enums {
        items.push(CompletionItem {
            label: en.name.clone(),
            kind: Some(CompletionItemKind::ENUM),
            detail: Some("User .cui Enum".to_string()),
            ..Default::default()
        });
    }
    for kw in [
        "export component",
        "export struct",
        "export enum",
        "import { } from \"\"",
        "in property",
        "out property",
        "in-out property",
        "callback",
        "Theme",
        "ThemeMode",
        "SafeArea.insets",
        "for item, idx in",
        "if",
        "else",
        "on_tap =>",
        "on_change =>",
        "on_submit =>",
        "on_close =>",
    ] {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }
    items
}

pub fn lookup_hover(text: &str, pos: Position) -> Option<Hover> {
    let offset = position_to_offset(text, pos);
    let word = extract_word(text, offset)?;
    let doc = match word.as_str() {
        "Column" => "Vertical Flexbox container (`FlexDirection::Column`).",
        "Row" => "Horizontal Flexbox container (`FlexDirection::Row`).",
        "Grid" => "CSS Grid container (`columns`, `gap`).",
        "Stack" => "Overlay / Z-stack positioning container.",
        "Spacer" => "Flexible space-filling node (`flex_grow: 1.0`).",
        "SafeArea" => {
            "Hardware cutout / Dynamic Island / gesture-bar + keyboard inset token (`SafeArea.insets`)."
        }
        "Divider" => {
            "Horizontal or vertical hairline separator (`direction`, `thickness`, `color`)."
        }
        "Text" => {
            "Multi-script shaped text rendered with Parley & Vello (`text`, `font_size`, `font_weight`, `color`)."
        }
        "Icon" => "Scalable vector SVG / built-in preset icon (`svg`, `size`, `color`).",
        "Button" => {
            "Accessible interactive button with hover/press feedback (`text`, `background`, `foreground`, `on_tap`)."
        }
        "ImageView" | "Image" => {
            "Zero-copy GPU texture / PixelBuffer viewport (`source`, `fit`, `border_radius`)."
        }
        "Canvas" => "Immediate-mode Vello vector scene widget with raw multi-touch pointer events.",
        "ScrollView" => "Momentum scrolling container (`direction: Vertical | Horizontal`).",
        "VirtualList" | "VirtualGrid" => {
            "Windowed recycling list/grid capable of 10k+ items at 120fps."
        }
        "Slider" => {
            "Accessible continuous range slider supporting `<=>` two-way binding (`min`, `max`, `value`, `on_change`)."
        }
        "TextInput" => {
            "Single-line text editor with selection, clipboard shortcuts, IME, and `<=>` two-way `value` binding."
        }
        "TextArea" => {
            "Multi-line text editor supporting newlines, Up/Down cursor motion, and `<=>` two-way `value` binding."
        }
        "Checkbox" | "Switch" => {
            "Boolean toggle control supporting `<=>` two-way `checked` binding and `on_change`."
        }
        "Tabs" | "TabBar" => "Tab navigation bar (`tabs`, `<=> active`, `on_change`).",
        "Select" | "Dropdown" => {
            "Single-selection dropdown picker with floating menu (`options`, `<=> selected`, `placeholder`)."
        }
        "RadioGroup" => "Exclusive choice radio group (`options`, `<=> selected`, `direction`).",
        "PopupMenu" => {
            "Anchored context / action menu dismissed on outside click or `Escape` (`open`, `items`, `on_select`, `on_close`)."
        }
        "Tooltip" => "Informational callout rendered on hover or focus (`text`).",
        "ProgressIndicator" => {
            "Determinate (`0.0..=1.0`) or indeterminate animated progress bar (`progress`)."
        }
        "Modal" | "BottomSheet" => {
            "Focus-trapping overlay / sheet with backdrop click & `Escape` dismissal (`open`, `on_close`)."
        }
        "PlatformView" => "Native platform view / texture hole-punch container.",
        "Theme" => {
            "Reactive `ThemeTokens` design system palette (`background`, `surface`, `primary`, `border`, etc.)."
        }
        "ThemeMode" => "Theme variant enum (`Light`, `Dark`, `Amoled`, `HighContrast`).",
        w if BUILTIN_EVENTS.contains(&w) => "CraftUI widget event handler (`on_tap => { ... }`).",
        w if BUILTIN_PROPS.contains(&w) => {
            "CraftUI element property (`px`, `dp`, `%`, Color, Enum, or reactive binding)."
        }
        _ => return None,
    };

    Some(Hover {
        contents: HoverContents::Scalar(MarkedString::String(doc.to_string())),
        range: None,
    })
}

fn extract_word(text: &str, byte_offset: usize) -> Option<String> {
    let bytes = text.as_bytes();
    if byte_offset >= bytes.len() {
        return None;
    }
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    let mut start = byte_offset;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = byte_offset;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    if start >= end {
        None
    } else {
        Some(text[start..end].to_string())
    }
}

pub fn offset_to_position(text: &str, byte_offset: usize) -> Position {
    let clamped = byte_offset.min(text.len());
    let slice = &text[..clamped];
    let mut line = 0u32;
    let mut last_nl = 0usize;
    for (idx, b) in slice.bytes().enumerate() {
        if b == b'\n' {
            line += 1;
            last_nl = idx + 1;
        }
    }
    let col = slice[last_nl..].encode_utf16().count() as u32;
    Position {
        line,
        character: col,
    }
}

pub fn position_to_offset(text: &str, pos: Position) -> usize {
    let mut cur_line = 0u32;
    let mut line_start = 0usize;
    for (idx, b) in text.bytes().enumerate() {
        if cur_line == pos.line {
            break;
        }
        if b == b'\n' {
            cur_line += 1;
            line_start = idx + 1;
        }
    }
    let line_str = &text[line_start..];
    let line_end_rel = line_str.find('\n').unwrap_or(line_str.len());
    let line_slice = &line_str[..line_end_rel];
    let mut utf16_seen = 0u32;
    for (byte_idx, ch) in line_slice.char_indices() {
        if utf16_seen >= pos.character {
            return line_start + byte_idx;
        }
        utf16_seen += ch.len_utf16() as u32;
    }
    line_start + line_slice.len()
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(CuiBackend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_diagnostics_color_provider_completions_and_hover() {
        let src = r#"export component Card {
    Column {
        background: #1A2B3C;
        Text { text: "Hello"; }
    }
}
"#;
        assert!(analyze_cui_diagnostics(src).is_empty());

        let colors = extract_color_information(src);
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0].range.start.line, 2);
        assert!((colors[0].color.red - (0x1A as f32 / 255.0)).abs() < 0.01);

        let theme_comps = build_completions("background: Theme.", Position::new(0, 18));
        assert!(theme_comps.iter().any(|c| c.label == "primary"));

        let hover = lookup_hover(src, Position::new(1, 5)).expect("hover on Column");
        match hover.contents {
            HoverContents::Scalar(MarkedString::String(s)) => assert!(s.contains("Vertical")),
            _ => panic!("expected markdown scalar"),
        }

        let bad = "export component Broken { UnknownWidget { } }";
        let diags = analyze_cui_diagnostics(bad);
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::WARNING))
        );
    }

    #[test]
    fn lsp_multifile_imports_events_structs_and_new_widget_hover() {
        let tmp = std::env::temp_dir().join("craftui_lsp_import_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let types_path = tmp.join("types.cui");
        let app_path = tmp.join("app.cui");
        std::fs::write(
            &types_path,
            r#"export enum FilterKind { All, Open }
export struct ItemRow { id: int, label: string }
export component Badge {
    in property <string> text: "";
    Text { text: text; }
}
"#,
        )
        .unwrap();

        let app_src = r#"import { FilterKind, ItemRow, Badge } from "./types.cui";
export component App {
    in-out property <string> query: "";
    Column {
        Tabs { tabs: ["A", "B"]; active: 0; on_change => { } }
        TextInput { value <=> query; on_submit => { } }
        Badge { text: "ok"; }
    }
}
"#;
        std::fs::write(&app_path, app_src).unwrap();
        let url = Url::from_file_path(&app_path).unwrap();
        let diags = analyze_cui_document(Some(&url), app_src);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

        let comps = build_completions(app_src, Position::new(4, 12));
        assert!(comps.iter().any(|c| c.label == "Tabs"));
        assert!(comps.iter().any(|c| c.label == "PopupMenu"));
        assert!(comps.iter().any(|c| c.label == "on_submit"));
        assert!(comps.iter().any(|c| c.label == "on_double_tap"));

        let hover_tabs = lookup_hover(app_src, Position::new(4, 9)).expect("hover on Tabs");
        match hover_tabs.contents {
            HoverContents::Scalar(MarkedString::String(s)) => assert!(s.contains("Tab")),
            _ => panic!("expected scalar hover"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

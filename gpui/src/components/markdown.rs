use super::{
    font as native_font,
    support::{self, tokens},
    syntax,
};
use gpui_kit::{
    App, ClipboardItem, Context, Entity, IntoElement, Render, Subscription, Task, TestSupportExt,
    TextRun, Window,
    base::text::CodeBlock,
    base::{
        Button, MarkdownBlockSpacing, MarkdownExtensions, MarkdownTaskCheckbox, TextView,
        TextViewState, TextViewStyle,
    },
    div, font,
    prelude::*,
    px, rgb,
};
use std::time::Duration;

pub fn extensions() -> MarkdownExtensions {
    MarkdownExtensions::default()
        .soft_breaks(true)
        .math(false)
        .skip_empty_table_header(true)
}

pub fn text_style(dark: bool, window: &Window) -> TextViewStyle {
    text_style_at_size(dark, 13., window)
}

pub fn text_style_at_size(dark: bool, body: f32, window: &Window) -> TextViewStyle {
    let p = tokens(dark);
    // CSS 80ch is eighty advances of the body font's zero glyph, not eighty characters of prose.
    let zero_width = window
        .text_system()
        .layout_line(
            "0",
            px(body),
            &[TextRun {
                len: 1,
                font: font(native_font::family()),
                color: rgb(p.text).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        )
        .width;
    let mut table_style = gpui_kit::StyleRefinement::default()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(p.border))
        .bg(gpui_kit::rgba(0x00000000));
    table_style.overflow.x = Some(gpui_kit::Overflow::Scroll);
    TextViewStyle::default()
        .with_prose_max_width(zero_width * 80.)
        .with_strong_weight(gpui_kit::FontWeight::SEMIBOLD)
        .with_dark(dark)
        .with_code_block_horizontal_scroll(true)
        .with_body_text_size(px(body))
        .with_block_spacing(MarkdownBlockSpacing {
            consecutive_heading_top: 1.2,
            first_code_top: 0.7,
            ..Default::default()
        })
        .with_inline_code_box(0.12, px(4.))
        .with_task_checkbox(MarkdownTaskCheckbox {
            offset_em: 0.0,
            border: rgb(if dark { 0x5c5c5c } else { 0xbebebe }).into(),
            background: rgb(if dark { 0x1e1e1e } else { 0xffffff }).into(),
            checked_background: rgb(if dark { 0x846837 } else { 0xf8c98b }).into(),
            ..Default::default()
        })
        .with_blockquote(
            gpui_kit::StyleRefinement::default()
                .border_l(px(2.))
                .pl(px(14.))
                .pr(px(0.)),
        )
        .with_horizontal_rule_thickness(px(1.))
        .with_heading_strong_inherit(true)
        .with_deleted_muted(true)
        .with_inline_code_scale(0.88)
        .with_list_style(px(body * 1.4), px(body * 0.28), rgb(p.faint).into())
        .with_inline_code_padding(px(body * 0.88 * 0.3))
        .with_heading(move |level| {
            let size = body
                * match level {
                    1 => 1.35,
                    2 => 1.2,
                    3 => 1.08,
                    4 => 1.,
                    5 => 0.95,
                    _ => 0.9,
                };
            let mut style = gpui_kit::StyleRefinement::default()
                .text_size(px(size))
                .font_weight(gpui_kit::FontWeight(if level <= 4 { 650. } else { 600. }))
                .line_height(gpui_kit::relative(1.25));
            if level == 6 {
                style = style.text_color(rgb(p.muted));
            }
            style
        })
        .with_foreground(rgb(p.text).into())
        .with_muted_foreground(rgb(p.muted).into())
        .with_link(rgb(if dark { 0x59c2ff } else { 0x22a4e6 }).into())
        .with_code_background(rgb(if dark { 0x080b11 } else { 0xebeef0 }).into())
        .with_inline_code(gpui_kit::HighlightStyle::default())
        .with_border(rgb(p.border).into())
        .with_code_block(
            gpui_kit::StyleRefinement::default()
                .p(px(11.))
                .px(px(12.))
                .rounded(px(9.))
                .text_size(px(body * 0.88))
                // CSS pre computes 1.5 body em; its smaller code child inherits that height.
                .line_height(gpui_kit::relative(1.5 / 0.88))
                .border_1()
                .border_color(rgb(p.border)),
        )
        .with_table(
            table_style
                .text_size(px(body * 0.94))
                .line_height(gpui_kit::relative(1.62)),
        )
        .with_table_fit_content(true)
        .with_table_css_cells(true)
        .with_table_head(
            gpui_kit::StyleRefinement::default().font_weight(gpui_kit::FontWeight::SEMIBOLD),
        )
        .with_table_cell(gpui_kit::StyleRefinement::default().py(px(6.)).px(px(12.)))
}

type BlockKey = (usize, usize);

fn block_key(block: &CodeBlock) -> BlockKey {
    block
        .source_range()
        .map(|span| (span.start, span.end))
        .or_else(|| block.span.map(|span| (span.start, span.end)))
        .unwrap_or((0, 0))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CopyBlock {
    start: usize,
    end: usize,
    source_version: u64,
}

impl CopyBlock {
    fn parsed(block: &CodeBlock) -> Option<Self> {
        use std::hash::{Hash, Hasher};
        let span = block.source_range()?;
        let mut version = std::collections::hash_map::DefaultHasher::new();
        block.source().hash(&mut version);
        block.code().hash(&mut version);
        block.lang().hash(&mut version);
        block.closed_fence().hash(&mut version);
        Some(Self {
            start: span.start,
            end: span.end,
            source_version: version.finish(),
        })
    }
}

fn copied_code(code: &str) -> String {
    format!("{}\n", code.strip_suffix('\n').unwrap_or(code))
}

fn diagram_source(block: &CodeBlock) -> Option<String> {
    if !block.closed_fence()
        || !block
            .lang()?
            .split_whitespace()
            .next()?
            .eq_ignore_ascii_case("mermaid")
    {
        return None;
    }
    let source = copied_code(&block.code().replace("\r\n", "\n").replace('\r', "\n"));
    (source.encode_utf16().count() <= 8000).then_some(source)
}

struct DiagramEntry {
    key: BlockKey,
    source: String,
    entity: Entity<super::diagram_view::DiagramBlock>,
    _changed: Subscription,
}

/// Attach to a caller-owned TextViewState. No nested scroll surface or selection domain.
/// Keep this entity only as long as that text view is mounted/cached.
/// Initialize text with `TextViewState::markdown_with_extensions(source, extensions(), cx)`;
/// observe this controller for feedback, and use `on_layout` to remeasure the owning row.
/// Diagrams are pictures, not text hit targets: their logical copy leaf remains parsed code.
pub struct MarkdownPresentation {
    text: Entity<TextViewState>,
    dark: bool,
    blocks: Vec<CopyBlock>,
    copied: std::rc::Rc<std::cell::Cell<Option<usize>>>,
    revision: usize,
    source_revision: usize,
    timer: Option<Task<()>>,
    diagrams: std::collections::VecDeque<DiagramEntry>,
    pending: std::collections::HashSet<BlockKey>,
    on_layout: std::rc::Rc<dyn Fn(&mut App)>,
}

impl MarkdownPresentation {
    pub fn new(
        text: Entity<TextViewState>,
        dark: bool,
        on_layout: impl Fn(&mut App) + 'static,
        _: &mut Context<Self>,
    ) -> Self {
        Self {
            text,
            dark,
            blocks: Vec::new(),
            copied: Default::default(),
            revision: 0,
            source_revision: 0,
            timer: None,
            diagrams: Default::default(),
            pending: Default::default(),
            on_layout: std::rc::Rc::new(on_layout),
        }
    }

    pub fn set_theme(&mut self, dark: bool, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        for entry in &self.diagrams {
            entry
                .entity
                .update(cx, |diagram, cx| diagram.set_theme(dark, cx));
        }
        cx.notify();
    }

    /// Feedback is source-owned, but the text entity and its selection/undo are not replaced.
    pub fn source_changed(&mut self, cx: &mut Context<Self>) {
        self.copied.set(None);
        self.revision += 1;
        self.source_revision += 1;
        self.timer = None;
        self.blocks.clear();
        self.pending.clear();
        self.diagrams.clear();
        cx.notify();
    }

    pub fn copied_block(&self) -> Option<usize> {
        self.copied.get()
    }

    /// Observational resource state; does not start work or alter the document.
    pub fn render_pending(&self, cx: &App) -> bool {
        !self.pending.is_empty()
            || self
                .diagrams
                .iter()
                .any(|entry| entry.entity.read(cx).busy())
    }

    pub fn diagram_blocks(&self) -> Vec<Entity<super::diagram_view::DiagramBlock>> {
        self.diagrams
            .iter()
            .map(|entry| entry.entity.clone())
            .collect()
    }

    pub fn has_expanded_diagram(&self, cx: &App) -> bool {
        self.diagrams
            .iter()
            .any(|entry| entry.entity.read(cx).is_expanded())
    }

    pub fn rendered_code_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// Natural-height by default; callers may opt into TextView's own scrolling.
    pub fn decorate(
        entity: &Entity<Self>,
        body: f32,
        dark: bool,
        window: &Window,
        cx: &mut App,
    ) -> TextView {
        entity.update(cx, |this, cx| this.set_theme(dark, cx));
        let text = entity.read(cx).text.clone();
        let weak = entity.downgrade();
        let renderer_owner = weak.clone();
        let configured = extensions().code_block_body_renderer(move |block, _, cx| {
            let source = diagram_source(block)?;
            let key = block_key(block);
            let owner = renderer_owner.upgrade()?;
            owner.update(cx, |this, cx| {
                if let Some(entry) = this
                    .diagrams
                    .iter()
                    .find(|entry| entry.key == key && entry.source == source)
                {
                    return entry
                        .entity
                        .read(cx)
                        .ready()
                        .then(|| entry.entity.clone().into_any_element());
                }
                // The parser supplies identity/closure status. Prepare async resources after paint.
                // Admission is bounded, not an eviction/reprepare loop: natural-height
                // documents can project more than 32 blocks in a single frame.
                if this.diagrams.len() + this.pending.len() < 32 && this.pending.insert(key) {
                    let revision = this.source_revision;
                    let weak = cx.entity().downgrade();
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.source_revision != revision {
                                return;
                            }
                            this.pending.remove(&key);
                            let diagram = cx.new(|cx| {
                                super::diagram_view::DiagramBlock::new(
                                    source.clone(),
                                    this.dark,
                                    cx,
                                )
                            });
                            let changed = cx.subscribe(
                                &diagram,
                                |this, _, _: &super::diagram_view::DiagramChanged, cx| {
                                    this.text
                                        .update(cx, |text, cx| text.invalidate_inline_layout(cx));
                                    (this.on_layout)(cx);
                                    cx.notify();
                                },
                            );
                            this.diagrams.push_back(DiagramEntry {
                                key,
                                source,
                                entity: diagram,
                                _changed: changed,
                            });
                            this.text
                                .update(cx, |text, cx| text.invalidate_inline_layout(cx));
                            (this.on_layout)(cx);
                            cx.notify();
                        });
                    });
                }
                None
            })
        });
        TextView::new(&text)
            .on_link_click(|_, _, _, _| {})
            .text_size(px(body))
            .line_height(gpui_kit::relative(1.62))
            .style(text_style_at_size(dark, body, window))
            .markdown_extensions(configured)
            .shared_code_block_highlighter(syntax::code_block_highlighter(dark))
            .code_block_actions(move |block, window, cx| {
                let p = tokens(dark);
                // Synthetic blocks have no document identity; never alias them to offset zero.
                let Some(current) = CopyBlock::parsed(block) else {
                    return div();
                };
                let Some(owner) = weak.upgrade() else {
                    return div();
                };
                let (index, copied, source_revision) = owner.update(cx, |this, _| {
                    let index = this
                        .blocks
                        .iter()
                        .position(|candidate| candidate.start == current.start)
                        .unwrap_or_else(|| {
                            if this.blocks.len() >= 256 {
                                return 256usize.saturating_add(current.start);
                            }
                            this.blocks.push(current);
                            this.blocks.len() - 1
                        });
                    if let Some(previous) = this.blocks.get_mut(index)
                        && *previous != current
                    {
                        *previous = current;
                        if this.copied.get() == Some(index) {
                            this.copied.set(None);
                            this.revision += 1;
                            this.timer = None;
                        }
                    }
                    (
                        index,
                        this.copied.get() == Some(index),
                        this.source_revision,
                    )
                });
                let code = copied_code(block.code().as_ref());
                let focus = window
                    .use_keyed_state(
                        format!(
                            "markdown-copy-focus-{:?}-{}-{}",
                            owner.entity_id(),
                            source_revision,
                            current.start
                        ),
                        cx,
                        |_, cx| cx.focus_handle(),
                    )
                    .read(cx)
                    .clone();
                let keyboard_focus = focus.is_focused(window) && window.last_input_was_keyboard();
                let press_focus = focus.clone();
                let owner = owner.downgrade();
                div()
                    .relative()
                    .flex()
                    .items_start()
                    .child(
                        div()
                            .id(format!("language-{}", index + 1))
                            .test_support()
                            .mr(px(3.))
                            .mt(px(5.))
                            .font_family(support::mono())
                            .text_size(px(10.))
                            .line_height(gpui_kit::relative(1.))
                            .text_color(rgb(p.muted))
                            .opacity(0.)
                            .group_hover("markdown-code", |s| s.opacity(0.7))
                            .child(block.lang().unwrap_or_default()),
                    )
                    .child(
                        Button::new(format!("copy-{}", index + 1))
                            .accessibility_label("Copy Code")
                            .track_focus(&focus)
                            .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
                                press_focus.focus(window, cx);
                                cx.stop_propagation();
                            })
                            .cursor_default()
                            .group("markdown-copy")
                            .p(px(4.))
                            .rounded(px(4.))
                            // Zero opacity on the button skips GPUI's interaction paint.
                            .child(
                                div()
                                    .id(format!("copy-paint-{}", index + 1))
                                    .test_support()
                                    .size(px(13.))
                                    .opacity(if keyboard_focus { 1. } else { 0. })
                                    .group_hover("markdown-code", |s| s.opacity(1.))
                                    .child(
                                        support::icon(
                                            if copied { "check" } else { "copy" },
                                            13.,
                                            p.muted,
                                        )
                                        .opacity(if keyboard_focus || copied { 1. } else { 0.45 })
                                        .group_hover("markdown-copy", |s| s.opacity(1.)),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = owner.update(cx, |this, cx| {
                                    // A callback from an older projection cannot retarget feedback.
                                    if this.source_revision != source_revision
                                        || this
                                            .blocks
                                            .get(index)
                                            .is_some_and(|seen| *seen != current)
                                    {
                                        return;
                                    }
                                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                                    if this.blocks.get(index).is_none() {
                                        return;
                                    }
                                    this.revision += 1;
                                    let revision = this.revision;
                                    this.copied.set(Some(index));
                                    this.timer = Some(cx.spawn(async move |weak, cx| {
                                        cx.background_executor()
                                            .timer(Duration::from_millis(1200))
                                            .await;
                                        let _ = weak.update(cx, |this, cx| {
                                            if this.revision == revision {
                                                this.copied.set(None);
                                                cx.notify();
                                            }
                                        });
                                    }));
                                    cx.notify();
                                });
                            }),
                    )
            })
    }
}

pub struct MarkdownView {
    source: String,
    on_link: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    dark: bool,
    text: Entity<TextViewState>,
    presentation: Entity<MarkdownPresentation>,
    copy_feedback: std::rc::Rc<std::cell::Cell<Option<usize>>>,
    _presentation_changed: Subscription,
    natural_height: bool,
}

impl MarkdownView {
    pub fn set_theme(&mut self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        self.presentation
            .update(cx, |presentation, cx| presentation.set_theme(dark, cx));
        cx.notify();
    }
    pub fn new(
        source: impl Into<String>,
        dark: bool,
        on_link: impl Fn(&str) + Send + Sync + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = source.into();
        let text = cx.new(|cx| TextViewState::markdown_with_extensions(&source, extensions(), cx));
        let owner = cx.entity().downgrade();
        let presentation = cx.new(|cx| {
            MarkdownPresentation::new(
                text.clone(),
                dark,
                move |cx| {
                    let _ = owner.update(cx, |_, cx| cx.notify());
                },
                cx,
            )
        });
        let changed = cx.observe(&presentation, |_, _, cx| cx.notify());
        let copy_feedback = presentation.read(cx).copied.clone();
        Self {
            source,
            dark,
            text,
            presentation,
            copy_feedback,
            _presentation_changed: changed,
            natural_height: false,
            on_link: std::sync::Arc::new(on_link),
        }
    }

    pub fn set_source(&mut self, source: impl Into<String>, cx: &mut Context<Self>) {
        let source = source.into();
        if self.source == source {
            return;
        }
        if let Some(append) = source.strip_prefix(&self.source) {
            self.text.update(cx, |text, cx| text.push_str(append, cx));
        } else {
            self.text.update(cx, |text, cx| text.set_text(&source, cx));
        }
        self.source = source;
        self.presentation
            .update(cx, |presentation, cx| presentation.source_changed(cx));
        cx.notify();
    }

    pub fn natural_height(mut self) -> Self {
        self.natural_height = true;
        self
    }

    pub fn presentation(&self) -> &Entity<MarkdownPresentation> {
        &self.presentation
    }

    pub fn set_link_handler(
        &mut self,
        handler: impl Fn(&str) + Send + Sync + 'static,
        cx: &mut Context<Self>,
    ) {
        self.on_link = std::sync::Arc::new(handler);
        cx.notify();
    }

    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn text(&self) -> &Entity<TextViewState> {
        &self.text
    }
    pub fn copied_block(&self) -> Option<usize> {
        self.copy_feedback.get()
    }
}

impl Render for MarkdownView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = tokens(self.dark);
        let handler = self.on_link.clone();
        let view = MarkdownPresentation::decorate(&self.presentation, 13., self.dark, window, cx)
            .scrollable(!self.natural_height)
            .when(!self.natural_height, |view| view.size_full())
            .on_link_click(move |url, _, _, _| handler(url.as_ref()));
        div()
            .w_full()
            .when(!self.natural_height, |root| root.h_full())
            .text_color(rgb(p.text))
            .font_family(native_font::family())
            .text_size(px(13.))
            .line_height(gpui_kit::relative(1.62))
            .child(view)
    }
}

#[cfg(test)]
#[path = "markdown_parity_tests.rs"]
mod parity_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        InputEvent, MouseButton, ScrollDelta, ScrollWheelEvent, TestApp, TestAppContext,
        TestSupportExt, gpui, point, test::TestWindowExt,
    };

    struct CopyHost {
        markdown: Entity<MarkdownView>,
        editor: Entity<gpui_kit::base::input::InputState>,
    }
    impl CopyHost {
        fn new(markdown: Entity<MarkdownView>, w: &mut Window, cx: &mut Context<Self>) -> Self {
            Self {
                markdown,
                editor: cx.new(|cx| gpui_kit::base::input::InputState::new(w, cx)),
            }
        }
    }
    impl Render for CopyHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .p(px(40.))
                .flex_col()
                .child(
                    div()
                        .h(px(32.))
                        .child(gpui_kit::base::input::Input::new(&self.editor)),
                )
                .child(self.markdown.clone())
        }
    }

    #[test]
    fn root_keyboard_copy_from_real_input_reveals_ink_and_activates_with_tab_and_option_tab() {
        for tab in ["tab"]
            .into_iter()
            .chain(cfg!(target_os = "macos").then_some("alt-tab"))
        {
            for key in ["enter", "space"] {
                let mut app = TestApp::new();
                app.update(|cx| crate::components::init(cx, false));
                let mut view = None;
                let mut window = app.open_window(|w, cx| {
                    let markdown = cx.new(|cx| {
                        MarkdownView::new("plain text\n\n```rs\nlet a = 1;\n```", false, |_| {}, cx)
                    });
                    view = Some(markdown.clone());
                    gpui_kit::base::Root::new(cx.new(|cx| CopyHost::new(markdown, w, cx)), w, cx)
                });
                let view = view.unwrap();
                window.simulate_resize(gpui_kit::size(px(900.), px(700.)));
                window.update(|_, w, cx| w.bounds_changed(cx));
                window.draw();
                window.update(|_, w, _| {
                    assert!(!w.find("copy-paint-1").visible());
                    assert_eq!(
                        w.find("copy-1").bounds().size,
                        gpui_kit::size(px(21.), px(21.))
                    );
                });
                window.simulate_click(point(px(60.), px(50.)), MouseButton::Left);
                window.draw();
                for _ in 0..2 {
                    let handle = app.windows()[0];
                    app.update(|cx| {
                        cx.update_window(handle, |_, w, cx| w.press(tab, cx))
                            .unwrap()
                    });
                    app.run_until_parked();
                    window.draw();
                    if window.update(|_, w, _| w.find("copy-1").focused() == Some(true)) {
                        break;
                    }
                }
                window.update(|_, w, _| {
                    assert_eq!(w.find("copy-1").focused(), Some(true), "{tab}");
                    assert!(w.find("copy-paint-1").visible(), "{tab}");
                    assert_eq!(
                        w.find("copy-paint-1").bounds().size,
                        gpui_kit::size(px(13.), px(13.))
                    );
                });
                let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
                window.simulate_event(gpui_kit::KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                });
                window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
                window.update(|_, w, cx| {
                    assert_eq!(view.read(cx).copied_block(), Some(0), "{tab} -> {key}");
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        "let a = 1;\n"
                    );
                    assert_eq!(w.find("copy-1").focused(), Some(true));
                });
            }
        }
    }

    #[test]
    fn initial_root_copy_accepts_move_down_up_without_an_intervening_paint() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut view = None;
        let mut window = app.open_window(|w, cx| {
            let markdown =
                cx.new(|cx| MarkdownView::new("```rs\nlet a = 1;\n```", false, |_| {}, cx));
            view = Some(markdown.clone());
            gpui_kit::base::Root::new(cx.new(|cx| CopyHost::new(markdown, w, cx)), w, cx)
        });
        let view = view.unwrap();
        window.simulate_resize(gpui_kit::size(px(900.), px(700.)));
        window.update(|_, w, cx| w.bounds_changed(cx));
        window.draw();
        let bounds = window.update(|_, w, _| {
            assert!(!w.find("copy-paint-1").visible());
            w.find("copy-1").bounds()
        });
        window.simulate_mouse_move(bounds.center());
        window.simulate_mouse_down(bounds.center(), MouseButton::Left);
        window.simulate_mouse_up(bounds.center(), MouseButton::Left);
        window.update(|_, _, cx| assert_eq!(view.read(cx).copied_block(), Some(0)));
        window.draw();
        window.update(|_, w, _| {
            assert!(w.find("copy-paint-1").visible());
            assert_eq!(w.find("copy-1").focused(), Some(true));
            assert_eq!(w.find("copy-1").bounds(), bounds);
        });
    }

    #[test]
    fn theme_round_trip_repaints_without_resetting_text_selection_or_copy_timer() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let source = format!(
            "```rs\nlet answer = 42;\n```\n\n{}",
            "paragraph\n\n".repeat(100)
        );
        let mut window = app.open_window(|_, cx| MarkdownView::new(source, false, |_| {}, cx));
        window.draw();
        let copy = window.update(|_, window, _| window.find("copy-1").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        window.update(|view, _, cx| view.text.update(cx, |text, cx| text.select_all(cx)));
        let before = window.read(|view, cx| {
            (
                view.text.clone(),
                view.source.clone(),
                view.text.read(cx).selected_text(),
                view.presentation.read(cx).revision,
            )
        });
        let handler = window.read(|view, _| view.on_link.clone());
        app.advance_clock(Duration::from_millis(600));
        app.run_until_parked();
        for dark in [true, false] {
            let scroll =
                window.read(|view, cx| view.text.read(cx).list_state().logical_scroll_top());
            let focus = window.update(|_, window, cx| window.focused(cx));
            window.update(|view, window, cx| view.set_theme(dark, window, cx));
            window.draw();
            window.read(|view, cx| {
                assert_eq!(view.text, before.0);
                assert_eq!(view.source, before.1);
                assert_eq!(view.text.read(cx).selected_text(), before.2);
                assert_eq!(view.presentation.read(cx).revision, before.3);
                let after = view.text.read(cx).list_state().logical_scroll_top();
                assert_eq!(after.item_ix, scroll.item_ix);
                assert_eq!(after.offset_in_item, scroll.offset_in_item);
                assert_eq!(view.copied_block(), Some(0));
                assert!(view.presentation.read(cx).timer.is_some());
                assert!(std::sync::Arc::ptr_eq(&view.on_link, &handler));
            });
            window.update(|_, window, cx| {
                assert_eq!(window.focused(cx), focus);
                assert!(window.painted_quads().iter().any(|quad| {
                    quad.background == rgb(if dark { 0x080b11 } else { 0xebeef0 }).into()
                }));
            });
        }
        app.advance_clock(Duration::from_millis(601));
        app.run_until_parked();
        assert_eq!(window.read(|view, _| view.copied_block()), None);
    }

    #[test]
    fn copy_feedback_expires_and_source_replacement_cancels_old_timer() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut window =
            app.open_window(|_, cx| MarkdownView::new("```rs\nlet a = 1;\n```", false, |_| {}, cx));
        window.draw();
        let copy = window.update(|_, window, _| window.find("copy-1").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        assert_eq!(window.read(|view, _| view.copied_block()), Some(0));
        app.advance_clock(Duration::from_millis(600));
        app.run_until_parked();
        window.update(|view, _, cx| view.set_source("```rs\nlet b = 2;\n```", cx));
        window.draw();
        let copy = window.update(|_, window, _| window.find("copy-1").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        app.advance_clock(Duration::from_millis(600));
        app.run_until_parked();
        assert_eq!(window.read(|view, _| view.copied_block()), Some(0));
        app.advance_clock(Duration::from_millis(601));
        app.run_until_parked();
        assert_eq!(window.read(|view, _| view.copied_block()), None);
    }

    #[test]
    fn streaming_copy_keeps_first_identity_and_invalidates_only_its_changed_source() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut window =
            app.open_window(|_, cx| MarkdownView::new("```txt\none", false, |_| {}, cx));
        window.draw();
        let mut original_focus = None;
        for (append, expected) in [
            ("", "one\n"),
            ("\ntwo", "one\ntwo\n"),
            ("\n```", "one\ntwo\n"),
        ] {
            if !append.is_empty() {
                // Conversation streaming intentionally does not call source_changed.
                window.update(|view, _, cx| {
                    view.text.update(cx, |text, cx| text.push_str(append, cx))
                });
                window.draw();
                assert_eq!(window.read(|view, _| view.copied_block()), None);
            }
            window
                .read(|view, cx| assert_eq!(view.presentation.read(cx).rendered_code_blocks(), 1));
            let copy = window.update(|_, w, _| w.find("copy-1").bounds());
            window.simulate_click(copy.center(), MouseButton::Left);
            assert_eq!(window.read(|view, _| view.copied_block()), Some(0));
            let focus = window.update(|_, window, cx| window.focused(cx));
            if let Some(original) = &original_focus {
                assert_eq!(&focus, original);
            } else {
                original_focus = Some(focus);
            }
            window.update(|_, _, cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected)
            });
        }
        app.advance_clock(Duration::from_millis(1201));
        app.run_until_parked();
        assert_eq!(window.read(|view, _| view.copied_block()), None);
    }

    #[test]
    fn streaming_multiple_fence_kinds_keep_copy_ids_payloads_and_other_feedback() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut window =
            app.open_window(|_, cx| MarkdownView::new("````txt\none\n```\n", false, |_| {}, cx));
        window.draw();
        let copy = window.update(|_, w, _| w.find("copy-1").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        window.update(|_, _, cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "one\n```\n"
            )
        });
        window.update(|view, _, cx| {
            view.text.update(cx, |text, cx| {
                text.push_str("two\n````\n\n~~~txt\nthree", cx)
            })
        });
        window.draw();
        assert_eq!(window.read(|view, _| view.copied_block()), None);
        assert_eq!(
            window.read(|view, cx| view.presentation.read(cx).rendered_code_blocks()),
            2
        );
        let copy = window.update(|_, w, _| w.find("copy-1").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        window.update(|_, _, cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "one\n```\ntwo\n"
            )
        });

        // Growing another block must not cancel the first block's existing feedback/timer.
        let revision = window.read(|view, cx| view.presentation.read(cx).revision);
        window.update(|view, _, cx| view.text.update(cx, |text, cx| text.push_str("\nfour", cx)));
        window.draw();
        window.read(|view, cx| {
            assert_eq!(view.copied_block(), Some(0));
            assert_eq!(view.presentation.read(cx).revision, revision);
            assert!(view.presentation.read(cx).timer.is_some());
            assert_eq!(view.presentation.read(cx).rendered_code_blocks(), 2);
        });
        let copy = window.update(|_, w, _| w.find("copy-2").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        assert_eq!(window.read(|view, _| view.copied_block()), Some(1));
        window.update(|_, _, cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "three\nfour\n"
            )
        });
        window.update(|view, _, cx| {
            view.text.update(cx, |text, cx| {
                text.push_str("\n~~~\n\n```txt\nfive\n\n```", cx)
            })
        });
        window.draw();
        assert_eq!(window.read(|view, _| view.copied_block()), None);
        assert_eq!(
            window.read(|view, cx| view.presentation.read(cx).rendered_code_blocks()),
            3
        );
        for (id, expected, index) in [
            ("copy-1", "one\n```\ntwo\n", 0),
            ("copy-2", "three\nfour\n", 1),
            ("copy-3", "five\n", 2),
        ] {
            let copy = window.update(|_, w, _| w.find(id).bounds());
            window.simulate_click(copy.center(), MouseButton::Left);
            assert_eq!(window.read(|view, _| view.copied_block()), Some(index));
            window.update(|_, _, cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected)
            });
        }
    }

    #[test]
    fn synthetic_code_has_no_shared_copy_identity() {
        assert!(CopyBlock::parsed(&CodeBlock::from_code("one", Some("txt"))).is_none());
        assert!(CopyBlock::parsed(&CodeBlock::from_code("two", Some("txt"))).is_none());
    }

    #[test]
    fn rendered_link_click_uses_latest_caller_handler() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let old = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let old_handler = old.clone();
        let mut window = app.open_window(|_, cx| {
            MarkdownView::new(
                "[link](https://example.org)",
                false,
                move |url| {
                    old_handler.lock().unwrap().push(url.to_string());
                },
                cx,
            )
        });
        window.draw();
        window.simulate_click(gpui_kit::point(px(10.), px(10.)), MouseButton::Left);
        assert_eq!(old.lock().unwrap().as_slice(), &["https://example.org"]);

        let latest = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = latest.clone();
        window.update(|view, _, cx| {
            view.set_link_handler(
                move |url| received.lock().unwrap().push(url.to_string()),
                cx,
            )
        });
        window.draw();
        window.simulate_click(gpui_kit::point(px(10.), px(10.)), MouseButton::Left);
        assert_eq!(old.lock().unwrap().len(), 1);
        assert_eq!(latest.lock().unwrap().as_slice(), &["https://example.org"]);
    }

    struct BoundedMarkdownHost {
        view: Entity<MarkdownView>,
    }

    impl Render for BoundedMarkdownHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("markdown-host")
                .test_support()
                .w(px(800.))
                .h(px(300.))
                .overflow_hidden()
                .child(self.view.clone())
        }
    }

    #[gpui::test]
    fn bounded_code_layout_and_copy_stay_pinned_after_wheel(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let source = format!(
            "| Name | State |\n| --- | --- |\n| Alpha | Ready |\n\n```txt\n{}\n```",
            "long-code-line-".repeat(120)
        );
        let (_, window) = cx.add_window_view(|_, cx| BoundedMarkdownHost {
            view: cx.new(|cx| MarkdownView::new(source, false, |_| {}, cx)),
        });
        let (host, code, copy) = window.update(|window, cx| {
            let _ = window.draw(cx);
            (
                window.find("markdown-host").bounds(),
                window.find("code-1").bounds(),
                window.find("copy-1").bounds(),
            )
        });
        assert_eq!(host.size.width, px(800.));
        assert!(
            code.size.width <= host.size.width,
            "code {code:?}, host {host:?}"
        );
        assert!(code.left() >= host.left() && code.right() <= host.right());
        assert!(
            code.size.height < px(100.),
            "long line must not wrap: {code:?}"
        );
        assert!(
            copy.left() >= code.left() && copy.right() <= code.right(),
            "copy {copy:?}, code {code:?}"
        );
        assert!(copy.top() >= code.top() && copy.bottom() <= code.bottom());
        window.update(|window, cx| {
            window.dispatch_event(
                ScrollWheelEvent {
                    position: point(code.left() + px(40.), code.top() + px(20.)),
                    delta: ScrollDelta::Pixels(point(px(-250.), px(0.))),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            let _ = window.draw(cx);
            assert_eq!(window.find("code-1").bounds(), code);
            assert_eq!(window.find("copy-1").bounds(), copy);
        });
    }

    #[gpui::test]
    fn wide_code_stays_single_row_and_table_renders(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let source = format!(
            "| Name | State |\n| --- | --- |\n| Alpha | Ready |\n\n```txt\n{}\n```",
            "long-code-line-".repeat(120)
        );
        let (view, window) =
            cx.add_window_view(|_, cx| MarkdownView::new(source, false, |_| {}, cx));
        window.update(|window, cx| {
            let _ = window.draw(cx);
            let rendered = view.read(cx).text.read(cx).rendered_text();
            assert!(rendered.as_str().contains("Alpha"));
            assert!(rendered.as_str().contains("Ready"));
        });
        let copy = window.update(|window, _| window.find("copy-1").bounds());
        assert!(
            copy.top() < px(150.),
            "long code must remain a single scrollable row: {copy:?}"
        );
    }

    #[gpui::test]
    fn source_update_reparses_text_and_invalidates_copy(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let view = cx.update(|cx| {
            cx.new(|cx| MarkdownView::new("# First\n\n```rs\nlet a = 1;\n```", false, |_| {}, cx))
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.text.update(cx, |text, cx| text.select_all(cx));
                view.presentation.update(cx, |p, _| p.copied.set(Some(0)));
                let revision = view.presentation.read(cx).revision;
                view.set_source("# First\n\n```rs\nlet a = 1;\n```", cx);
                assert_eq!(view.presentation.read(cx).revision, revision);
                assert_eq!(view.copied_block(), Some(0));
                assert!(!view.text.read(cx).selected_text().is_empty());
                view.set_source("# Second\n\nA [link](https://example.org)", cx);
            })
        });
        cx.read(|cx| {
            let view = view.read(cx);
            assert_eq!(view.source(), "# Second\n\nA [link](https://example.org)");
            assert_eq!(view.copied_block(), None);
            assert!(
                view.text()
                    .read(cx)
                    .rendered_text()
                    .as_str()
                    .contains("Second")
            );
        });
    }
}

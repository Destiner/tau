//! Searchable command palette; pages and destinations are supplied by the host.
use super::support;
use gpui_kit::{
    BoxShadow, Context, Entity, EntityInputHandler, IntoElement, KeyDownEvent, MouseButton, Render,
    ScrollHandle, TestSupportExt, Window,
    base::{
        Button, Input,
        input::{InputEvent, InputState},
    },
    div,
    prelude::*,
    px, rgb, rgba,
};

gpui_kit::actions!(native_palette, [NextRow, PreviousRow, KeepFocus]);

#[derive(Clone)]
pub struct PaletteItem {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub shortcut: String,
    pub group: Option<String>,
    pub destination: Option<String>,
}
#[derive(Clone)]
pub struct PalettePage {
    pub id: String,
    pub placeholder: String,
    pub items: Vec<PaletteItem>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteEvent {
    Selected(String),
    Back,
    Dismissed,
}

pub struct Palette {
    editor_style: gpui_kit::base::input::InputEditorStyle,
    dark: bool,
    open: bool,
    pages: Vec<PalettePage>,
    page: usize,
    selected: Option<usize>,
    query: String,
    root_search: Option<(String, Option<String>)>,
    scroll: ScrollHandle,
    search: Entity<InputState>,
}
impl gpui_kit::EventEmitter<PaletteEvent> for Palette {}
impl Palette {
    pub fn set_theme(&mut self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        // Palette placeholders use muted rather than faint ink.
        let muted = rgb(support::tokens(self.dark).muted).into();
        if self.editor_style.muted_foreground == muted {
            self.editor_style.muted_foreground = rgb(support::tokens(dark).muted).into();
        }
        super::composer::retheme_input_style(&mut self.editor_style, self.dark, dark);
        self.dark = dark;
        self.search.update(cx, |editor, cx| {
            editor.set_editor_style(self.editor_style.clone());
            cx.notify();
        });
        cx.notify();
    }

    pub fn set_editor_style(
        &mut self,
        style: gpui_kit::base::input::InputEditorStyle,
        cx: &mut Context<Self>,
    ) {
        self.editor_style = style.clone();
        self.search.update(cx, |editor, cx| {
            editor.set_editor_style(style);
            cx.notify();
        });
    }
    pub fn new(
        dark: bool,
        pages: Vec<PalettePage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let pages = Self::with_root(pages);
        cx.bind_keys([
            gpui_kit::KeyBinding::new("down", NextRow, Some("NativePaletteSearch > Input")),
            gpui_kit::KeyBinding::new("up", PreviousRow, Some("NativePaletteSearch > Input")),
            gpui_kit::KeyBinding::new("tab", KeepFocus, Some("NativePaletteSearch > Input")),
            gpui_kit::KeyBinding::new("shift-tab", KeepFocus, Some("NativePaletteSearch > Input")),
        ]);
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .accessibility_label("Search")
                .placeholder(pages[0].placeholder.clone())
        });
        search.update(cx, |s, _| {
            s.set_protect_ime_keys(true);
            s.set_editor_paddings(gpui_kit::Edges::all(px(1.)));
            s.set_editor_style(gpui_kit::base::input::InputEditorStyle {
                muted_foreground: rgb(support::tokens(dark).muted).into(),
                ..support::input_style(dark)
            });
        });
        cx.subscribe_in(
            &search,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && this.open
                    && let Some(i) = this.selected
                {
                    this.activate(i, window, cx);
                }
                if matches!(event, InputEvent::Change) {
                    this.query = input.read(cx).value().to_string();
                    if !this.selected.is_some_and(|i| this.visible().contains(&i)) {
                        this.selected = this.visible().first().copied();
                    }
                    cx.notify();
                }
            },
        )
        .detach();
        let selected = (!pages[0].items.is_empty()).then_some(0);
        Self {
            editor_style: gpui_kit::base::input::InputEditorStyle {
                muted_foreground: rgb(support::tokens(dark).muted).into(),
                ..support::input_style(dark)
            },
            dark,
            open: false,
            pages,
            page: 0,
            selected,
            query: String::new(),
            root_search: None,
            scroll: ScrollHandle::new(),
            search,
        }
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn page_id(&self) -> &str {
        &self.pages[self.page].id
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn set_query(
        &mut self,
        query: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.query = query.into();
        let value = self.query.clone();
        self.search
            .update(cx, |search, cx| search.set_value(value, window, cx));
        self.selected = self.visible().first().copied();
        cx.notify();
    }
    pub fn selected_id(&self) -> Option<&str> {
        self.selected.map(|i| self.rows()[i].id.as_str())
    }
    pub fn search_editor(&self) -> Entity<InputState> {
        self.search.clone()
    }
    pub fn set_pages(
        &mut self,
        pages: Vec<PalettePage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pages = Self::with_root(pages);
        self.page = 0;
        self.reset_search(window, cx);
    }
    /// Refresh host data without resetting text, focus, page, or row identity.
    pub fn refresh_pages(
        &mut self,
        pages: Vec<PalettePage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = self.page_id().to_owned();
        let selected = self.selected_id().map(str::to_owned);
        self.pages = Self::with_root(pages);
        self.page = self.pages.iter().position(|p| p.id == page).unwrap_or(0);
        self.selected = selected.and_then(|id| self.rows().iter().position(|r| r.id == id));
        let placeholder = self.pages[self.page].placeholder.clone();
        self.search
            .update(cx, |s, cx| s.set_placeholder(placeholder, window, cx));
        cx.notify();
    }
    fn with_root(mut pages: Vec<PalettePage>) -> Vec<PalettePage> {
        if pages.is_empty() {
            pages.push(PalettePage {
                id: "root".into(),
                placeholder: String::new(),
                items: vec![],
            });
        }
        pages
    }
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_page(&self.pages[0].id.clone(), window, cx);
    }
    /// Open a host-supplied page without changing which page is the root.
    pub fn open_page(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.root_search = None;
        self.page = self
            .pages
            .iter()
            .position(|page| page.id == id)
            .unwrap_or(0);
        self.reset_search(window, cx);
    }
    /// Close for host dispatch without scheduling a second focus restoration.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.close(cx);
        cx.emit(PaletteEvent::Dismissed);
    }
    fn rows(&self) -> &[PaletteItem] {
        &self.pages[self.page].items
    }
    fn visible(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        self.rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                q.is_empty() || row.label.to_lowercase().contains(&q) || row.id.contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }
    fn reset_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        let placeholder = self.pages[self.page].placeholder.clone();
        self.search.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder(placeholder, window, cx);
            s.focus(window, cx);
        });
        self.selected = self.visible().first().copied();
        cx.notify();
    }
    fn activate(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || !self.visible().contains(&i) {
            return;
        }
        if let Some(next) = self.rows()[i]
            .destination
            .as_ref()
            .and_then(|id| self.pages.iter().position(|page| &page.id == id))
        {
            if self.page == 0 {
                self.root_search =
                    Some((self.query.clone(), self.selected_id().map(str::to_owned)));
            }
            self.page = next;
            self.reset_search(window, cx);
        } else {
            let event = PaletteEvent::Selected(self.rows()[i].id.clone());
            self.open = false;
            cx.emit(event);
            cx.notify();
        }
    }
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != 0 {
            self.page = 0;
            cx.emit(PaletteEvent::Back);
            self.reset_search(window, cx);
            if let Some((query, selected)) = self.root_search.take() {
                self.set_query(query, window, cx);
                self.selected = selected.and_then(|id| self.rows().iter().position(|r| r.id == id));
            }
        }
    }
    fn move_row(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let composing = self
            .search
            .update(cx, |s, cx| s.marked_text_range(window, cx).is_some());
        if !self.open || composing {
            return;
        }
        let visible = self.visible();
        if visible.is_empty() {
            return;
        }
        let current = self
            .selected
            .and_then(|i| visible.iter().position(|&n| n == i))
            .unwrap_or(0);
        self.selected = Some(
            visible[if down {
                (current + 1) % visible.len()
            } else {
                (current + visible.len() - 1) % visible.len()
            }],
        );
        if let Some(index) = self
            .selected
            .and_then(|i| visible.iter().position(|n| *n == i))
        {
            self.scroll.scroll_to_item(index);
        }
        cx.notify();
    }
    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open
            || self
                .search
                .update(cx, |s, cx| s.marked_text_range(window, cx).is_some())
        {
            return;
        }
        let modifiers = e.keystroke.modifiers;
        match e.keystroke.key.as_str() {
            "escape" => self.dismiss(cx),
            "[" if self.page != 0 && (modifiers.platform || modifiers.control) => {
                self.back(window, cx)
            }
            "backspace" | "delete"
                if self.page != 0
                    && self.query.is_empty()
                    && !modifiers.platform
                    && !modifiers.control
                    && !modifiers.alt
                    && !modifiers.shift =>
            {
                self.back(window, cx)
            }
            _ => return,
        }
        cx.stop_propagation();
    }
    fn dialog(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows_height = (f32::from(window.viewport_size().height) * 0.85 - 61.).clamp(55., 480.);
        let t = support::tokens(self.dark);
        let visible = self.visible();
        div()
            .id("palette-surface")
            .test_support()
            .w(px(490.))
            .max_w_full()
            .max_h(px(520.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(12.),
                    rgba(if self.dark { 0x00000099 } else { 0x00000033 }).into(),
                )
                .blur_radius(px(36.)),
            ])
            .overflow_hidden()
            .occlude()
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|s, _: &NextRow, w, cx| s.move_row(true, w, cx)))
            .on_action(cx.listener(|s, _: &PreviousRow, w, cx| s.move_row(false, w, cx)))
            .on_action(cx.listener(|s, _: &KeepFocus, w, cx| {
                s.search.update(cx, |input, cx| input.focus(w, cx));
            }))
            .child(
                div()
                    .h(px(35.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .child(if self.page != 0 {
                        Button::new("back")
                            .w(px(12.))
                            .h(px(12.))
                            .p_0()
                            .focusable(true)
                            .child(support::icon("chevron-left", 12., t.muted))
                            .on_click(cx.listener(|s, _, w, cx| s.back(w, cx)))
                            .into_any_element()
                    } else {
                        div()
                            .w(px(12.))
                            .child(support::icon("search", 12., t.muted))
                            .into_any_element()
                    })
                    .child(
                        div()
                            .id("search")
                            .test_support()
                            .flex_1()
                            .key_context("NativePaletteSearch")
                            .line_height(px(16.))
                            .child(Input::new(&self.search)),
                    ),
            )
            .child(
                div()
                    .p(px(6.))
                    .id("palette-results")
                    .max_h(px(rows_height))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .children(visible.iter().copied().map(|i| {
                        let row = &self.rows()[i];
                        let previous = visible
                            .iter()
                            .position(|&n| n == i)
                            .and_then(|position| position.checked_sub(1))
                            .map(|position| &self.rows()[visible[position]]);
                        let heading = row.group.as_ref().filter(|group| {
                            previous.and_then(|prior| prior.group.as_ref()) != Some(*group)
                        });
                        div()
                            .when_some(heading.cloned(), |d, group| {
                                d.child(
                                    div()
                                        .h(px(27.))
                                        .px(px(10.))
                                        .pt(px(9.))
                                        .pb(px(6.))
                                        .line_height(px(12.))
                                        .text_color(rgb(t.muted))
                                        .text_size(px(10.))
                                        .child(group),
                                )
                            })
                            .child(
                                Button::new(format!("row-{i}"))
                                    .w_full()
                                    .min_h(px(28.))
                                    .px(px(10.))
                                    .rounded(px(4.))
                                    .bg(if self.selected == Some(i) {
                                        rgba(if self.dark { 0x4752664d } else { 0x6b7d8f33 })
                                    } else {
                                        rgba(0x00000000)
                                    })
                                    .text_color(rgb(t.text))
                                    .text_size(px(12.))
                                    .focusable(false)
                                    .tab_stop(false)
                                    .on_mouse_move(cx.listener(move |s, _, _, cx| {
                                        if s.selected != Some(i) {
                                            s.selected = Some(i);
                                            cx.notify();
                                        }
                                    }))
                                    .on_click(cx.listener(move |s, _, w, cx| s.activate(i, w, cx)))
                                    .child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_baseline()
                                                    .gap(px(8.))
                                                    .child(row.label.clone())
                                                    .when(!row.detail.is_empty(), |d| {
                                                        d.child(
                                                            div()
                                                                .text_size(px(11.))
                                                                .text_color(rgb(t.muted))
                                                                .child(row.detail.clone()),
                                                        )
                                                    }),
                                            )
                                            .when(!row.shortcut.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .flex()
                                                        .gap(px(2.))
                                                        .font_family(support::mono())
                                                        .text_size(px(11.))
                                                        .text_color(rgb(t.muted))
                                                        .children(row.shortcut.chars().map(
                                                            |key| div().child(key.to_string()),
                                                        )),
                                                )
                                            }),
                                    ),
                            )
                    }))
                    .when(visible.is_empty(), |d| {
                        d.child(
                            div()
                                .h(px(90.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .line_height(px(16.))
                                .text_color(rgb(t.muted))
                                .child(if self.query.is_empty() {
                                    "No commands available"
                                } else {
                                    "No matches"
                                }),
                        )
                    }),
            )
    }
}
impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let viewport = window.viewport_size();
        div().when(self.open, |d| {
            d.absolute()
                .top_0()
                .left_0()
                .w(viewport.width)
                .h(viewport.height)
                .occlude()
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .bg(rgba(t.scrim))
                        .on_mouse_down(MouseButton::Left, cx.listener(|s, _, _, cx| s.dismiss(cx))),
                )
                .child(
                    div()
                        .absolute()
                        .top(viewport.height * 0.15)
                        .left_0()
                        .right_0()
                        .px(px(14.))
                        .flex()
                        .justify_center()
                        .child(self.dialog(window, cx)),
                )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    #[test]
    fn page_selection_and_back_are_typed() {
        let mut pages = vec![
            PalettePage {
                id: "root".into(),
                placeholder: "Search".into(),
                items: vec![PaletteItem {
                    id: "navigate".into(),
                    label: "Navigate".into(),
                    detail: "".into(),
                    shortcut: "".into(),
                    group: None,
                    destination: Some("next".into()),
                }],
            },
            PalettePage {
                id: "next".into(),
                placeholder: "Find".into(),
                items: vec![],
            },
        ];
        pages[1].items.push(PaletteItem {
            id: "apply".into(),
            label: "Apply".into(),
            detail: String::new(),
            shortcut: String::new(),
            group: None,
            destination: None,
        });
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|window, cx| Palette::new(false, pages, window, cx));
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &PaletteEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        window.update(|palette, window, cx| {
            palette.open(window, cx);
            assert_eq!(palette.selected_id(), Some("navigate"));
            palette.set_query("missing", window, cx);
            assert_eq!(palette.selected_id(), None);
            palette.set_query("navigate", window, cx);
            assert_eq!(palette.selected_id(), Some("navigate"));
            palette.activate(0, window, cx);
            assert_eq!(palette.page_id(), "next");
            assert_eq!(palette.selected_id(), Some("apply"));
            palette.back(window, cx);
            palette.activate(0, window, cx);
            palette.activate(0, window, cx);
            assert!(!palette.is_open());
        });
        assert_eq!(
            *received.borrow(),
            vec![PaletteEvent::Back, PaletteEvent::Selected("apply".into())]
        );
    }
    #[test]
    fn surface_tracks_viewport_and_backdrop_dismisses() {
        use gpui_kit::{point, size, test::TestWindowExt};
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|window, cx| {
            Palette::new(
                false,
                vec![PalettePage {
                    id: "root".into(),
                    placeholder: "Search".into(),
                    items: vec![],
                }],
                window,
                cx,
            )
        });
        window.update(|palette, window, cx| palette.open(window, cx));
        for (width, height) in [(900., 700.), (560., 480.)] {
            window.simulate_resize(size(px(width), px(height)));
            window.update(|_, window, cx| window.bounds_changed(cx));
            window.draw();
            let bounds = window.update(|_, window, _| window.find("palette-surface").bounds());
            assert!(
                (f32::from(bounds.left()) - (width - 490.) / 2.).abs() < 1.,
                "{width}x{height}: {bounds:?}"
            );
            assert!((f32::from(bounds.top()) - height * 0.15).abs() < 1.);
        }
        window.simulate_click(point(px(5.), px(5.)), MouseButton::Left);
        assert!(!window.read(|palette, _| palette.is_open()));
    }
    #[test]
    fn empty_pages_replace_stale_rows_and_can_be_replaced_again() {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|window, cx| Palette::new(false, vec![], window, cx));
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &PaletteEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        window.update(|palette, window, cx| {
            palette.open(window, cx);
            assert_eq!(palette.page_id(), "root");
            assert_eq!(palette.selected_id(), None);
            palette.set_pages(
                vec![PalettePage {
                    id: "replacement".into(),
                    placeholder: "Find".into(),
                    items: vec![PaletteItem {
                        id: "new".into(),
                        label: "New".into(),
                        detail: String::new(),
                        shortcut: String::new(),
                        group: None,
                        destination: None,
                    }],
                }],
                window,
                cx,
            );
            assert_eq!(palette.selected_id(), Some("new"));
            palette.set_query("new", window, cx);
            palette.set_pages(vec![], window, cx);
            assert_eq!(palette.page_id(), "root");
            assert_eq!(palette.query(), "");
            assert_eq!(palette.selected_id(), None);
            palette.open(window, cx);
            assert_eq!(palette.selected_id(), None);
            palette.activate(0, window, cx);
        });
        assert!(received.borrow().is_empty());
    }
    #[test]
    fn empty_search_has_no_selection() {
        let pages = vec![PalettePage {
            id: "root".into(),
            placeholder: "Search".into(),
            items: vec![],
        }];
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|window, cx| Palette::new(false, pages, window, cx));
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &PaletteEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        window.update(|palette, window, cx| {
            palette.open(window, cx);
            assert_eq!(palette.selected_id(), None);
            palette.move_row(true, window, cx);
            assert_eq!(palette.selected_id(), None);
            palette.dismiss(cx);
        });
        assert_eq!(*received.borrow(), vec![PaletteEvent::Dismissed]);
    }
}

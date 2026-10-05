//! Native anchored and context menus.
use super::support;
use gpui_kit::{
    Anchor, Bounds, Context, FocusHandle, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    Pixels, Render, Window,
    base::{Button, ElementExt, Popup, TestSupportExt},
    div,
    prelude::*,
    px, rgb,
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub id: String,
    pub element_id: String,
    pub label: String,
    pub shortcut: Option<String>,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEvent {
    Selected(String),
    Dismissed,
}

pub struct Menu {
    dark: bool,
    context: bool,
    trigger_label: String,
    context_position: (f32, f32),
    root_origin: Rc<Cell<(f32, f32)>>,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    items: Vec<MenuItem>,
    open: bool,
    selected: Option<usize>,
    focus: FocusHandle,
    menu_focus: FocusHandle,
}
impl gpui_kit::EventEmitter<MenuEvent> for Menu {}
impl Menu {
    pub fn new(dark: bool, context: bool, items: Vec<MenuItem>, cx: &mut Context<Self>) -> Self {
        Self {
            dark,
            context,
            trigger_label: String::new(),
            context_position: (0., 0.),
            root_origin: Rc::new(Cell::new((0., 0.))),
            trigger_bounds: Rc::new(Cell::new(None)),
            items,
            open: false,
            selected: None,
            focus: cx.focus_handle(),
            menu_focus: cx.focus_handle(),
        }
    }
    pub fn set_trigger(&mut self, label: impl Into<String>, cx: &mut Context<Self>) {
        self.trigger_label = label.into();
        cx.notify();
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn set_items(&mut self, items: Vec<MenuItem>, cx: &mut Context<Self>) {
        self.items = items;
        self.selected = None;
        cx.notify();
    }
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.selected = None;
        self.menu_focus.focus(window, cx);
        cx.notify();
    }
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.emit(MenuEvent::Dismissed);
            cx.notify();
        }
    }
    fn key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => self.dismiss(cx),
            "down" | "up" => {
                let enabled: Vec<_> = self
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| !item.disabled)
                    .map(|(i, _)| i)
                    .collect();
                if !enabled.is_empty() {
                    let current = self
                        .selected
                        .and_then(|i| enabled.iter().position(|&n| n == i));
                    let next = if event.keystroke.key == "down" {
                        current.map_or(0, |i| (i + 1) % enabled.len())
                    } else {
                        current.map_or(enabled.len() - 1, |i| {
                            (i + enabled.len() - 1) % enabled.len()
                        })
                    };
                    self.selected = Some(enabled[next]);
                    cx.notify();
                }
            }
            "enter" | "space" => {
                if let Some(i) = self.selected {
                    self.activate(i, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.items.get(index).filter(|item| !item.disabled) {
            let event = MenuEvent::Selected(item.id.clone());
            cx.emit(event);
            self.open = false;
            cx.notify();
        }
    }
    fn content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let trigger_bounds = self.trigger_bounds.clone();
        div()
            .id("menu-surface")
            .test_support()
            .w(px(152.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .shadow_lg()
            .track_focus(&self.menu_focus)
            .occlude()
            .on_mouse_down_out(cx.listener(move |s, event: &MouseDownEvent, _, cx| {
                if s.open
                    && !trigger_bounds
                        .get()
                        .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    s.dismiss(cx);
                }
            }))
            .children(self.items.iter().enumerate().map(|(i, item)| {
                let selected = self.selected == Some(i);
                Button::new(item.element_id.clone())
                    .w_full()
                    .h(px(24.))
                    .px(px(8.))
                    .rounded(px(5.))
                    .bg(rgb(if selected { t.hover } else { t.panel_raised }))
                    .text_size(px(12.))
                    .text_color(rgb(if item.disabled { t.faint } else { t.text }))
                    .disabled(item.disabled)
                    .focusable(false)
                    .tab_stop(false)
                    .on_mouse_move(cx.listener(move |s, _, _, cx| {
                        if s.selected != Some(i) && !s.items[i].disabled {
                            s.selected = Some(i);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |s, _, _, cx| s.activate(i, cx)))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(item.label.clone())
                            .when_some(item.shortcut.clone(), |d, shortcut| {
                                d.child(
                                    div()
                                        .ml(px(6.))
                                        .text_size(px(11.))
                                        .text_color(rgb(t.muted))
                                        .child(shortcut),
                                )
                            }),
                    )
            }))
    }
}
impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let trigger = Button::new("trigger")
            .px(px(12.))
            .h(px(34.))
            .rounded(px(5.))
            .border_1()
            .border_color(rgb(t.border))
            .focus_visible(move |s| s.border_color(rgb(t.accent)))
            .bg(rgb(t.panel))
            .text_color(rgb(t.text))
            .text_size(px(13.))
            .child(self.trigger_label.clone())
            .track_focus(&self.focus)
            .on_click(cx.listener(|s, _, window, cx| {
                if !s.context {
                    if s.open {
                        s.dismiss(cx);
                        s.focus.focus(window, cx);
                    } else {
                        s.open(window, cx);
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|s, event: &gpui_kit::MouseUpEvent, window, cx| {
                    if s.context {
                        let origin = s.root_origin.get();
                        s.context_position = (
                            f32::from(event.position.x) - origin.0,
                            f32::from(event.position.y) - origin.1,
                        );
                        s.open(window, cx);
                    }
                }),
            );
        let bounds = self.trigger_bounds.clone();
        let popup = Popup::new(
            "menu-popup",
            div()
                .relative()
                .on_prepaint(move |rect, _, _| bounds.set(Some(rect)))
                .child(trigger),
        )
        .anchor(Anchor::BottomLeft)
        .offset(px(5.))
        .when(self.open && !self.context, |p| p.content(self.content(cx)));
        let origin = self.root_origin.clone();
        div()
            .absolute()
            .top_0()
            .left_0()
            .on_prepaint(move |bounds, _, _| {
                origin.set((f32::from(bounds.left()), f32::from(bounds.top())));
            })
            .on_key_down(cx.listener(Self::key))
            .child(popup)
            .when(self.open && self.context, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(self.context_position.0))
                        .top(px(self.context_position.1))
                        .child(self.content(cx)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    fn item(id: &str, disabled: bool) -> MenuItem {
        MenuItem {
            id: id.into(),
            element_id: id.into(),
            label: id.into(),
            shortcut: None,
            disabled,
        }
    }
    struct BoundedHost {
        menu: gpui_kit::Entity<Menu>,
    }
    impl Render for BoundedHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().relative().w(px(800.)).h(px(600.)).child(
                div()
                    .absolute()
                    .left(px(224.))
                    .top(px(310.))
                    .right_0()
                    .bottom_0()
                    .child(self.menu.clone()),
            )
        }
    }

    #[test]
    fn embedded_menu_is_intrinsic_and_handles_selection_and_outside_clicks() {
        use gpui_kit::{point, test::TestWindowExt};
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|_, cx| BoundedHost {
            menu: cx.new(|cx| {
                let mut menu = Menu::new(false, false, vec![item("first", false)], cx);
                menu.trigger_label = "Add project".into();
                menu
            }),
        });
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|host, _, cx| {
            cx.subscribe(&host.menu, move |_, _, event: &MenuEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        window.draw();
        let trigger = window.update(|_, window, _| window.find("trigger").bounds());
        assert_eq!(trigger.left(), px(224.));
        assert!(
            trigger.size.width < px(200.),
            "trigger stretched: {trigger:?}"
        );
        window.simulate_click(trigger.center(), MouseButton::Left);
        window.draw();
        let item = window.update(|_, window, _| window.find("first").bounds());
        window.simulate_click(item.center(), MouseButton::Left);
        window.draw();
        assert_eq!(
            *received.borrow(),
            vec![MenuEvent::Selected("first".into())]
        );
        window.simulate_click(trigger.center(), MouseButton::Left);
        window.draw();
        window.simulate_click(trigger.center(), MouseButton::Left);
        window.draw();
        assert_eq!(
            *received.borrow(),
            vec![MenuEvent::Selected("first".into()), MenuEvent::Dismissed]
        );
        window.simulate_click(trigger.center(), MouseButton::Left);
        window.draw();
        window.simulate_click(point(px(500.), px(400.)), MouseButton::Left);
        window.draw();
        assert_eq!(
            *received.borrow(),
            vec![
                MenuEvent::Selected("first".into()),
                MenuEvent::Dismissed,
                MenuEvent::Dismissed
            ]
        );
    }

    #[test]
    fn context_uses_pointer_coordinates_in_embedded_host() {
        use gpui_kit::{point, test::TestWindowExt};
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|_, cx| BoundedHost {
            menu: cx.new(|cx| {
                let mut menu = Menu::new(false, true, vec![item("first", false)], cx);
                menu.trigger_label = "Review".into();
                menu
            }),
        });
        window.draw();
        let trigger = window.update(|_, window, _| window.find("trigger").bounds());
        let click = point(trigger.left() + px(12.), trigger.top() + px(9.));
        window.simulate_click(click, MouseButton::Right);
        window.draw();
        assert!(window.read(|host, cx| host.menu.read(cx).is_open()));
        let surface = window.update(|_, window, _| window.find("menu-surface").bounds());
        assert_eq!(surface.left(), click.x);
        assert_eq!(surface.top(), click.y);
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|host, _, cx| {
            cx.subscribe(&host.menu, move |_, _, event: &MenuEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        window.simulate_click(point(px(500.), px(400.)), MouseButton::Left);
        assert_eq!(*received.borrow(), vec![MenuEvent::Dismissed]);
    }
    #[test]
    fn toggle_escape_and_disabled_policy() {
        use gpui_kit::{point, test::TestWindowExt};
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|_, cx| {
            let mut menu = Menu::new(
                false,
                false,
                vec![item("first", false), item("blocked", true)],
                cx,
            );
            menu.trigger_label = "Open".into();
            menu
        });
        window.draw();
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &MenuEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        let trigger = window.update(|_, window, _| window.find("trigger").bounds());
        window.simulate_click(trigger.center(), MouseButton::Left);
        window.draw();
        assert!(window.read(|menu, _| menu.is_open()));
        window.update(|menu, _, cx| {
            menu.activate(1, cx);
            assert!(received.borrow().is_empty());
        });
        window.simulate_keystroke("escape");
        window.draw();
        assert!(!window.read(|menu, _| menu.is_open()));
        window.simulate_click(trigger.center(), MouseButton::Left);
        window.draw();
        window.simulate_click(point(px(2.), px(2.)), MouseButton::Left);
        window.draw();
        assert_eq!(
            *received.borrow(),
            vec![MenuEvent::Dismissed, MenuEvent::Dismissed]
        );
    }
}

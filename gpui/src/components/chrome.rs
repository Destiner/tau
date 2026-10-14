//! Reusable native sidebar and header. Projects and presentation metadata belong to the caller.
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Duration,
};

use crate::components::{
    font,
    menus::{Menu, MenuEvent, MenuItem},
    support,
};
#[cfg(test)]
use std::cell::RefCell;

use gpui_kit::{
    Bounds, BoxShadow, Context, Entity, EventEmitter, FocusHandle, Focusable, HoverListenerMode,
    IntoElement, KeyDownEvent, ListAlignment, ListOffset, ListState, MouseButton, MouseDownEvent,
    Pixels, Point, Render, Subscription, Window,
    base::{
        Button as BaseButton, ElementExt, Input, InputBase, Placement, Positioner, TestSupportExt,
        TextView, TextViewState, TextViewStyle,
        input::{InputEvent, InputState},
    },
    div, list,
    prelude::*,
    px, rgb, rgba, svg,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Select(String),
    NewSession,
    ToggleProject,
    RemoveProject(String),
    ProjectToggle(String),
    NewSessionIn(String),
    ReorderProjects(Vec<String>),
    Rename {
        session_id: String,
        title: String,
    },
    OpenLocalProject,
    OpenRemoteProject,
    CheckUpdates,
    ToggleArchived,
    Archive(String),
    SetUnread {
        project_id: String,
        session_id: String,
        unread: bool,
    },
    Restore(String),
    Palette,
    FocusComposer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Draft,
    Working,
    Unread,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionItem {
    pub id: String,
    pub title: String,
    pub time: String,
    pub status: Option<SessionStatus>,
    pub unread: bool,
    pub can_archive: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectItem {
    pub id: String,
    pub title: String,
    /// Caller-formatted location: working directory, or connection · working directory.
    pub location: String,
    pub collapsed: bool,
    pub sessions: Vec<SessionItem>,
}
/// Parent-supplied production tooltip copy. `message` is title Markdown, not a transcript excerpt.
/// Relative time accepts production shorthand or already-expanded copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionDetails {
    pub status: String,
    pub message: String,
    pub relative_time: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarProps {
    pub sessions: Vec<SessionItem>,
    pub active: String,
    /// Legacy compatibility field; normal rows use each ProjectItem's collapsed state.
    pub collapsed: bool,
    /// Legacy compatibility field; project presence is determined by caller-supplied projects.
    pub present: bool,
    pub archived: bool,
}

/// Archive presentation metadata cannot be inferred from relative-time strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivedDetails {
    pub group: String,
    pub project: String,
    pub model: Option<String>,
}
const ARCHIVED_GROUPS: [&str; 5] = ["Today", "Yesterday", "This Week", "Last Week", "Older"];

#[derive(Clone, Debug, PartialEq, Eq)]
enum ArchiveEntry {
    Heading(String),
    Session { index: usize, id: String },
}
impl ArchiveEntry {
    fn key(&self) -> (&str, bool) {
        match self {
            Self::Heading(group) => (group, true),
            Self::Session { id, .. } => (id, false),
        }
    }
}

fn expanded_relative_time(value: &str) -> String {
    if value == "now" {
        return "just now".into();
    }
    let split = value.len().saturating_sub(1);
    let Some((count, suffix)) = value.get(..split).zip(value.get(split..)) else {
        return value.into();
    };
    if count.is_empty() || !count.bytes().all(|c| c.is_ascii_digit()) {
        return value.into();
    }
    let unit = match suffix {
        "m" => "minute",
        "h" => "hour",
        "d" => "day",
        "w" => "week",
        "y" => "year",
        _ => return value.into(),
    };
    let Ok(count) = count.parse::<u64>() else {
        return value.into();
    };
    format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" })
}

fn wash(dark: bool) -> gpui_kit::Rgba {
    rgba(if dark { 0x47526633 } else { 0x6b7d8f1a })
}

// A window-local tooltip with the production delay and surface. The anchor is
// measured during prepaint rather than inferred from text or the pointer.
type AnchorBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarWidthChanged(pub f32);

fn bounded_sidebar_width(width: f32) -> f32 {
    if width.is_finite() {
        width.round().clamp(200., 480.)
    } else {
        260.
    }
}
struct Hint {
    pending: Option<String>,
    visible: Option<String>,
    epoch: usize,
    anchors: Vec<(String, AnchorBounds)>,
    surface_bounds: AnchorBounds,
    #[cfg(test)]
    rendered_label: RefCell<Option<String>>,
}
impl Hint {
    fn new() -> Self {
        Self {
            pending: None,
            visible: None,
            epoch: 0,
            anchors: Vec::new(),
            surface_bounds: Rc::new(Cell::new(None)),
            #[cfg(test)]
            rendered_label: RefCell::new(None),
        }
    }
    fn anchor(&mut self, id: &str) -> AnchorBounds {
        if let Some((_, rect)) = self.anchors.iter().find(|(name, _)| name == id) {
            return rect.clone();
        }
        let rect = Rc::new(Cell::new(None));
        self.anchors.push((id.to_owned(), rect.clone()));
        rect
    }
    fn leave(&mut self, id: &str) -> bool {
        if self.pending.as_deref() == Some(id) || self.visible.as_deref() == Some(id) {
            self.epoch += 1;
            self.pending = None;
            self.visible = None;
            #[cfg(test)]
            self.rendered_label.replace(None);
            true
        } else {
            false
        }
    }
    fn start(&mut self, id: String) -> usize {
        self.epoch += 1;
        self.pending = Some(id);
        self.visible = None;
        self.surface_bounds.set(None);
        #[cfg(test)]
        self.rendered_label.replace(None);
        self.epoch
    }
    fn show(&mut self, id: &str, epoch: usize) -> bool {
        if self.epoch == epoch && self.pending.as_deref() == Some(id) {
            self.visible = Some(id.to_owned());
            true
        } else {
            false
        }
    }
    fn dismiss(&mut self) {
        self.epoch += 1;
        self.pending = None;
        self.visible = None;
        #[cfg(test)]
        self.rendered_label.replace(None);
    }
    #[cfg(test)]
    fn label(&self) -> Option<String> {
        self.rendered_label.borrow().clone()
    }
    fn session_view(
        &self,
        dark: bool,
        details: &SessionDetails,
        markdown: &Entity<TextViewState>,
        window: &Window,
        cx: &mut Context<Sidebar>,
    ) -> Option<gpui_kit::AnyElement> {
        let id = self.visible.as_ref()?;
        let anchor = self.anchors.iter().find(|(name, _)| name == id)?.1.get()?;
        #[cfg(test)]
        self.rendered_label.replace(Some(format!(
            "{}\n{}{}",
            details.status,
            details.message,
            details
                .relative_time
                .as_ref()
                .map(|time| format!("\n{}", expanded_relative_time(time)))
                .unwrap_or_default()
        )));
        let t = support::tokens(dark);
        // Scoped MarkdownContent.vue beats surface.css's name leading. Keep CSS's
        // fractional line boxes rather than TextView's default device-pixel snapping.
        let mut content = TextView::new(markdown)
            .text_size(px(11.))
            .line_height(gpui_kit::relative(1.62))
            .style(
                TextViewStyle::default()
                    .with_dark(dark)
                    .with_foreground(rgb(t.text).into())
                    .with_paragraph_gap(gpui_kit::rems(0.)),
            );
        content.text_style().fractional_line_height = Some(true);
        let viewport = window.viewport_size();
        let available = (anchor.top() - px(12.))
            .max(viewport.height - anchor.bottom() - px(12.))
            .max(px(0.));
        let bounds = self.surface_bounds.clone();
        let hover_id = id.clone();
        let surface = div()
            .id("chrome-tooltip")
            .test_support()
            .on_hover(cx.listener(move |s, hovered: &bool, w, cx| {
                if !*hovered {
                    s.hover(&hover_id, false, w, cx);
                }
            }))
            .occlude()
            .max_w(px(300.).min((viewport.width - px(12.)).max(px(0.))))
            .max_h(
                available
                    .min((viewport.height - px(12.)).max(px(0.)))
                    .floor(),
            )
            .min_h_0()
            .flex_shrink_0()
            .overflow_y_scroll()
            .px(px(11.))
            .py(px(9.))
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(8.),
                    rgba(if dark { 0x00000073 } else { 0x0000001f }).into(),
                )
                .blur_radius(px(10.)),
            ])
            .text_size(px(10.))
            .line_height(gpui_kit::relative(1.25))
            .text_color(rgb(t.muted))
            .child(
                div()
                    .id("session-tooltip-status")
                    .test_support()
                    .flex_shrink_0()
                    .child(details.status.clone()),
            )
            .child(
                div()
                    .id("session-tooltip-name")
                    .test_support()
                    .flex_shrink_0()
                    .mt(px(5.))
                    .min_w_0()
                    .child(content),
            )
            .when_some(details.relative_time.clone(), |d, time| {
                d.child(
                    div()
                        .id("session-tooltip-date")
                        .test_support()
                        .flex_shrink_0()
                        .mt(px(12.))
                        .child(expanded_relative_time(&time)),
                )
            });
        Some(
            gpui_kit::deferred(
                Positioner::side(anchor)
                    .placement(Placement::Bottom)
                    .align(gpui_kit::base::Align::Start)
                    .offset(px(6.))
                    .margin(px(6.))
                    .on_position(move |position| bounds.set(Some(position.bounds)))
                    .child(surface),
            )
            .priority(100)
            .into_any_element(),
        )
    }
    fn view(
        &self,
        dark: bool,
        bottom: bool,
        label: &str,
        shortcut: Option<&str>,
    ) -> Option<impl IntoElement> {
        let id = self.visible.as_ref()?;
        let anchor = self.anchors.iter().find(|(name, _)| name == id)?.1.get()?;
        #[cfg(test)]
        self.rendered_label.replace(Some(match shortcut {
            Some(shortcut) => format!("{label} {shortcut}"),
            None => label.to_owned(),
        }));
        let t = support::tokens(dark);
        let surface = div()
            .max_w(px(260.))
            .px(px(7.))
            .py(px(3.))
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(8.),
                    rgba(if dark { 0x00000073 } else { 0x0000001f }).into(),
                )
                // GPUI takes Gaussian sigma; CSS's 20px blur uses half that value.
                .blur_radius(px(10.)),
            ])
            .text_size(px(10.))
            .line_height(gpui_kit::relative(1.25))
            .text_color(rgb(t.text))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(label.to_owned())
            .when_some(shortcut.map(str::to_owned), move |d, shortcut| {
                d.child(
                    div()
                        .px(px(4.))
                        .py(px(1.))
                        .border_1()
                        .border_color(rgba(if dark { 0x6c738059 } else { 0x6e757f59 }))
                        .rounded(px(3.))
                        .text_size(px(10.))
                        .text_color(rgb(t.muted))
                        .child(shortcut),
                )
            });
        let surface = div().id("chrome-tooltip").test_support().child(surface);
        Some(
            gpui_kit::deferred(
                Positioner::side(anchor)
                    .placement(if bottom {
                        Placement::Bottom
                    } else {
                        Placement::Top
                    })
                    .align(gpui_kit::base::Align::Center)
                    .offset(px(6.))
                    .margin(px(6.))
                    .child(surface),
            )
            .priority(100),
        )
    }
}

fn icon_button_color(
    id: impl Into<gpui_kit::ElementId>,
    icon: &str,
    size: f32,
    dark: bool,
    danger: bool,
    state: (u32, Option<gpui_kit::Rgba>),
) -> BaseButton {
    icon_button_styled(id, icon, size, dark, danger, (state.0, true), state.1)
}
fn icon_button_admitted(
    id: impl Into<gpui_kit::ElementId>,
    icon: &str,
    size: f32,
    dark: bool,
    danger: bool,
    state: (u32, bool),
) -> BaseButton {
    icon_button_styled(id, icon, size, dark, danger, state, None)
}
fn icon_button_styled(
    id: impl Into<gpui_kit::ElementId>,
    icon: &str,
    size: f32,
    dark: bool,
    danger: bool,
    state: (u32, bool),
    selected_hover: Option<gpui_kit::Rgba>,
) -> BaseButton {
    let (foreground, enabled) = state;
    let t = support::tokens(dark);
    BaseButton::new(id)
        .disabled(!enabled)
        .group("chrome-icon")
        .w(px(size))
        .h(px(size))
        .p_0()
        .rounded(px(4.))
        .bg(rgba(0x00000000))
        .text_color(rgb(t.muted))
        .opacity(if enabled { 0.65 } else { 0.35 })
        .when(enabled, |b| {
            b.hover(move |b| {
                if danger {
                    b.opacity(1.).text_color(rgb(t.danger))
                } else {
                    let b = b.opacity(1.);
                    if let Some(bg) = selected_hover {
                        b.bg(bg)
                    } else {
                        b
                    }
                }
            })
        })
        .focus_visible(move |b| {
            b.bg(selected_hover.unwrap_or_else(|| wash(dark)))
                .opacity(1.)
        })
        .child(
            svg()
                .path(format!("product-icons/{icon}.svg"))
                .size(px(if size <= 24. { 12. } else { 16. }))
                .text_color(rgb(foreground))
                .when(enabled, |svg| {
                    svg.group_hover("chrome-icon", move |svg| {
                        svg.text_color(rgb(if danger { t.danger } else { foreground }))
                    })
                }),
        )
}

const SIDEBAR_MOTION: Duration = Duration::from_millis(140);

#[derive(Clone, Copy)]
struct Slide {
    from: f32,
    to: f32,
    started: Duration,
}
impl Slide {
    fn value(self, now: Duration) -> f32 {
        let progress = (now.saturating_sub(self.started).as_secs_f32()
            / SIDEBAR_MOTION.as_secs_f32())
        .clamp(0., 1.);
        let eased = 1. - (1. - progress).powi(3);
        self.from + (self.to - self.from) * eased
    }
    fn retarget(&mut self, to: f32, now: Duration, animate: bool) {
        if self.to != to {
            self.from = self.value(now);
            self.to = to;
            self.started = now;
        }
        if !animate {
            self.from = to;
        }
    }
    fn running(self, now: Duration) -> bool {
        self.from != self.to && now.saturating_sub(self.started) < SIDEBAR_MOTION
    }
}
// CSS cubic-bezier: solve x before evaluating y (time is not the curve parameter).
fn css_bezier(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if progress <= 0. || progress >= 1. {
        return progress.clamp(0., 1.);
    }
    let curve = |t: f32, a: f32, b: f32| {
        3. * (1. - t).powi(2) * t * a + 3. * (1. - t) * t * t * b + t.powi(3)
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..16 {
        let mid = (low + high) / 2.;
        if curve(mid, x1, x2) < progress {
            low = mid;
        } else {
            high = mid;
        }
    }
    curve((low + high) / 2., y1, y2)
}
#[derive(Clone, Copy)]
struct HeaderReveal {
    opacity_from: f32,
    from: f32,
    to: f32,
    started: Duration,
}
impl HeaderReveal {
    fn value(self, now: Duration) -> f32 {
        let progress = (now.saturating_sub(self.started).as_secs_f32() / 0.160).clamp(0., 1.);
        if progress >= 1. {
            return self.to;
        }
        self.from + (self.to - self.from) * css_bezier(progress, 0.2, 0.8, 0.2, 1.)
    }
    fn opacity(self, now: Duration) -> f32 {
        let progress = (now.saturating_sub(self.started).as_secs_f32() / 0.110).clamp(0., 1.);
        if progress >= 1. {
            return self.to;
        }
        self.opacity_from
            + (self.to - self.opacity_from) * css_bezier(progress, 0.25, 0.1, 0.25, 1.)
    }
    fn retarget(&mut self, to: f32, now: Duration, animate: bool) {
        if self.to != to {
            self.from = self.value(now);
            self.opacity_from = self.opacity(now);
            self.to = to;
            self.started = now;
        }
        if !animate {
            self.from = to;
            self.opacity_from = to;
        }
    }
    fn running(self, now: Duration) -> bool {
        self.from != self.to && now.saturating_sub(self.started) < Duration::from_millis(160)
    }
}
fn aggregate_status(sessions: &[SessionItem]) -> Option<SessionStatus> {
    [
        SessionStatus::Unread,
        SessionStatus::Working,
        SessionStatus::Draft,
    ]
    .into_iter()
    .find(|status| sessions.iter().any(|s| s.status == Some(*status)))
}
fn status_color(status: Option<SessionStatus>, t: &support::Tokens) -> gpui_kit::Rgba {
    match status {
        Some(SessionStatus::Unread) => rgb(t.accent),
        Some(SessionStatus::Working) => rgb(t.muted),
        Some(SessionStatus::Draft) => rgba((t.muted << 8) | 0x4d),
        None => rgba(0x00000000),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HeldSessionId {
    id: String,
    source_identity: String,
}

pub struct Sidebar {
    context_menu: Entity<Menu>,
    menu_target: Option<(String, SessionItem)>,
    menu_origin: Option<FocusHandle>,
    menu_origin_session: Option<String>,
    session_focus: BTreeMap<String, (FocusHandle, FocusHandle)>,
    menu_focus: Option<FocusHandle>,
    _menu_events: Subscription,
    animations_enabled: bool,
    clock: Rc<dyn Fn() -> Duration>,
    header_hover: BTreeSet<String>,
    session_hover: BTreeSet<String>,
    // MouseExited retains GPUI's last position; reject deferred hover until real motion.
    pointer_outside_window: bool,
    header_slides: BTreeMap<String, HeaderReveal>,
    group_slides: BTreeMap<String, Slide>,
    group_rects: BTreeMap<String, AnchorBounds>,
    pending_order: Option<Vec<String>>,
    props: SidebarProps,
    projects: Vec<ProjectItem>,
    version: String,
    update_title: String,
    update_message: String,
    active_project: Option<String>,
    session_details: BTreeMap<String, SessionDetails>,
    tooltip_markdown: BTreeMap<String, Entity<TextViewState>>,
    admin_control: Option<gpui_kit::AnyView>,
    project_rects: BTreeMap<String, AnchorBounds>,
    project_drag: Option<(String, Point<Pixels>, bool, Vec<String>)>,
    dark: bool,
    actions_interactive: bool,
    remove_project_available: bool,
    hint: Hint,
    #[cfg(test)]
    hovered_session: Option<String>,
    archived_details: BTreeMap<String, ArchivedDetails>,
    closed_groups: BTreeSet<String>,
    archived_focus: BTreeMap<String, (FocusHandle, FocusHandle)>,
    archive_entries: Vec<ArchiveEntry>,
    mounted_archive_ids: BTreeSet<String>,
    orphaned_archive_focus: Vec<FocusHandle>,
    archive_list: ListState,
    held_session_order: Option<BTreeMap<String, Vec<HeldSessionId>>>,
    session_identities: BTreeMap<String, String>,
    body_bounds: AnchorBounds,
    version_open: bool,
    version_interactive: bool,
    version_registered: Option<gpui_kit::AnyWindowHandle>,
    version_focus: FocusHandle,
    version_surface_focus: FocusHandle,
    version_origin: Option<FocusHandle>,
    project_open: bool,
    project_cursor: Option<usize>,
    project_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    menu_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    version_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    popover_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    focus: FocusHandle,
    width: f32,
    resizing: bool,
    resize_hovered: bool,
    resize_pointer_focused: bool,
    resize_suppressed: bool,
    resize_focus: FocusHandle,
    resize_bounds: AnchorBounds,
    resize_handle_bounds: AnchorBounds,
    resize_lifecycle: Vec<Subscription>,
}
impl EventEmitter<Action> for Sidebar {}
impl EventEmitter<SidebarWidthChanged> for Sidebar {}
impl Sidebar {
    pub fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.dark = dark;
        self.context_menu
            .update(cx, |menu, cx| menu.set_theme(dark, window, cx));
        for markdown in self.tooltip_markdown.values() {
            markdown.update(cx, |view, cx| view.invalidate_inline_layout(cx));
        }
        cx.notify();
    }

    pub fn new(
        props: SidebarProps,
        projects: Vec<ProjectItem>,
        version: String,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let context_menu = cx.new(|cx| Menu::new_context(dark, vec![], cx));
        let menu_events = cx.subscribe(&context_menu, |s, menu, event, cx| {
            // Outside-click dismissal can be queued immediately before a replacement opens.
            if matches!(event, MenuEvent::Dismissed) && menu.read(cx).is_open() {
                return;
            }
            if let MenuEvent::Selected(id) = event
                && s.actions_interactive
            {
                let action = match id.as_str() {
                    "read" => s
                        .menu_target
                        .as_ref()
                        .map(|(project, session)| Action::SetUnread {
                            project_id: project.clone(),
                            session_id: session.id.clone(),
                            unread: !session.unread,
                        }),
                    "archive" => s
                        .menu_target
                        .as_ref()
                        .filter(|(_, session)| session.can_archive)
                        .map(|(_, session)| Action::Archive(session.id.clone())),
                    "local" => Some(Action::OpenLocalProject),
                    "remote" => Some(Action::OpenRemoteProject),
                    "archived" => Some(Action::ToggleArchived),
                    _ => None,
                };
                if let Some(action) = action {
                    cx.emit(action);
                }
            }
            s.menu_target = None;
            cx.notify();
        });
        let executor = cx.background_executor().clone();
        let epoch = executor.now();
        let mut sidebar = Self {
            context_menu,
            menu_target: None,
            menu_origin: None,
            menu_origin_session: None,
            session_focus: BTreeMap::new(),
            menu_focus: None,
            _menu_events: menu_events,
            animations_enabled: true,
            clock: Rc::new(move || executor.now().duration_since(epoch)),
            header_hover: BTreeSet::new(),
            session_hover: BTreeSet::new(),
            pointer_outside_window: false,
            header_slides: BTreeMap::new(),
            group_slides: BTreeMap::new(),
            group_rects: BTreeMap::new(),
            pending_order: None,
            props,
            projects,
            version,
            update_title: "Updates unavailable".into(),
            update_message: "Updates are not supported in this build.".into(),
            active_project: None,
            session_details: BTreeMap::new(),
            tooltip_markdown: BTreeMap::new(),
            admin_control: None,
            project_rects: BTreeMap::new(),
            project_drag: None,
            dark,
            actions_interactive: true,
            remove_project_available: false,
            hint: Hint::new(),
            #[cfg(test)]
            hovered_session: None,
            archived_details: BTreeMap::new(),
            closed_groups: BTreeSet::new(),
            archived_focus: BTreeMap::new(),
            archive_entries: Vec::new(),
            mounted_archive_ids: BTreeSet::new(),
            orphaned_archive_focus: Vec::new(),
            archive_list: ListState::new(0, ListAlignment::Top, px(68.))
                .with_estimated_item_size(gpui_kit::size(px(248.), px(34.))),
            held_session_order: None,
            session_identities: BTreeMap::new(),
            body_bounds: Rc::new(Cell::new(None)),
            version_open: false,
            version_interactive: true,
            version_registered: None,
            version_focus: cx.focus_handle().tab_stop(true).tab_index(0),
            version_surface_focus: cx.focus_handle(),
            version_origin: None,
            project_open: false,
            project_cursor: None,
            project_bounds: Rc::new(Cell::new(None)),
            menu_bounds: Rc::new(Cell::new(None)),
            version_bounds: Rc::new(Cell::new(None)),
            popover_bounds: Rc::new(Cell::new(None)),
            focus: cx.focus_handle(),
            width: 260.,
            resizing: false,
            resize_hovered: false,
            resize_pointer_focused: false,
            resize_suppressed: false,
            resize_focus: cx.focus_handle().tab_stop(true).tab_index(0),
            resize_bounds: Rc::new(Cell::new(None)),
            resize_handle_bounds: Rc::new(Cell::new(None)),
            resize_lifecycle: Vec::new(),
        };
        sidebar.rebuild_archive();
        sidebar
    }
    pub fn width(&self) -> f32 {
        self.width
    }
    /// Preference hydration is silent; only user resizing emits width changes.
    pub fn set_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let width = bounded_sidebar_width(width);
        if self.width != width {
            self.width = width;
            cx.notify();
        }
    }
    fn change_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let previous = self.width;
        self.set_width(width, cx);
        if previous != self.width {
            cx.emit(SidebarWidthChanged(self.width));
        }
    }
    /// Business admission is independent of the version/modal surface's policy.
    pub fn set_actions_interactive(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.actions_interactive != enabled {
            self.actions_interactive = enabled;
            if !enabled {
                self.dismiss_context(cx);
                self.project_open = false;
                self.project_drag = None;
                self.pending_order = None;
                self.hint.dismiss();
            }
            cx.notify();
        }
    }
    /// Enable only after the shell wires the existing core removal operation.
    pub fn set_remove_project_available(&mut self, available: bool, cx: &mut Context<Self>) {
        if self.remove_project_available != available {
            self.remove_project_available = available;
            self.hint.dismiss();
            cx.notify();
        }
    }
    pub fn resizing(&self) -> bool {
        self.resizing
    }
    pub fn handle_highlight(&self, window: &Window) -> bool {
        self.resizing
            || (!self.resize_suppressed
                && (self.resize_hovered
                    || (self.resize_focus.is_focused(window) && !self.resize_pointer_focused)))
    }
    fn resize_to_pointer(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(bounds) = self.resize_bounds.get() {
            self.change_width(f32::from(position.x - bounds.left()), cx);
        }
    }
    fn finish_resize(&mut self, cx: &mut Context<Self>) {
        self.resizing = false;
        self.resize_suppressed = true;
        cx.notify();
    }
    fn finish_resize_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.finish_resize(cx);
        // Capture can hide the leave callback while dragging outside the handle.
        self.resize_suppressed = self
            .resize_handle_bounds
            .get()
            .is_some_and(|bounds| bounds.contains(&position));
    }
    fn resize_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.resize_pointer_focused = false;
        self.resize_suppressed = false;
        let step = if e.keystroke.modifiers.shift {
            40.
        } else {
            10.
        };
        let next = match e.keystroke.key.as_str() {
            "left" => self.width - step,
            "right" => self.width + step,
            "home" => 200.,
            "end" => 480.,
            _ => {
                cx.notify();
                return;
            }
        };
        self.change_width(next, cx);
        cx.stop_propagation();
        cx.notify();
    }
    pub fn displayed_sessions(&self, project: &str) -> Option<Vec<SessionItem>> {
        self.projects
            .iter()
            .find(|p| p.id == project)
            .map(|p| self.ordered_sessions(p))
    }
    pub fn focus_selected_row(&self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = Some(self.props.active.as_str());
        let focus = if self.props.archived {
            selected
                .filter(|id| self.mounted_archive_ids.contains(*id))
                .and_then(|id| self.archived_focus.get(id))
                .map(|pair| &pair.0)
        } else {
            selected
                .filter(|id| {
                    self.projects
                        .iter()
                        .any(|p| !p.collapsed && p.sessions.iter().any(|s| s.id == *id))
                })
                .and_then(|id| self.session_focus.get(id))
                .map(|pair| &pair.0)
        };
        focus.unwrap_or(&self.focus).focus(window, cx);
    }
    pub fn context_menu(&self) -> Entity<Menu> {
        self.context_menu.clone()
    }
    pub fn set_animations_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.animations_enabled = enabled;
        cx.notify();
    }
    fn dismiss_context(&mut self, cx: &mut Context<Self>) {
        self.context_menu.update(cx, |menu, cx| menu.dismiss(cx));
        self.menu_target = None;
    }
    fn open_context(
        &mut self,
        target: Option<(String, SessionItem)>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.actions_interactive {
            return;
        }
        let focused = window.focused(cx);
        let origin = if let Some((_, session)) = &target {
            Some(
                self.session_focus
                    .entry(session.id.clone())
                    .or_insert_with(|| (cx.focus_handle(), cx.focus_handle()))
                    .0
                    .clone(),
            )
        } else if self.context_menu.read(cx).is_open()
            || (self.menu_focus.is_some() && focused == self.menu_focus)
        {
            self.menu_origin.clone()
        } else {
            focused.clone()
        };
        if let Some((_, session)) = &target {
            self.menu_origin_session = Some(session.id.clone());
        } else if !self.context_menu.read(cx).is_open()
            && !(self.menu_focus.is_some() && focused == self.menu_focus)
        {
            self.menu_origin_session = None;
        }
        self.menu_origin = origin.clone();
        self.version_open = false;
        self.project_open = false;
        self.hint.dismiss();
        let item = |id: &str, label: &str, shortcut: Option<&str>| MenuItem {
            id: id.into(),
            element_id: format!("sidebar-menu-{id}"),
            label: label.into(),
            shortcut: shortcut.map(str::to_owned),
            disabled: false,
        };
        let items = if let Some((_, session)) = &target {
            let mut items = vec![item(
                "read",
                if session.unread {
                    "Mark as Read"
                } else {
                    "Mark as Unread"
                },
                Some("⌘⇧U"),
            )];
            if session.can_archive {
                items.push(item("archive", "Archive Session", Some("⌘⇧A")));
            }
            items
        } else {
            vec![
                item("local", "Open Local Project", None),
                item("remote", "Open Remote Project", None),
                item("archived", "Show Archived Sessions", None),
            ]
        };
        let width = if target.is_some() { 152. } else { 176. };
        self.menu_target = target;
        self.context_menu.update(cx, |menu, cx| {
            menu.set_items(items, cx);
            menu.set_min_width(px(width), cx);
            menu.open_at(position, origin, window, cx);
        });
        self.menu_focus = window.focused(cx);
        cx.notify();
    }
    pub fn set_projects(&mut self, projects: Vec<ProjectItem>, cx: &mut Context<Self>) {
        self.set_projects_with_identities(projects, BTreeMap::new(), cx);
    }
    /// Atomically supplies current session ID → stable ephemeral-object identity.
    /// Keep that identity through materialization, including saved-row registration.
    /// Captured IDs win over identity when an outgoing phase remains present.
    /// Ordinary saved sessions can omit the map entry (their ID is their identity).
    pub fn set_projects_with_identities(
        &mut self,
        projects: Vec<ProjectItem>,
        identities: BTreeMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        let identities_changed = self.session_identities != identities;
        self.session_identities = identities;
        if self.projects != projects || identities_changed {
            if self.projects.iter().map(|p| &p.id).collect::<Vec<_>>()
                != projects.iter().map(|p| &p.id).collect::<Vec<_>>()
            {
                self.project_drag = None;
            }
            self.dismiss_context(cx);
            // Rebase visual offsets when the parent accepts the release order.
            let old = self.rendered_projects();
            let tops = |projects: &[ProjectItem]| {
                let mut y = 0.;
                projects
                    .iter()
                    .map(|p| {
                        let top = y;
                        y += self
                            .group_rects
                            .get(&p.id)
                            .and_then(|r| r.get())
                            .map_or(30., |b| f32::from(b.size.height));
                        (p.id.clone(), top)
                    })
                    .collect::<BTreeMap<_, _>>()
            };
            let old_tops = tops(&old);
            let new_tops = tops(&projects);
            for (id, slide) in &mut self.group_slides {
                let delta = new_tops.get(id).copied().unwrap_or(0.)
                    - old_tops.get(id).copied().unwrap_or(0.);
                slide.from -= delta;
                slide.to -= delta;
            }
            self.pending_order = None;
            let ids: BTreeSet<_> = projects.iter().map(|project| project.id.clone()).collect();
            self.group_rects.retain(|id, _| ids.contains(id));
            self.project_rects.retain(|id, _| ids.contains(id));
            self.group_slides.retain(|id, _| ids.contains(id));
            self.header_slides.retain(|id, _| ids.contains(id));
            self.header_hover.retain(|id| ids.contains(id));
            self.projects = projects;
            self.hint.dismiss();
            cx.notify();
        }
    }
    pub fn set_active_project(&mut self, project: Option<String>, cx: &mut Context<Self>) {
        if self.active_project != project {
            self.active_project = project;
            cx.notify();
        }
    }
    pub fn set_session_details(
        &mut self,
        details: BTreeMap<String, SessionDetails>,
        cx: &mut Context<Self>,
    ) {
        if self.session_details != details {
            self.tooltip_markdown.retain(|id, _| {
                self.session_details.get(id).map(|d| &d.message)
                    == details.get(id).map(|d| &d.message)
            });
            self.session_details = details;
            cx.notify();
        }
    }
    pub fn set_admin_control(
        &mut self,
        control: Option<gpui_kit::AnyView>,
        cx: &mut Context<Self>,
    ) {
        self.admin_control = control;
        cx.notify();
    }
    pub fn set_version(&mut self, version: String, cx: &mut Context<Self>) {
        if self.version != version {
            self.version = version;
            cx.notify();
        }
    }
    pub fn set_update_details(&mut self, title: String, message: String, cx: &mut Context<Self>) {
        self.update_title = title;
        self.update_message = message;
        cx.notify();
    }
    fn rendered_projects(&self) -> Vec<ProjectItem> {
        self.projects.clone()
    }
    fn move_project_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.actions_interactive || self.project_drag.is_none() {
            return;
        }
        let original: Vec<_> = self
            .rendered_projects()
            .iter()
            .map(|p| p.id.clone())
            .collect();
        let Some((id, start, dragging, order)) = &mut self.project_drag else {
            return;
        };
        if !*dragging
            && (position.x - start.x)
                .abs()
                .max((position.y - start.y).abs())
                < px(3.)
        {
            return;
        }
        *dragging = true;
        if let Some(target) = self.group_rects.iter().find_map(|(target, rect)| {
            rect.get()
                .filter(|b| b.contains(&position))
                .map(|_| target.clone())
        }) {
            *order = original;
            let from = order.iter().position(|key| key == id);
            let to = order.iter().position(|key| key == &target);
            if let (Some(from), Some(to)) = (from, to) {
                let moving = order.remove(from);
                order.insert(to, moving);
            }
        }
        cx.notify();
    }
    fn release_project_drag(&mut self, cx: &mut Context<Self>) {
        if let Some((_, _, true, order)) = self.project_drag.take() {
            let original: Vec<_> = self
                .rendered_projects()
                .iter()
                .map(|p| p.id.clone())
                .collect();
            if order != original {
                self.pending_order = Some(order.clone());
                cx.emit(Action::ReorderProjects(order));
            }
        }
        cx.notify();
    }
    pub fn set_archived_details(
        &mut self,
        details: BTreeMap<String, ArchivedDetails>,
        cx: &mut Context<Self>,
    ) {
        if self.archived_details != details {
            self.archived_details = details;
            self.rebuild_archive();
            cx.notify();
        }
    }
    fn tooltip_details(&self, id: &str) -> Option<SessionDetails> {
        if self.props.archived {
            let session = self.props.sessions.iter().find(|s| s.id == id)?;
            Some(SessionDetails {
                status: "Archived".into(),
                message: self
                    .session_details
                    .get(id)
                    .map_or_else(|| session.title.clone(), |details| details.message.clone()),
                relative_time: (!session.time.is_empty()).then(|| session.time.clone()),
            })
        } else {
            self.session_details.get(id).cloned()
        }
    }
    fn rebuild_archive(&mut self) {
        let mut entries = Vec::new();
        // Vue buckets by calendar group, preserving caller order inside each bucket.
        let mut buckets: Vec<Vec<(usize, &SessionItem)>> =
            (0..=ARCHIVED_GROUPS.len()).map(|_| Vec::new()).collect();
        for (index, session) in self.props.sessions.iter().enumerate() {
            let bucket = self
                .archived_details
                .get(&session.id)
                .and_then(|d| ARCHIVED_GROUPS.iter().position(|g| *g == d.group))
                .unwrap_or(ARCHIVED_GROUPS.len());
            buckets[bucket].push((index, session));
        }
        for (bucket, sessions) in buckets.into_iter().enumerate() {
            if sessions.is_empty() {
                continue;
            }
            let group = ARCHIVED_GROUPS.get(bucket).copied().unwrap_or("");
            if !group.is_empty() {
                entries.push(ArchiveEntry::Heading(group.to_owned()));
            }
            if !self.closed_groups.contains(group) {
                entries.extend(sessions.into_iter().map(|(index, session)| {
                    ArchiveEntry::Session {
                        index,
                        id: session.id.clone(),
                    }
                }));
            }
        }
        let old_top = self.archive_list.logical_scroll_top();
        let same_keys = self
            .archive_entries
            .iter()
            .map(ArchiveEntry::key)
            .eq(entries.iter().map(ArchiveEntry::key));
        if !same_keys {
            // Keep the same row and pixel offset; if removed, prefer its next surviving row.
            let positions: BTreeMap<_, _> = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| (entry.key(), index))
                .collect();
            let anchor = self
                .archive_entries
                .iter()
                .skip(old_top.item_ix)
                .find_map(|old| positions.get(&old.key()).copied());
            self.archive_list.reset(entries.len());
            if !entries.is_empty() {
                self.archive_list.scroll_to(ListOffset {
                    item_ix: if old_top.item_ix == 0 && old_top.offset_in_item == px(0.) {
                        // A list pinned to its start must reveal newly inserted headings.
                        0
                    } else {
                        anchor.unwrap_or(old_top.item_ix.min(entries.len() - 1))
                    },
                    offset_in_item: old_top.offset_in_item,
                });
            }
        }
        let visible: BTreeSet<_> = entries
            .iter()
            .filter_map(|entry| match entry {
                ArchiveEntry::Session { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        self.archived_focus.retain(|id, (select, restore)| {
            if visible.contains(id.as_str()) {
                return true;
            }
            self.orphaned_archive_focus
                .extend([select.clone(), restore.clone()]);
            false
        });
        self.archive_entries = entries;
    }
    pub fn version_open(&self) -> bool {
        self.version_open
    }
    /// The shell can deny opening behind a modal or during guarded shutdown.
    pub fn set_version_interactive(&mut self, interactive: bool, cx: &mut Context<Self>) {
        let closing = !interactive && self.version_open;
        if self.version_interactive != interactive || closing {
            self.version_interactive = interactive;
            if !interactive {
                self.version_open = false;
                self.version_origin = None;
            }
            cx.notify();
        }
    }
    /// Vue keeps the update surface mounted beneath the palette. Deny new reveals
    /// and outside dismissal without taking focus from the palette's own editor.
    pub fn preserve_version_under_palette(&mut self, cx: &mut Context<Self>) {
        if self.version_interactive {
            self.version_interactive = false;
            cx.notify();
        }
    }
    /// Shared sidebar/native-menu entrypoint. This never initiates an update check.
    pub fn open_version_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.version_interactive {
            return;
        }
        if !self.version_open {
            self.version_origin = window.focused(cx);
        }
        self.dismiss_context(cx);
        self.project_open = false;
        self.hint.dismiss();
        self.version_open = true;
        self.version_surface_focus.focus(window, cx);
        cx.notify();
    }
    fn dismiss_version(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.version_open = false;
        if restore {
            if let Some(origin) = self.version_origin.take() {
                origin.focus(window, cx);
            } else {
                self.version_focus.focus(window, cx);
            }
        } else {
            self.version_origin = None;
        }
        cx.notify();
    }
    #[cfg(test)]
    pub fn tooltip_label(&self) -> Option<String> {
        self.hint.label()
    }
    #[cfg(test)]
    pub fn hovered_session(&self) -> Option<String> {
        self.hovered_session.clone()
    }
    /// Selection-only updates need not clone/compare the caller's entire archive snapshot.
    pub fn set_active_session(&mut self, id: String, cx: &mut Context<Self>) {
        if self.props.active != id {
            self.props.active = id;
            cx.notify();
        }
    }
    pub fn set_props(&mut self, props: SidebarProps, cx: &mut Context<Self>) {
        let rows_changed = self.props.sessions != props.sessions;
        let mode_changed = self.props.archived != props.archived;
        if rows_changed
            || mode_changed
            || self.props.active != props.active
            || self.props.collapsed != props.collapsed
            || self.props.present != props.present
        {
            self.dismiss_context(cx);
            if mode_changed {
                self.held_session_order = None;
                self.project_drag = None;
                self.closed_groups.clear();
            }
            self.props = props;
            if rows_changed || mode_changed {
                self.tooltip_markdown.clear();
                self.rebuild_archive();
            }
            self.hint.dismiss();
            #[cfg(test)]
            {
                self.hovered_session = None;
            }
            cx.notify();
        }
    }
    fn hover(&mut self, id: &str, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !hovered {
            #[cfg(test)]
            {
                if self.hovered_session.as_deref() == id.strip_prefix("session-") {
                    self.hovered_session = None;
                }
            }
            if id.starts_with("session-")
                && self.hint.visible.as_deref() == Some(id)
                && self.hint.surface_bounds.get().is_some()
            {
                // Keep the reader reachable across the 6px anchor-to-surface gap.
                let id = id.to_owned();
                let epoch = self.hint.epoch;
                cx.spawn_in(window, async move |entity, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    let _ = entity.update_in(cx, |s, w, cx| {
                        let pointer = w.mouse_position();
                        let inside = s
                            .hint
                            .surface_bounds
                            .get()
                            .is_some_and(|b| b.contains(&pointer))
                            || s.hint.anchors.iter().any(|(key, b)| {
                                key == &id && b.get().is_some_and(|b| b.contains(&pointer))
                            });
                        if s.hint.epoch == epoch && !inside && s.hint.leave(&id) {
                            cx.notify();
                        }
                    });
                })
                .detach();
                return;
            }
            if self.hint.leave(id) {
                cx.notify();
            }
            return;
        }
        if self.pointer_outside_window
            || self.resizing
            || self.version_open
            || self.project_open
            || self.project_drag.is_some()
        {
            return;
        }
        // The select button and its overlaid archive action both receive hover.
        // Resolve the measured action first, independent of callback order.
        let archive_id = id
            .strip_prefix("session-")
            .map(|id| format!("archive-{id}"));
        let id = archive_id
            .as_deref()
            .filter(|archive_id| {
                self.hint.anchors.iter().any(|(name, bounds)| {
                    name == archive_id
                        && bounds
                            .get()
                            .is_some_and(|b| b.contains(&window.mouse_position()))
                })
            })
            .unwrap_or(id);
        #[cfg(test)]
        {
            self.hovered_session = id.strip_prefix("session-").map(str::to_owned);
        }
        if self.hint.pending.as_deref() == Some(id) {
            return;
        }
        let epoch = self.hint.start(id.to_owned());
        let delay = if id.starts_with("session-") || id.starts_with("project-toggle") {
            700
        } else {
            450
        };
        let id = id.to_owned();
        cx.spawn_in(window, async move |entity, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            let _ = entity.update_in(cx, |s, _, cx| {
                if s.hint.show(&id, epoch) {
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn header_hover(&mut self, id: &str, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.header_hover.insert(id.to_owned());
        } else {
            self.header_hover.remove(id);
        }
        cx.notify();
    }
    fn motion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> BTreeMap<String, f32> {
        let now = (self.clock)();
        let animate = self.animations_enabled && !cx.reduce_motion();
        let projects = self.rendered_projects();
        let original: Vec<_> = projects.iter().map(|p| p.id.clone()).collect();
        let preview = self
            .project_drag
            .as_ref()
            .filter(|(_, _, dragging, _)| *dragging)
            .map(|(_, _, _, order)| order)
            .or(self.pending_order.as_ref())
            .unwrap_or(&original);
        let tops = |order: &[String]| {
            let mut y = 0.;
            order
                .iter()
                .map(|id| {
                    let top = y;
                    y += self
                        .group_rects
                        .get(id)
                        .and_then(|r| r.get())
                        .map_or(30., |b| f32::from(b.size.height));
                    (id.clone(), top)
                })
                .collect::<BTreeMap<_, _>>()
        };
        let base = tops(&original);
        let target = tops(preview);
        let mut offsets = BTreeMap::new();
        let mut running = false;
        for id in &original {
            let slide = self.group_slides.entry(id.clone()).or_insert(Slide {
                from: 0.,
                to: 0.,
                started: now,
            });
            slide.retarget(target[id] - base[id], now, animate);
            offsets.insert(id.clone(), slide.value(now));
            running |= slide.running(now);
            let chosen = self
                .project_drag
                .as_ref()
                .is_some_and(|(dragged, _, _, _)| dragged == id);
            let slide = self
                .header_slides
                .entry(id.clone())
                .or_insert(HeaderReveal {
                    opacity_from: 0.,
                    from: 0.,
                    to: 0.,
                    started: now,
                });
            slide.retarget(
                if chosen || self.header_hover.contains(id) {
                    1.
                } else {
                    0.
                },
                now,
                animate,
            );
            running |= slide.running(now);
        }
        if running {
            window.request_animation_frame();
        }
        offsets
    }
    fn session_identity<'a>(&'a self, id: &'a str) -> &'a str {
        self.session_identities.get(id).map_or(id, String::as_str)
    }
    fn ordered_sessions(&self, project: &ProjectItem) -> Vec<SessionItem> {
        let Some(held) = self
            .held_session_order
            .as_ref()
            .and_then(|order| order.get(&project.id))
        else {
            return project.sessions.clone();
        };
        held.iter()
            .filter_map(|entry| {
                project
                    .sessions
                    .iter()
                    .find(|s| s.id == entry.id)
                    .or_else(|| {
                        project
                            .sessions
                            .iter()
                            .find(|s| self.session_identity(&s.id) == entry.source_identity)
                    })
            })
            .cloned()
            .collect()
    }
    fn hold_body_order(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            if self.held_session_order.is_none() {
                self.held_session_order = Some(
                    self.projects
                        .iter()
                        .map(|project| {
                            (
                                project.id.clone(),
                                project
                                    .sessions
                                    .iter()
                                    .map(|s| HeldSessionId {
                                        id: s.id.clone(),
                                        source_identity: self.session_identity(&s.id).to_owned(),
                                    })
                                    .collect(),
                            )
                        })
                        .collect(),
                );
            }
        } else if self.held_session_order.take().is_some() {
            cx.notify();
        }
    }
    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key == "escape" && self.project_drag.is_some() {
            self.project_drag = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.project_open {
            match e.keystroke.key.as_str() {
                "down" | "up" | "home" | "end" | "pageup" | "pagedown" => {
                    self.project_cursor = Some(match e.keystroke.key.as_str() {
                        "down" => self.project_cursor.map_or(0, |cursor| (cursor + 1).min(1)),
                        "up" => self
                            .project_cursor
                            .map_or(1, |cursor| cursor.saturating_sub(1)),
                        "home" | "pageup" => 0,
                        _ => 1,
                    });
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "enter" | "space" => {
                    if let Some(cursor) = self.project_cursor {
                        self.project_open = false;
                        self.focus.focus(window, cx);
                        cx.emit(if cursor == 0 {
                            Action::OpenLocalProject
                        } else {
                            Action::OpenRemoteProject
                        });
                    }
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }
        if self.version_open {
            if e.keystroke.key == "escape" {
                self.dismiss_version(true, window, cx);
                cx.stop_propagation();
                return;
            }
            if e.keystroke.key == "enter"
                && (e.keystroke.modifiers.platform || e.keystroke.modifiers.control)
            {
                // The native preview has no updater: even Mod-Enter must remain inert.
                cx.stop_propagation();
                return;
            }
        }
        if e.keystroke.key == "escape" && self.project_open {
            self.project_open = false;
            self.focus.focus(window, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }
}
impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.version_registered != Some(window.window_handle()) {
            crate::native::register_version_surface(
                window,
                cx,
                |s| s.version_interactive,
                Self::open_version_status,
            );
            self.version_registered = Some(window.window_handle());
        }
        if self
            .orphaned_archive_focus
            .drain(..)
            .any(|focus| focus.is_focused(window))
        {
            self.focus.focus(window, cx);
        }
        if self.resize_lifecycle.is_empty() {
            self.resize_lifecycle
                .push(cx.observe_window_activation(window, |s, w, cx| {
                    if !w.is_window_active() {
                        s.hold_body_order(false, cx);
                        s.hint.dismiss();
                        s.finish_resize(cx);
                        s.resize_hovered = false;
                        s.resize_pointer_focused = false;
                        s.pointer_outside_window = true;
                        s.session_hover.clear();
                        s.header_hover.clear();
                        #[cfg(test)]
                        {
                            s.hovered_session = None;
                        }
                    }
                    // Activation changes CSS selection washes even without pointer input.
                    cx.notify();
                }));
            self.resize_lifecycle
                .push(cx.on_blur(&self.resize_focus, window, |s, _, cx| {
                    if s.resizing {
                        s.finish_resize(cx);
                    }
                    s.resize_pointer_focused = false;
                    s.resize_suppressed = false;
                    cx.notify();
                }));
        }
        // Focus callbacks may be deferred until the next active frame. Also cover
        // gesture ownership lost while the window is already inactive.
        if self.resizing && !self.resize_focus.is_focused(window) {
            self.finish_resize(cx);
        }
        let visible_sessions: BTreeSet<_> = if self.props.archived {
            BTreeSet::new()
        } else {
            self.rendered_projects()
                .into_iter()
                .filter(|project| !project.collapsed)
                .flat_map(|project| {
                    self.ordered_sessions(&project)
                        .into_iter()
                        .map(|session| session.id)
                })
                .collect()
        };
        self.session_hover
            .retain(|id| visible_sessions.contains(id));
        if self.session_focus.iter().any(|(id, (row, archive))| {
            !visible_sessions.contains(id) && (row.is_focused(window) || archive.is_focused(window))
        }) {
            self.focus.focus(window, cx);
        }
        let origin_visible = self
            .menu_origin_session
            .as_ref()
            .is_none_or(|id| visible_sessions.contains(id));
        if !origin_visible {
            if window.focused(cx).is_some()
                && (window.focused(cx) == self.menu_focus || window.focused(cx) == self.menu_origin)
            {
                self.focus.focus(window, cx);
            }
            self.menu_origin = None;
            self.menu_origin_session = None;
            self.menu_focus = None;
        }
        if !self.context_menu.read(cx).is_open()
            && self.menu_focus.is_some()
            && window.focused(cx) == self.menu_focus
            && let Some(origin) = &self.menu_origin
            && origin.contains(origin, window)
        {
            origin.focus(window, cx);
        }
        if self.props.archived {
            self.archived_focus.retain(|id, (select, restore)| {
                self.mounted_archive_ids.contains(id)
                    || select.is_focused(window)
                    || restore.is_focused(window)
            });
            self.hint.anchors.retain(|(key, _)| {
                key.strip_prefix("restore-")
                    .or_else(|| key.strip_prefix("session-"))
                    .is_none_or(|id| self.archived_focus.contains_key(id))
            });
            self.mounted_archive_ids.clear();
        }
        let t = support::tokens_for_window(self.dark, window);
        let selection = rgba(t.selected);
        let dark = self.dark;
        let admitted = self.actions_interactive;
        let remove_admitted = admitted && self.remove_project_available;
        let offsets = self.motion(window, cx);
        let now = (self.clock)();
        let body_bounds = self.body_bounds.clone();
        let mut body = div()
            .id("sidebar-body")
            .test_support()
            .on_prepaint(move |bounds, _, _| body_bounds.set(Some(bounds)))
            // Recency stays frozen under a stationary pointer, including while typing.
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(|s, hovered, _, cx| s.hold_body_order(*hovered, cx)))
            .flex_1()
            .min_h_0()
            .p(px(6.))
            .when(!self.props.archived, |d| d.overflow_y_scroll())
            .flex()
            .flex_col();
        if self.props.archived && self.props.sessions.is_empty() {
            body = body.child(
                div()
                    .id("archived-empty")
                    .test_support()
                    // Keep empty copy on the same content row as the normal project list.
                    .min_h(px(30.))
                    .px(px(5.))
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .line_height({
                        let font = window
                            .text_system()
                            .resolve_font(&gpui_kit::font(font::family()));
                        window.text_system().ascent(font, px(10.))
                            + window.text_system().descent(font, px(10.)).abs()
                    })
                    .text_color(rgb(t.faint))
                    .child("No archived sessions"),
            );
        } else if !self.props.archived {
            for project_item in self.rendered_projects() {
                let project_id = project_item.id.clone();
                let offset = offsets.get(&project_id).copied().unwrap_or(0.);
                let header_progress = self
                    .header_slides
                    .get(&project_id)
                    .map_or(0., |slide| slide.value(now));
                let header_opacity = self
                    .header_slides
                    .get(&project_id)
                    .map_or(0., |slide| slide.opacity(now));
                let group_rect = self
                    .group_rects
                    .entry(project_id.clone())
                    .or_insert_with(|| Rc::new(Cell::new(None)))
                    .clone();
                let mut group = div()
                    .id(format!("project-group-{project_id}"))
                    .test_support()
                    .relative()
                    .when(!admitted, |d| d.opacity(0.5))
                    .top(px(offset))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .on_prepaint(move |mut b, _, _| {
                        b.origin.y -= px(offset);
                        group_rect.set(Some(b));
                    });
                let active = self.active_project.as_ref().map_or_else(
                    || {
                        project_item
                            .sessions
                            .iter()
                            .any(|s| s.id == self.props.active)
                    },
                    |id| *id == project_id,
                );
                let target = |name: &str| format!("{name}-{project_id}");
                let toggle_key = target("project-toggle");
                let new_key = target("project-new");
                let remove_key = target("project-remove");
                let drag_key = format!("project-drag-{project_id}");
                let drag_anchor = self.hint.anchor(&drag_key);
                let project_anchor = self.hint.anchor(&toggle_key);
                let project_rect = self
                    .project_rects
                    .entry(project_id.clone())
                    .or_insert_with(|| Rc::new(Cell::new(None)))
                    .clone();
                let project_status = aggregate_status(&project_item.sessions);
                let project_status_key = format!(
                    "status-{}-project-{project_id}",
                    match project_status {
                        Some(SessionStatus::Unread) => "unread",
                        Some(SessionStatus::Working) => "working",
                        Some(SessionStatus::Draft) => "draft",
                        None => "none",
                    }
                );
                let project_status_anchor = self.hint.anchor(&project_status_key);
                let project = BaseButton::new(toggle_key.clone())
                    .disabled(!admitted)
                    .justify_start()
                    .min_w_0()
                    .flex_1()
                    .h(px(30.))
                    .px(px(5.))
                    .pl(px(5. + 22. * header_progress))
                    .bg(rgba(0x00000000))
                    .text_color(rgb(t.text))
                    .text_size(px(13.))
                    .on_click(cx.listener({
                        let id = project_id.clone();
                        move |s, _, _, cx| {
                            cx.stop_propagation();
                            if s.actions_interactive {
                                cx.emit(Action::ProjectToggle(id.clone()));
                            }
                        }
                    }))
                    .on_hover(cx.listener({
                        let key = toggle_key.clone();
                        move |s, h, w, cx| s.hover(&key, *h, w, cx)
                    }))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .justify_start()
                            .gap(px(3.))
                            .when(
                                project_item.collapsed
                                    && aggregate_status(&project_item.sessions).is_some(),
                                |d| {
                                    d.child(
                                        div()
                                            .id(format!("project-status-{project_id}"))
                                            .test_support()
                                            .w(px(6.))
                                            .h(px(6.))
                                            .mr(px(4.))
                                            .relative()
                                            .top(px(1.))
                                            .rounded_full()
                                            .bg(status_color(project_status, &t))
                                            .on_prepaint(move |b, _, _| {
                                                project_status_anchor.set(Some(b))
                                            })
                                            .on_hover(cx.listener(
                                                move |s, hovered, window, cx| {
                                                    s.hover(
                                                        &project_status_key,
                                                        *hovered,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            )),
                                    )
                                },
                            )
                            .child(
                                div()
                                    .id(format!("project-label-{project_id}"))
                                    .min_w_0()
                                    .truncate()
                                    .text_color(rgb(if active { t.text } else { t.muted }))
                                    .test_support()
                                    .child(project_item.title.clone()),
                            )
                            .child(
                                support::icon(
                                    if project_item.collapsed {
                                        "chevron-right"
                                    } else {
                                        "chevron-down"
                                    },
                                    11.,
                                    t.muted,
                                )
                                .relative()
                                .top(px(1.)),
                            ),
                    );
                let new_anchor = self.hint.anchor(&new_key);
                let remove_anchor = self.hint.anchor(&remove_key);
                group = group.child(
                    div()
                        .id(format!("project-row-{project_id}"))
                        .test_support()
                        .relative()
                        .track_focus(&self.focus)
                        .group("project")
                        .on_hover(cx.listener({
                            let id = project_id.clone();
                            move |s, hovered, _, cx| s.header_hover(&id, *hovered, cx)
                        }))
                        .on_click(cx.listener({
                            let id = project_id.clone();
                            move |s, _, _, cx| {
                                if s.actions_interactive {
                                    cx.emit(Action::ProjectToggle(id.clone()));
                                }
                            }
                        }))
                        .on_prepaint(move |mut b, _, _| {
                            b.origin.y -= px(offset);
                            project_rect.set(Some(b));
                        })
                        .h(px(30.))
                        .rounded(px(7.))
                        .when(
                            self.project_drag
                                .as_ref()
                                .is_some_and(|(id, _, dragging, _)| *dragging && id == &project_id),
                            |d| d.bg(wash(dark)),
                        )
                        .when(admitted, |d| d.hover(move |d| d.bg(wash(dark))))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .justify_start()
                                .on_prepaint(move |b, _, _| project_anchor.set(Some(b)))
                                .child(project),
                        )
                        .child(
                            div()
                                .on_prepaint(move |b, _, _| new_anchor.set(Some(b)))
                                .child(
                                    icon_button_admitted(
                                        new_key.clone(),
                                        "plus",
                                        24.,
                                        dark,
                                        false,
                                        (t.muted, admitted),
                                    )
                                    .when(admitted, |b| {
                                        b.opacity(0.).group_hover("project", |b| b.opacity(0.65))
                                    })
                                    .on_hover(
                                        cx.listener(move |s, h, w, cx| {
                                            s.hover(&new_key, *h, w, cx)
                                        }),
                                    )
                                    .on_click(cx.listener({
                                        let id = project_id.clone();
                                        move |s, _, _, cx| {
                                            cx.stop_propagation();
                                            if s.actions_interactive {
                                                cx.emit(Action::NewSessionIn(id.clone()));
                                            }
                                        }
                                    })),
                                ),
                        )
                        .child(
                            div()
                                .on_prepaint(move |b, _, _| remove_anchor.set(Some(b)))
                                .child(
                                    icon_button_admitted(
                                        remove_key.clone(),
                                        "trash",
                                        24.,
                                        dark,
                                        true,
                                        (t.muted, remove_admitted),
                                    )
                                    .when(remove_admitted, |b| {
                                        b.opacity(0.).group_hover("project", |b| b.opacity(0.65))
                                    })
                                    .on_hover(cx.listener(move |s, h, w, cx| {
                                        s.hover(&remove_key, *h, w, cx)
                                    }))
                                    .on_click(cx.listener({
                                        let id = project_id.clone();
                                        move |s, _, _, cx| {
                                            cx.stop_propagation();
                                            if s.actions_interactive && s.remove_project_available {
                                                cx.emit(Action::RemoveProject(id.clone()));
                                            }
                                        }
                                    })),
                                ),
                        )
                        // Only the production-sized handle overlays the toggle, never the tools.
                        .child(
                            div()
                                .id(drag_key.clone())
                                .test_support()
                                .on_prepaint(move |b, _, _| drag_anchor.set(Some(b)))
                                .on_hover(
                                    cx.listener(move |s, h, w, cx| s.hover(&drag_key, *h, w, cx)),
                                )
                                .absolute()
                                .left(px(3.))
                                .top(px((30. - 29.) / 2. + 1.))
                                .w(px(25.))
                                .h(px(29.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .id(format!("project-grip-{project_id}"))
                                        .test_support()
                                        .w(px(9.))
                                        .h(px(13.))
                                        .relative()
                                        .left(px(-8. * (1. - header_progress)))
                                        .opacity(header_opacity)
                                        .child(support::icon("grip", 9., t.muted).h(px(13.))),
                                )
                                .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        if !s.actions_interactive {
                                            return;
                                        }
                                        s.focus.focus(window, cx);
                                        s.hint.dismiss();
                                        s.dismiss_context(cx);
                                        s.pending_order = None;
                                        s.project_drag = Some((
                                            project_id.clone(),
                                            e.position,
                                            false,
                                            s.rendered_projects()
                                                .iter()
                                                .map(|p| p.id.clone())
                                                .collect(),
                                        ));
                                        cx.notify();
                                    }),
                                ),
                        ),
                );
                if !project_item.collapsed {
                    group = group.child(div().h(px(1.)));
                    let sessions = self.ordered_sessions(&project_item);
                    let empty = sessions.is_empty();
                    for session in sessions {
                        let active = self.props.active == session.id;
                        let id = session.id.clone();
                        let key = format!("session-{}", session.id);
                        let (row_focus, archive_focus) = self
                            .session_focus
                            .entry(session.id.clone())
                            .or_insert_with(|| (cx.focus_handle(), cx.focus_handle()))
                            .clone();
                        let focused = row_focus.contains_focused(window, cx)
                            || archive_focus.contains_focused(window, cx);
                        let anchor = self.hint.anchor(&key);
                        let archive_key = format!("archive-{}", session.id);
                        let archive_anchor = self.hint.anchor(&archive_key);
                        let hover_key = key.clone();
                        let move_key = key.clone();
                        let move_archive_key = archive_key.clone();
                        let move_archive_anchor = archive_anchor.clone();
                        let context_target = (project_item.id.clone(), session.clone());
                        let status_key = format!(
                            "status-{}-{}",
                            match session.status {
                                Some(SessionStatus::Unread) => "unread",
                                Some(SessionStatus::Working) => "working",
                                Some(SessionStatus::Draft) => "draft",
                                None => "none",
                            },
                            session.id
                        );
                        let status_anchor = self.hint.anchor(&status_key);
                        let title = session.title.clone();
                        let time = session.time.clone();
                        let pointer_hovered =
                            window.is_window_active() && self.session_hover.contains(&session.id);
                        let row_hover_id = session.id.clone();
                        let bg = if active { selection } else { rgba(0x00000000) };
                        // Selected wins over hover: both are translucent washes over the panel.
                        group = group.child(
                            div()
                                .relative()
                                .id(format!("session-row-{}", session.id))
                                .test_support()
                                .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
                                .on_hover(cx.listener(move |s, hovered, w, cx| {
                                    let changed = if *hovered
                                        && w.is_window_active()
                                        && !s.pointer_outside_window
                                    {
                                        s.session_hover.insert(row_hover_id.clone())
                                    } else {
                                        s.session_hover.remove(&row_hover_id)
                                    };
                                    if changed {
                                        cx.notify();
                                    }
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        s.open_context(
                                            Some(context_target.clone()),
                                            e.position,
                                            window,
                                            cx,
                                        );
                                    }),
                                )
                                .on_prepaint(move |b, _, _| anchor.set(Some(b)))
                                .child(
                                    BaseButton::new(key)
                                        .disabled(!admitted)
                                        .track_focus(&row_focus)
                                        .w_full()
                                        .h(px(29.))
                                        // Reserve the keyboard border inside the source's 7px inset.
                                        .px(px(6.))
                                        .border_1()
                                        .border_color(rgba(0x00000000))
                                        .focus_visible(move |b| b.border_color(rgb(t.accent)))
                                        .rounded(px(7.))
                                        .bg(bg)
                                        .when(admitted, |b| {
                                            b.hover(move |b| {
                                                b.bg(if active { selection } else { wash(dark) })
                                            })
                                        })
                                        .on_hover(cx.listener(move |s, h, w, cx| {
                                            s.hover(&hover_key, *h, w, cx)
                                        }))
                                        .on_mouse_move(cx.listener(
                                            move |s, e: &gpui_kit::MouseMoveEvent, w, cx| {
                                                if (s.hint.pending.is_none()
                                                    || s.hint.pending.as_deref()
                                                        == Some(&move_archive_key))
                                                    && !move_archive_anchor
                                                        .get()
                                                        .is_some_and(|b| b.contains(&e.position))
                                                {
                                                    s.hover(&move_key, true, w, cx);
                                                }
                                            },
                                        ))
                                        .on_click(cx.listener(move |s, _, _, cx| {
                                            if s.actions_interactive {
                                                cx.emit(Action::Select(id.clone()));
                                            }
                                        }))
                                        .child(
                                            div()
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .gap(px(7.))
                                                .child(
                                                    div()
                                                        .id(format!(
                                                            "session-status-{}",
                                                            session.id
                                                        ))
                                                        .test_support()
                                                        .on_prepaint(move |b, _, _| {
                                                            status_anchor.set(Some(b))
                                                        })
                                                        .on_hover(cx.listener(
                                                            move |s, hovered, window, cx| {
                                                                if session.status.is_some() {
                                                                    s.hover(
                                                                        &status_key,
                                                                        *hovered,
                                                                        window,
                                                                        cx,
                                                                    );
                                                                }
                                                            },
                                                        ))
                                                        .w(px(6.))
                                                        .h(px(6.))
                                                        .rounded_full()
                                                        .bg(status_color(session.status, &t)),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .text_ellipsis()
                                                        .text_size(px(12.))
                                                        .line_height(chrome_title_leading(
                                                            window, 12.,
                                                        ))
                                                        .text_color(rgb(t.text))
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .id(format!("session-time-{}", session.id))
                                                        .test_support()
                                                        .text_size(px(10.))
                                                        .text_color(rgb(t.faint))
                                                        .when(
                                                            session.can_archive
                                                                && (focused || pointer_hovered),
                                                            |d| d.opacity(0.),
                                                        )
                                                        .child(time),
                                                ),
                                        ),
                                )
                                .when(session.can_archive, |row| {
                                    row.child(
                                        icon_button_admitted(
                                            archive_key.clone(),
                                            "archive",
                                            24.,
                                            dark,
                                            false,
                                            (t.muted, admitted),
                                        )
                                        .track_focus(&archive_focus)
                                        .on_prepaint(move |b, _, _| archive_anchor.set(Some(b)))
                                        .on_hover(cx.listener(move |s, h, w, cx| {
                                            s.hover(&archive_key, *h, w, cx)
                                        }))
                                        .absolute()
                                        .right(px(0.))
                                        .top(px(2.))
                                        .when(admitted, |b| {
                                            b.opacity(if focused || pointer_hovered {
                                                0.65
                                            } else {
                                                0.
                                            })
                                        })
                                        .on_click(
                                            cx.listener({
                                                let id = session.id.clone();
                                                move |s, _, _, cx| {
                                                    cx.stop_propagation();
                                                    if s.actions_interactive {
                                                        cx.emit(Action::Archive(id.clone()));
                                                    }
                                                }
                                            }),
                                        ),
                                    )
                                }),
                        );
                    }
                    if empty {
                        group = group.child(
                            div()
                                .id(format!("empty-sessions-{}", project_item.id))
                                .test_support()
                                .h(px(29.))
                                .px(px(5.))
                                .flex()
                                .items_center()
                                .text_size(px(11.))
                                .text_color(rgb(t.faint))
                                .child("No active sessions"),
                        );
                    }
                    // ProjectSidebar.vue's expanded .session-list has margin: 1px 0 5px.
                    group = group.child(div().h(px(5.)).flex_shrink_0());
                }
                body = body.child(group);
            }
        }
        if self.props.archived {
            let font = window
                .text_system()
                .resolve_font(&gpui_kit::font(font::family()));
            let normal_leading = |size| {
                window.text_system().ascent(font, px(size))
                    + window.text_system().descent(font, px(size)).abs()
            };
            let name_leading = normal_leading(11.);
            let meta_leading = normal_leading(10.);
            body = body.child(
                list(
                    self.archive_list.clone(),
                    cx.processor(move |s, index: usize, window, cx| {
                        let Some(entry) = s.archive_entries.get(index).cloned() else {
                            return div().into_any_element();
                        };
                        if let ArchiveEntry::Heading(group) = entry {
                            let label = group.clone();
                            let closed = s.closed_groups.contains(&group);
                            return div()
                                .w_full()
                                .h(px(28.))
                                .flex_shrink_0()
                                .child(
                                    BaseButton::new(format!("archived-group-{group}"))
                                        .w_full()
                                        .h(px(22.))
                                        .flex_shrink_0()
                                        .mt(px(6.))
                                        .px(px(5.))
                                        .justify_start()
                                        .gap(px(4.))
                                        .bg(rgba(0x00000000))
                                        .text_size(px(10.))
                                        .text_color(rgb(t.muted))
                                        .on_click(cx.listener(move |s, _, _, cx| {
                                            if !s.closed_groups.remove(&label) {
                                                s.closed_groups.insert(label.clone());
                                            }
                                            s.rebuild_archive();
                                            cx.notify();
                                        }))
                                        .child(group.to_owned())
                                        .child(support::icon(
                                            if closed {
                                                "chevron-right"
                                            } else {
                                                "chevron-down"
                                            },
                                            10.,
                                            t.muted,
                                        )),
                                )
                                .into_any_element();
                        }
                        let ArchiveEntry::Session { index, .. } = entry else {
                            unreachable!()
                        };
                        let session = s.props.sessions[index].clone();
                        let id = session.id.clone();
                        s.mounted_archive_ids.insert(id.clone());
                        let session_key = format!("session-{id}");
                        let session_anchor = s.hint.anchor(&session_key);
                        let restore_key = format!("restore-{id}");
                        let restore_anchor = s.hint.anchor(&restore_key);
                        let active = s.props.active == id;
                        let (select_focus, restore_focus) = s
                            .archived_focus
                            .entry(id.clone())
                            .or_insert_with(|| (cx.focus_handle(), cx.focus_handle()))
                            .clone();
                        let focused =
                            select_focus.is_focused(window) || restore_focus.is_focused(window);
                        let details = s.archived_details.get(&id);
                        let mut meta = div()
                            .id(format!("archived-meta-{id}"))
                            .test_support()
                            .line_height(meta_leading)
                            .flex()
                            .gap(px(4.))
                            .overflow_hidden()
                            .text_size(px(10.))
                            .text_color(rgb(t.muted));
                        if let Some(details) = details {
                            meta = meta.child(
                                div()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .child(details.project.clone()),
                            );
                            if let Some(model) = &details.model {
                                meta = meta.child("·").child(
                                    div()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .child(model.clone()),
                                );
                            }
                        }
                        div()
                            .id(format!("archived-row-{id}"))
                            .test_support()
                            .relative()
                            .group("archived-row")
                            .w_full()
                            .h(px(34.))
                            .flex_shrink_0()
                            .rounded(px(6.))
                            .bg(if active { selection } else { rgba(0x00000000) })
                            .when(!admitted, |d| d.opacity(0.5))
                            .when(admitted, |d| {
                                d.hover(move |d| d.bg(if active { selection } else { wash(dark) }))
                            })
                            .flex()
                            .items_stretch()
                            .child(
                                BaseButton::new(format!("archived-select-{id}"))
                                    .disabled(!admitted)
                                    .track_focus(&select_focus)
                                    .on_prepaint(move |b, _, _| session_anchor.set(Some(b)))
                                    .on_hover(cx.listener(move |s, h, w, cx| {
                                        s.hover(&session_key, *h, w, cx)
                                    }))
                                    .flex_1()
                                    .min_w_0()
                                    .px(px(7.))
                                    .py(px(3.))
                                    .bg(rgba(0x00000000))
                                    .justify_start()
                                    .on_click(cx.listener(move |s, _, _, cx| {
                                        if s.actions_interactive {
                                            cx.emit(Action::Select(id.clone()));
                                        }
                                    }))
                                    .child(
                                        div()
                                            .w_full()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .id(format!("archived-name-{}", session.id))
                                                    .test_support()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .text_size(px(11.))
                                                    .line_height(name_leading)
                                                    .text_color(rgb(t.text))
                                                    .child(session.title.clone()),
                                            )
                                            .child(meta),
                                    ),
                            )
                            .child(
                                div()
                                    .id(format!("archived-time-{}", session.id))
                                    .test_support()
                                    .flex_shrink_0()
                                    .self_end()
                                    .pl(px(6.))
                                    .pr(px(8.))
                                    .pb(px(4.))
                                    .text_size(px(10.))
                                    .line_height(meta_leading)
                                    .text_color(rgb(t.faint))
                                    .group_hover("archived-row", |d| d.opacity(0.))
                                    .when(focused, |d| d.opacity(0.))
                                    .child(session.time.clone()),
                            )
                            .child(
                                icon_button_admitted(
                                    format!("restore-{}", session.id),
                                    "archive",
                                    24.,
                                    dark,
                                    false,
                                    (t.muted, admitted),
                                )
                                .absolute()
                                .right(px(2.))
                                .top(gpui_kit::relative(0.5))
                                .mt(px(-12.))
                                .when(admitted, |b| {
                                    b.opacity(0.)
                                        .group_hover("archived-row", |b| b.opacity(0.65))
                                })
                                .track_focus(&restore_focus)
                                .on_prepaint(move |b, _, _| restore_anchor.set(Some(b)))
                                .on_hover(
                                    cx.listener(move |s, h, w, cx| {
                                        s.hover(&restore_key, *h, w, cx)
                                    }),
                                )
                                .when(focused, |b| b.opacity(0.65))
                                .on_click(cx.listener({
                                    let id = session.id.clone();
                                    move |s, _, _, cx| {
                                        cx.stop_propagation();
                                        if s.actions_interactive {
                                            cx.emit(Action::Restore(id.clone()));
                                        }
                                    }
                                })),
                            )
                            .into_any_element()
                    }),
                )
                // The flex parent owns the viewport, not the list's intrinsic content size.
                .with_sizing_behavior(gpui_kit::ListSizingBehavior::Auto)
                .flex_1()
                .min_h_0()
                .w_full(),
            );
        }
        if self.rendered_projects().is_empty() && !self.props.archived {
            body = body.child(
                div()
                    .h(px(180.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(rgb(t.muted))
                    .child(support::icon("folder", 24., t.faint))
                    .child("No projects yet"),
            );
        }
        let open_anchor = self.hint.anchor("open-project");
        let project_bounds = self.project_bounds.clone();
        let archive_anchor = self.hint.anchor("archived");
        let version_bounds = self.version_bounds.clone();
        let popover_bounds = self.popover_bounds.clone();
        let mut footer = div()
            .id("sidebar-footer")
            .test_support()
            .h(px(40.))
            .p(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        div()
                            .on_prepaint(move |b, _, _| {
                                open_anchor.set(Some(b));
                                project_bounds.set(Some(b));
                            })
                            .child(
                                icon_button_admitted(
                                    "open-project",
                                    "folder-plus",
                                    28.,
                                    dark,
                                    false,
                                    (t.muted, admitted),
                                )
                                .track_focus(&self.focus)
                                .on_hover(
                                    cx.listener(|s, h, w, cx| s.hover("open-project", *h, w, cx)),
                                )
                                .on_click(cx.listener(
                                    |s, _, window, cx| {
                                        if !s.actions_interactive {
                                            return;
                                        }
                                        s.project_open = !s.project_open;
                                        s.project_cursor = None;
                                        s.focus.focus(window, cx);
                                        s.version_open = false;
                                        s.hint.dismiss();
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .on_prepaint(move |b, _, _| archive_anchor.set(Some(b)))
                            .child(
                                icon_button_color(
                                    "archived",
                                    "archive",
                                    28.,
                                    dark,
                                    false,
                                    (
                                        if self.props.archived { t.text } else { t.muted },
                                        self.props.archived.then_some(selection),
                                    ),
                                )
                                .when(self.props.archived, move |b| {
                                    b.bg(selection).text_color(rgb(t.text))
                                })
                                .on_hover(cx.listener(|s, h, w, cx| s.hover("archived", *h, w, cx)))
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(Action::ToggleArchived)),
                                ),
                            ),
                    )
                    .when_some(self.admin_control.clone(), |d, control| d.child(control)),
            )
            .child(
                div().relative().top(px(1.)).child(
                    BaseButton::new("version")
                        .accessibility_label(format!("Tau version {}", self.version))
                        .disabled(!self.version_interactive)
                        .px(px(6.))
                        .h(px(22.5))
                        .line_height(gpui_kit::relative(1.25))
                        .rounded(px(4.))
                        .bg(if self.version_open {
                            wash(dark)
                        } else {
                            rgba(0x00000000)
                        })
                        .hover(move |b| b.bg(wash(dark)).text_color(rgb(t.text)))
                        .text_color(rgb(if self.version_open { t.text } else { t.muted }))
                        .text_size(px(10.))
                        .font_family(support::mono())
                        .focus_visible(move |b| b.bg(wash(dark)).text_color(rgb(t.text)))
                        .track_focus(&self.version_focus)
                        .on_click(cx.listener(|s, _, window, cx| {
                            if s.version_open {
                                s.dismiss_version(true, window, cx);
                            } else {
                                s.open_version_status(window, cx);
                                s.version_origin = Some(s.version_focus.clone());
                            }
                        }))
                        .child(format!("v{}", self.version))
                        .on_prepaint(move |b, _, _| {
                            version_bounds.set(Some(b));
                        }),
                ),
            );
        if self.version_open
            && let Some(anchor) = self.version_bounds.get()
        {
            let font = window
                .text_system()
                .resolve_font(&gpui_kit::font(font::family()));
            let title_leading = window.text_system().ascent(font, px(11.))
                + window.text_system().descent(font, px(11.)).abs();
            let popover = div()
                .id("version-popover")
                .test_support()
                .track_focus(&self.version_surface_focus)
                .role(gpui_kit::accesskit::Role::Dialog)
                .aria_label("Tau Update")
                .w(px(250.).min((window.viewport_size().width - px(18.)).max(px(0.))))
                .flex_shrink_0()
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.panel_raised))
                .shadow(vec![
                    BoxShadow::new(
                        px(0.),
                        px(8.),
                        rgba(if dark { 0x00000073 } else { 0x0000001f }).into(),
                    )
                    .blur_radius(px(10.)),
                ])
                .occlude()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_size(px(11.))
                                .line_height(title_leading)
                                .child(self.update_title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .line_height(gpui_kit::relative(1.45))
                                .text_color(rgb(t.muted))
                                .child(self.update_message.clone()),
                        ),
                )
                .child(
                    div().flex().justify_end().child(
                        support::button_sm(
                            "check-updates",
                            "Check for Updates",
                            support::ButtonVariant::Primary,
                            dark,
                        )
                        .disabled(true)
                        .backdrop(t.panel_raised),
                    ),
                );
            footer = footer.child(
                gpui_kit::deferred(
                    Positioner::side(anchor)
                        .placement(Placement::Top)
                        .align(gpui_kit::base::Align::End)
                        .offset(px(5.))
                        .margin(px(9.))
                        .on_position(move |position| popover_bounds.set(Some(position.bounds)))
                        .child(popover),
                )
                .priority(90),
            );
        }
        let project_menu = self
            .project_open
            .then(|| {
                let anchor = self.project_bounds.get()?;
                let bounds = self.menu_bounds.clone();
                let surface = div()
                    .id("project-menu")
                    .test_support()
                    .w(px(176.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.panel_raised))
                    .shadow(vec![
                        BoxShadow::new(
                            px(0.),
                            px(8.),
                            rgba(if dark { 0x00000073 } else { 0x0000001f }).into(),
                        )
                        .blur_radius(px(10.)),
                    ])
                    .on_prepaint(move |b, _, _| bounds.set(Some(b)))
                    .flex()
                    .flex_col()
                    .child(
                        BaseButton::new("project-local")
                            .w_full()
                            .h(px(24.))
                            .px(px(8.))
                            .justify_start()
                            .bg(rgba(0x00000000))
                            .rounded(px(6.))
                            .text_size(px(11.))
                            .text_color(rgb(t.text))
                            .hover(move |b| b.bg(wash(dark)))
                            .when(self.project_cursor == Some(0), move |b| b.bg(wash(dark)))
                            .on_hover(cx.listener(|s, hovered, _, cx| {
                                if *hovered {
                                    s.project_cursor = Some(0);
                                } else if s.project_cursor == Some(0) {
                                    s.project_cursor = None;
                                }
                                cx.notify();
                            }))
                            .on_click(cx.listener(|s, _, window, cx| {
                                if !s.actions_interactive {
                                    return;
                                }
                                s.project_open = false;
                                s.focus.focus(window, cx);
                                cx.emit(Action::OpenLocalProject);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .id("project-local-label")
                                    .test_support()
                                    .child("Open Local Project"),
                            ),
                    )
                    .child(
                        BaseButton::new("project-remote")
                            .w_full()
                            .h(px(24.))
                            .px(px(8.))
                            .justify_start()
                            .bg(rgba(0x00000000))
                            .rounded(px(6.))
                            .text_size(px(11.))
                            .text_color(rgb(t.text))
                            .hover(move |b| b.bg(wash(dark)))
                            .when(self.project_cursor == Some(1), move |b| b.bg(wash(dark)))
                            .on_hover(cx.listener(|s, hovered, _, cx| {
                                if *hovered {
                                    s.project_cursor = Some(1);
                                } else if s.project_cursor == Some(1) {
                                    s.project_cursor = None;
                                }
                                cx.notify();
                            }))
                            .on_click(cx.listener(|s, _, window, cx| {
                                if !s.actions_interactive {
                                    return;
                                }
                                s.project_open = false;
                                s.focus.focus(window, cx);
                                cx.emit(Action::OpenRemoteProject);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .id("project-remote-label")
                                    .test_support()
                                    .child("Open Remote Project"),
                            ),
                    );
                Some(
                    gpui_kit::deferred(
                        Positioner::side(anchor)
                            .placement(Placement::Top)
                            .align(gpui_kit::base::Align::Start)
                            .offset(px(5.))
                            .margin(px(6.))
                            .occlude()
                            .child(surface),
                    )
                    .priority(50),
                )
            })
            .flatten();
        if let Some(id) = self
            .hint
            .visible
            .as_deref()
            .and_then(|id| id.strip_prefix("session-"))
            && !self.tooltip_markdown.contains_key(id)
            && let Some(detail) = self.tooltip_details(id)
        {
            let markdown = cx.new(|cx| TextViewState::markdown(&detail.message, cx));
            self.tooltip_markdown.clear();
            self.tooltip_markdown.insert(id.to_owned(), markdown);
        }
        let tooltip = self.hint.visible.as_deref().and_then(|id| {
            let (label, shortcut) = match id {
                _ if id.starts_with("status-unread-") => ("Unread", None),
                _ if id.starts_with("status-working-") => ("Working", None),
                _ if id.starts_with("status-draft-") => ("Draft", None),
                _ if id.starts_with("project-drag") => ("Drag to Reorder", None),
                _ if id.starts_with("project-toggle") => {
                    let project_id = id.strip_prefix("project-toggle-")?;
                    let project = self
                        .rendered_projects()
                        .into_iter()
                        .find(|project| project.id == project_id)?;
                    return self
                        .hint
                        .view(dark, true, &project.location, None)
                        .map(IntoElement::into_any_element);
                }
                _ if id.starts_with("project-new") => ("New Session", Some("⌘N")),
                _ if id.starts_with("project-remove") => (
                    if self.remove_project_available {
                        "Remove Project"
                    } else {
                        "Remove Project unavailable"
                    },
                    None,
                ),
                "open-project" => ("Open Project", Some("⌘O")),
                "archived" => (
                    if self.props.archived {
                        "Show Sessions"
                    } else {
                        "Show Archived Sessions"
                    },
                    Some("⌘⇧H"),
                ),
                _ if id.starts_with("archive-") => ("Archive Session", Some("⌘⇧A")),
                _ if id.starts_with("restore-") => ("Unarchive Session", None),
                _ => {
                    let session_id = id.strip_prefix("session-")?;
                    if let (Some(details), Some(markdown)) = (
                        self.tooltip_details(session_id),
                        self.tooltip_markdown.get(session_id),
                    ) {
                        return self.hint.session_view(dark, &details, markdown, window, cx);
                    }
                    let session = self
                        .rendered_projects()
                        .into_iter()
                        .flat_map(|p| p.sessions)
                        .find(|s| s.id == session_id)?;
                    return self
                        .hint
                        .view(dark, true, &session.title, None)
                        .map(IntoElement::into_any_element);
                }
            };
            self.hint
                .view(
                    dark,
                    id.starts_with("project-") || id.starts_with("session-"),
                    label,
                    shortcut,
                )
                .map(IntoElement::into_any_element)
        });
        let version_bounds_out = self.version_bounds.clone();
        let popover_bounds_out = self.popover_bounds.clone();
        let project_bounds_out = self.project_bounds.clone();
        let menu_bounds_out = self.menu_bounds.clone();
        let version_bounds_inside = self.version_bounds.clone();
        let popover_bounds_inside = self.popover_bounds.clone();
        let project_bounds_inside = self.project_bounds.clone();
        let menu_bounds_inside = self.menu_bounds.clone();
        let resize_bounds = self.resize_bounds.clone();
        let resize_handle_bounds = self.resize_handle_bounds.clone();
        let highlight = self.handle_highlight(window);
        let resize_handle = div()
            .id("sidebar-resize-handle")
            .test_support()
            .absolute()
            .top_0()
            .bottom_0()
            // Matches Vue's measured 9px handle: sidebar right - 6px through right + 3px.
            .right(px(-4.))
            .w(px(9.))
            .on_prepaint(move |bounds, _, _| resize_handle_bounds.set(Some(bounds)))
            .cursor(gpui_kit::CursorStyle::ResizeLeftRight)
            .occlude()
            .track_focus(&self.resize_focus)
            .tab_index(0)
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(|s, hovered, _, cx| {
                s.resize_hovered = *hovered;
                if !*hovered && !s.resizing {
                    s.resize_suppressed = false;
                }
                cx.notify();
            }))
            .capture_any_mouse_down(cx.listener(|s, e: &MouseDownEvent, w, cx| {
                // This edge is a control, including over titlebar and neighbouring content.
                w.prevent_default();
                cx.stop_propagation();
                if e.button == MouseButton::Left {
                    s.project_drag = None;
                    s.hint.dismiss();
                    s.resize_pointer_focused = true;
                    s.resize_suppressed = false;
                    s.resizing = true;
                    s.resize_focus.focus(w, cx);
                    s.resize_to_pointer(e.position, cx);
                }
            }))
            .on_key_down(cx.listener(Self::resize_key))
            .child(
                div()
                    .id("sidebar-resize-line")
                    .test_support()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(4.))
                    .w(px(1.))
                    .bg(rgb(t.muted))
                    .opacity(if highlight { 0.4 } else { 0. }),
            );
        div()
            .id("sidebar")
            .test_support()
            .relative()
            .w(px(self.width))
            .flex_shrink_0()
            .on_prepaint(move |b, _, _| resize_bounds.set(Some(b)))
            .h_full()
            .min_h_0()
            .bg(rgb(t.panel))
            .border_r_1()
            .border_color(rgb(t.border))
            .font_family(font::family())
            .text_color(rgb(t.text))
            .on_key_down(cx.listener(Self::key))
            // GPUI's focusable rows focus on any mouse button before bubble listeners.
            .capture_any_mouse_down(|e, window, _| {
                if e.button == MouseButton::Right {
                    window.prevent_default();
                }
            })
            .when(!self.props.archived, |d| {
                d.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|s, e: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        s.open_context(None, e.position, window, cx);
                    }),
                )
            })
            // Keep keyboard dispatch rooted here without making blank space a focus target.
            .child(div().absolute().size_0().track_focus(&self.focus))
            .child(
                gpui_kit::canvas(|_, _, _| {}, {
                    let entity = cx.entity().downgrade();
                    move |_, _, window, cx| {
                        if entity.upgrade().is_some_and(|s| s.read(cx).resizing) {
                            window.set_window_cursor_style(gpui_kit::CursorStyle::ResizeLeftRight);
                        }
                        let pressed = entity.clone();
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                            if phase == gpui_kit::DispatchPhase::Capture
                                && pressed.upgrade().is_some_and(|s| s.read(cx).resizing)
                            {
                                window.prevent_default();
                                cx.stop_propagation();
                            }
                        });
                        let exiting = entity.clone();
                        window.on_mouse_event(move |_: &gpui_kit::MouseExitEvent, phase, _, cx| {
                            if phase == gpui_kit::DispatchPhase::Capture {
                                let _ = exiting.update(cx, |s, cx| {
                                    s.pointer_outside_window = true;
                                    let changed = !s.session_hover.is_empty()
                                        || s.hint.pending.is_some()
                                        || s.hint.visible.is_some();
                                    s.session_hover.clear();
                                    s.hint.dismiss();
                                    s.hold_body_order(false, cx);
                                    if changed {
                                        cx.notify();
                                    }
                                });
                            }
                        });
                        let moving = entity.clone();
                        window.on_mouse_event(
                            move |e: &gpui_kit::MouseMoveEvent, phase, window, cx| {
                                if phase == gpui_kit::DispatchPhase::Capture {
                                    let _ = moving.update(cx, |s, cx| {
                                        if s.pointer_outside_window {
                                            s.pointer_outside_window = false;
                                            let hovered: BTreeSet<_> =
                                                if window.is_window_active() && !s.props.archived {
                                                    s.hint
                                                        .anchors
                                                        .iter()
                                                        .filter_map(|(id, b)| {
                                                            id.strip_prefix("session-")
                                                                .filter(|_| {
                                                                    b.get().is_some_and(|b| {
                                                                        b.contains(&e.position)
                                                                    })
                                                                })
                                                                .map(str::to_owned)
                                                        })
                                                        .collect()
                                                } else {
                                                    BTreeSet::new()
                                                };
                                            if s.session_hover != hovered {
                                                s.session_hover = hovered;
                                                cx.notify();
                                            }
                                        }
                                        // Capture also sees moves outside the body when another
                                        // control owns the gesture and hover leave is suppressed.
                                        if !s
                                            .body_bounds
                                            .get()
                                            .is_some_and(|b| b.contains(&e.position))
                                        {
                                            s.hold_body_order(false, cx);
                                        }
                                        if s.resizing {
                                            if e.pressed_button == Some(MouseButton::Left) {
                                                s.resize_to_pointer(e.position, cx);
                                            } else {
                                                // A missed release must not leave a latched gesture.
                                                s.finish_resize_at(e.position, cx);
                                            }
                                            cx.stop_propagation();
                                        } else {
                                            s.move_project_drag(e.position, cx);
                                        }
                                    });
                                }
                            },
                        );
                        window.on_mouse_event(move |e: &gpui_kit::MouseUpEvent, phase, _, cx| {
                            if phase == gpui_kit::DispatchPhase::Capture
                                && e.button == MouseButton::Left
                            {
                                let _ = entity.update(cx, |s, cx| {
                                    if s.resizing {
                                        s.resize_to_pointer(e.position, cx);
                                        s.finish_resize_at(e.position, cx);
                                        cx.stop_propagation();
                                    } else if s.project_drag.is_some() {
                                        s.move_project_drag(e.position, cx);
                                        s.release_project_drag(cx);
                                    }
                                });
                            }
                        });
                    }
                })
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                    if s.project_open
                        && !project_bounds_inside
                            .get()
                            .is_some_and(|b| b.contains(&e.position))
                        && !menu_bounds_inside
                            .get()
                            .is_some_and(|b| b.contains(&e.position))
                    {
                        s.project_open = false;
                        cx.notify();
                    }
                    if s.version_open
                        && s.version_interactive
                        && !version_bounds_inside
                            .get()
                            .is_some_and(|b| b.contains(&e.position))
                        && !popover_bounds_inside
                            .get()
                            .is_some_and(|b| b.contains(&e.position))
                    {
                        s.dismiss_version(false, window, cx);
                    }
                }),
            )
            .on_mouse_down_out(cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                if s.project_open
                    && !project_bounds_out
                        .get()
                        .is_some_and(|b| b.contains(&e.position))
                    && !menu_bounds_out
                        .get()
                        .is_some_and(|b| b.contains(&e.position))
                {
                    s.project_open = false;
                    cx.notify();
                }
                if s.version_open
                    && s.version_interactive
                    && !version_bounds_out
                        .get()
                        .is_some_and(|b| b.contains(&e.position))
                    && !popover_bounds_out
                        .get()
                        .is_some_and(|b| b.contains(&e.position))
                {
                    s.dismiss_version(false, window, cx);
                }
            }))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .id("sidebar-blank")
                    .test_support(),
            )
            .child(body)
            .child(footer)
            // Deferred painting keeps the 4px overhang above adjacent pane chrome.
            .child(gpui_kit::deferred(resize_handle).priority(4))
            .child(self.context_menu.clone())
            .when_some(project_menu, |d, menu| d.child(menu))
            .when_some(tooltip, |d, tip| d.child(tip))
    }
}

pub struct Header {
    renamable: bool,
    active: String,
    title: String,
    dark: bool,
    editing: Option<String>,
    palette_focus_pending: bool,
    has_project: bool,
    new_session_enabled: bool,
    palette_enabled: bool,
    input: Entity<InputState>,
    input_style: gpui_kit::base::input::InputEditorStyle,
    _input_events: Subscription,
    _blur: Subscription,
    _activation: Subscription,
    input_bounds: AnchorBounds,
    hint: Hint,
    #[cfg(test)]
    titlebar_gestures: Vec<TitlebarGesture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TitlebarGesture {
    Drag,
    DoubleClick,
}

fn titlebar_gesture(event: &MouseDownEvent, excluded: &[AnchorBounds]) -> Option<TitlebarGesture> {
    if event.button != MouseButton::Left
        || excluded
            .iter()
            .any(|bounds| bounds.get().is_some_and(|b| b.contains(&event.position)))
    {
        return None;
    }
    match event.click_count {
        1 => Some(TitlebarGesture::Drag),
        2 => Some(TitlebarGesture::DoubleClick),
        _ => None,
    }
}

impl EventEmitter<Action> for Header {}
impl Header {
    pub fn set_theme(&mut self, dark: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.dark = dark;
        let mut style = self.input_style.clone();
        let theme = support::input_style(dark);
        style.foreground = theme.foreground;
        style.muted_foreground = theme.muted_foreground;
        style.background = theme.background;
        style.border = theme.border;
        style.selection = theme.selection;
        style.caret = theme.caret;
        self.set_editor_style(style, cx);
    }

    #[cfg(test)]
    pub fn editor_style(&self) -> &gpui_kit::base::input::InputEditorStyle {
        &self.input_style
    }

    pub fn set_editor_style(
        &mut self,
        style: gpui_kit::base::input::InputEditorStyle,
        cx: &mut Context<Self>,
    ) {
        self.input_style = style.clone();
        self.input.update(cx, |editor, cx| {
            editor.set_editor_style(style);
            cx.notify();
        });
        cx.notify();
    }

    pub fn new(
        active: String,
        title: String,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |s, _| s.set_editor_style(support::input_style(dark)));
        let events = cx.subscribe_in(&input, window, |s, _, e: &InputEvent, window, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                s.close(true, window, cx);
            }
        });
        let focus = input.read(cx).focus_handle(cx).clone();
        let blur = cx.on_blur(&focus, window, |s, window, cx| {
            // Deactivation emits blur without transferring the editor's focus handle.
            let retained_focus = s.input.read(cx).focus_handle(cx).is_focused(window);
            if !s.palette_focus_pending && (window.is_window_active() || !retained_focus) {
                s.close(true, window, cx);
            }
        });
        Self {
            renamable: true,
            active: active.clone(),
            title,
            dark,
            editing: None,
            palette_focus_pending: false,
            has_project: !active.is_empty(),
            new_session_enabled: true,
            palette_enabled: true,
            input,
            input_style: support::input_style(dark),
            _input_events: events,
            _blur: blur,
            _activation: cx.observe_window_activation(window, |s, w, cx| {
                if !w.is_window_active() {
                    s.hint.dismiss();
                }
                cx.notify();
            }),
            input_bounds: Rc::new(Cell::new(None)),
            hint: Hint::new(),
            #[cfg(test)]
            titlebar_gestures: Vec::new(),
        }
    }
    pub fn set_session(
        &mut self,
        active: String,
        title: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active != active {
            self.editing = None;
            self.hint.dismiss();
        }
        if self.active != active || self.title != title {
            if self.active != active {
                self.has_project = !active.is_empty();
            }
            self.active = active;
            self.title = title;
            cx.notify();
        }
    }
    pub fn set_renamable(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.renamable != enabled {
            self.renamable = enabled;
            if !enabled {
                self.editing = None;
                self.hint.dismiss();
            }
            cx.notify();
        }
    }
    pub fn set_action_admission(
        &mut self,
        has_project: bool,
        new_session: bool,
        palette: bool,
        cx: &mut Context<Self>,
    ) {
        if (
            self.has_project,
            self.new_session_enabled,
            self.palette_enabled,
        ) != (has_project, new_session, palette)
        {
            self.has_project = has_project;
            self.new_session_enabled = new_session;
            self.palette_enabled = palette;
            self.hint.dismiss();
            cx.notify();
        }
    }
    pub fn set_title(&mut self, title: String, cx: &mut Context<Self>) {
        if self.title != title {
            self.title = title;
            cx.notify();
        }
    }
    pub fn renaming(&self) -> bool {
        self.editing.is_some()
    }
    pub fn prepare_palette_open(&mut self) {
        self.palette_focus_pending = true;
    }
    pub fn finish_palette_focus(&mut self) {
        self.palette_focus_pending = false;
    }
    #[cfg(test)]
    pub fn tooltip_label(&self) -> Option<String> {
        self.hint.label()
    }
    pub fn editor(&self) -> Entity<InputState> {
        self.input.clone()
    }
    pub fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active.is_empty() || !self.renamable {
            return;
        }
        self.editing = Some(self.active.clone());
        self.hint.dismiss();
        self.input.update(cx, |input, cx| {
            input.set_value(&self.title, window, cx);
            input.focus_handle(cx).focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }
    fn close(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_with_focus(commit, true, window, cx);
    }
    fn close_with_focus(
        &mut self,
        commit: bool,
        return_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.editing.take() else {
            return;
        };
        let was_focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        if commit {
            let title = tau_session::session::normalize_session_name(&self.input.read(cx).value());
            if !title.is_empty() && id == self.active && title != self.title {
                cx.emit(Action::Rename {
                    session_id: id,
                    title,
                });
            }
        }
        if was_focused {
            if return_focus {
                cx.emit(Action::FocusComposer);
            } else {
                // Capture precedes the clicked target's normal focus handling.
                window.blur(cx);
            }
        }
        cx.notify();
    }
    fn hover(&mut self, id: &str, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !hovered {
            if self.hint.leave(id) {
                cx.notify();
            }
            return;
        }
        if !window.is_window_active() {
            return;
        }
        let epoch = self.hint.start(id.to_owned());
        let id = id.to_owned();
        cx.spawn_in(window, async move |entity, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(450))
                .await;
            let _ = entity.update_in(cx, |s, _, cx| {
                if s.hint.show(&id, epoch) {
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn key(&mut self, e: &KeyDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key == "escape" && self.editing.is_some() {
            self.close(false, w, cx);
            cx.stop_propagation();
        }
    }
}
// Button's neutral 1em line box is smaller than Inter's ascent + descent.
// Clipped labels need a complete typographic box, not a shifted baseline.
fn chrome_title_leading(window: &Window, size: f32) -> Pixels {
    let system = window.text_system();
    let face = system.resolve_font(&gpui_kit::font(font::family()));
    title_leading(
        system.ascent(face, px(size)),
        system.descent(face, px(size)),
    )
}

fn title_leading(ascent: Pixels, descent: Pixels) -> Pixels {
    ascent + descent.abs()
}

impl Render for Header {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let dark = self.dark;
        let name_anchor = self.hint.anchor("session-name");
        let new_anchor = self.hint.anchor("new-session");
        if !self.has_project {
            new_anchor.set(None);
        }
        let palette_anchor = self.hint.anchor("palette");
        let input_bounds = self.input_bounds.clone();
        let outside_bounds = self.input_bounds.clone();
        let palette_pointer_bounds = palette_anchor.clone();
        let entity = cx.entity().downgrade();
        let titlebar_exclusions = vec![
            if self.editing.is_some() {
                self.input_bounds.clone()
            } else {
                name_anchor.clone()
            },
            new_anchor.clone(),
            palette_anchor.clone(),
        ];
        let heading = if self.editing.is_some() {
            let font = window
                .text_system()
                .resolve_font(&gpui_kit::font(font::family()));
            let measure = window
                .text_system()
                .advance(font, px(12.), '0')
                .map(|advance| advance.width * 60.)
                .unwrap_or(px(360.));
            div()
                .id("rename-input")
                .test_support()
                .flex_1()
                .min_w_0()
                .h(px(21.4))
                .key_context("ChromeRename")
                .child(
                    InputBase::new("rename-field")
                        .w_full()
                        .max_w(measure)
                        .h_full()
                        .px(px(5.))
                        .py(px(2.))
                        .text_size(px(12.))
                        .line_height(gpui_kit::relative(1.45))
                        .on_prepaint(move |b, _, _| input_bounds.set(Some(b)))
                        .child(Input::new(&self.input)),
                )
                .into_any_element()
        } else {
            let font = window
                .text_system()
                .resolve_font(&gpui_kit::font(font::family()));
            let measure = window
                .text_system()
                .advance(font, px(13.), '0')
                .map(|advance| advance.width * 60.)
                .unwrap_or(px(390.));
            div()
                .flex()
                .min_w_0()
                .max_w(measure)
                .justify_start()
                .on_prepaint(move |b, _, _| name_anchor.set(Some(b)))
                .child(
                    BaseButton::new("session-name")
                        .accessibility_label(self.title.clone())
                        .focusable(self.renamable && !self.active.is_empty())
                        .justify_start()
                        .min_w_0()
                        .max_w_full()
                        .px(px(5.))
                        .py(px(2.))
                        // Production Vue: normal Inter leading + 2px vertical padding = 20px.
                        .h(px(20.))
                        .rounded(px(5.))
                        .bg(rgba(0x00000000))
                        .text_color(rgb(t.text))
                        .text_size(px(13.))
                        .line_height(chrome_title_leading(window, 13.))
                        .when(self.renamable && !self.active.is_empty(), |b| {
                            b.cursor_text()
                                .hover(move |b| b.bg(wash(dark)))
                                .focus_visible(move |b| b.bg(wash(dark)))
                                .on_hover(
                                    cx.listener(|s, h, w, cx| s.hover("session-name", *h, w, cx)),
                                )
                        })
                        .on_click(cx.listener(|s, _, w, cx| s.begin_rename(w, cx)))
                        .child(div().min_w_0().truncate().child(self.title.clone())),
                )
                .into_any_element()
        };
        let tip = self.hint.visible.as_deref().and_then(|id| {
            let (name, shortcut) = match id {
                "session-name" if self.renamable && !self.active.is_empty() => {
                    ("Rename Session", "⌘⇧R")
                }
                "new-session" => ("New Session", "⌘N"),
                "palette" => ("Command Palette", "⌘K"),
                _ => return None,
            };
            self.hint.view(dark, true, name, Some(shortcut))
        });
        div()
            .id("header")
            .test_support()
            .relative()
            .h(px(30.))
            .min_h(px(30.))
            .px(px(8.))
            .pl(px(12.))
            .border_b_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.canvas))
            .font_family(font::family())
            .flex()
            .items_center()
            .gap(px(6.))
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, event: &MouseDownEvent, window, _| {
                    if let Some(gesture) = titlebar_gesture(event, &titlebar_exclusions) {
                        #[cfg(test)]
                        s.titlebar_gestures.push(gesture);
                        #[cfg(not(test))]
                        let _ = s;
                        match gesture {
                            TitlebarGesture::Drag => {
                                // TestPlatform panics on native drag; tests assert the routed gesture.
                                #[cfg(not(test))]
                                window.start_window_move();
                            }
                            TitlebarGesture::DoubleClick => window.titlebar_double_click(),
                        }
                    }
                }),
            )
            .child(
                gpui_kit::canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                            if phase != gpui_kit::DispatchPhase::Capture {
                                return;
                            }
                            if palette_pointer_bounds
                                .get()
                                .is_some_and(|b| b.contains(&e.position))
                            {
                                // Vue's pointerdown.prevent retains the rename origin.
                                window.prevent_default();
                                return;
                            }
                            if !outside_bounds
                                .get()
                                .is_some_and(|b| b.contains(&e.position))
                            {
                                let _ = entity.update(cx, |s, cx| {
                                    if !s.palette_focus_pending {
                                        s.close_with_focus(true, false, window, cx);
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .id("outside")
                    .test_support()
                    .flex_1()
                    .min_w_0()
                    .ml(px(-5.))
                    .flex()
                    .justify_start()
                    .child(heading),
            )
            .when(self.has_project, |d| {
                d.child(
                    div()
                        .on_prepaint(move |b, _, _| new_anchor.set(Some(b)))
                        .child(
                            icon_button_admitted(
                                "new-session",
                                "plus",
                                28.,
                                dark,
                                false,
                                (t.muted, self.new_session_enabled),
                            )
                            .w(px(24.))
                            .h(px(24.))
                            .accessibility_label("New Session")
                            .on_hover(cx.listener(|s, h, w, cx| s.hover("new-session", *h, w, cx)))
                            .on_click(cx.listener(|s, _, _, cx| {
                                if s.has_project && s.new_session_enabled {
                                    cx.emit(Action::NewSession);
                                }
                            })),
                        ),
                )
            })
            .child(
                div()
                    .on_prepaint(move |b, _, _| palette_anchor.set(Some(b)))
                    .child(
                        icon_button_admitted(
                            "palette",
                            "search",
                            28.,
                            dark,
                            false,
                            (t.muted, self.palette_enabled),
                        )
                        .w(px(24.))
                        .h(px(24.))
                        .accessibility_label("Open Command Palette")
                        .on_hover(cx.listener(|s, h, w, cx| s.hover("palette", *h, w, cx)))
                        .on_click(cx.listener(|s, _, _, cx| {
                            if s.palette_enabled {
                                cx.emit(Action::Palette);
                            }
                        })),
                    ),
            )
            .when_some(tip, |d, tip| d.child(tip))
    }
}

#[cfg(test)]
#[path = "chrome/archive_tests.rs"]
mod archive_tests;
#[cfg(test)]
#[path = "chrome/regressions.rs"]
mod regressions;

#[cfg(all(test, target_os = "macos"))]
mod text_tests;

#[cfg(test)]
#[path = "chrome/resize_tests.rs"]
mod resize_tests;

#[cfg(test)]
#[path = "chrome/header_geometry_tests.rs"]
mod header_geometry_tests;

#[cfg(test)]
#[path = "chrome/behaviour_tests.rs"]
mod behaviour_tests;

#[cfg(test)]
#[path = "chrome/version_tests.rs"]
mod version_tests;

#[cfg(test)]
#[path = "chrome/sidebar_audit_tests.rs"]
mod sidebar_audit_tests;

#[cfg(test)]
#[path = "chrome/inactive_tests.rs"]
mod inactive_tests;

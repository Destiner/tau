//! Retained SSH form and directory browser. Window-owned controls are rebuilt on mount.
use super::*;
use gpui_kit::base::{Dialog, DialogBackdrop, DialogPopup, Input, InputBase, input::InputState};
use gpui_kit::{
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, KeyBinding, ScrollHandle,
    TestSupportExt, rgba,
};

gpui_kit::actions!(
    native_remote,
    [
        NextRow,
        PreviousRow,
        ParentDirectory,
        SelectCurrent,
        Submit,
        KeepFocus,
        Cancel
    ]
);
use service::{RemoteCommand, RemoteEvent, RemoteOperation, RemoteResult};
use tau_core::models::RemoteDirectoryListing;

#[derive(Default)]
pub(super) struct State {
    pub(super) view: Option<Entity<RemoteDialog>>,
    open: bool,
    connection: String,
    filter: String,
    listing: Option<RemoteDirectoryListing>,
    cursor: usize,
    error: String,
    revision: u64,
    pending: Option<RemoteCommand>,
    retry: Option<RemoteCommand>,
    subscription: Option<Subscription>,
}
impl State {
    pub(super) fn is_busy(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn is_open(&self) -> bool {
        self.open
    }
    fn options(&self) -> Vec<OptionRow> {
        let Some(listing) = &self.listing else {
            return vec![];
        };
        let mut rows = vec![];
        if listing.working_directory != "/" {
            let parent = listing
                .working_directory
                .trim_end_matches('/')
                .rsplit_once('/')
                .map(|(p, _)| if p.is_empty() { "/" } else { p })
                .unwrap_or("/");
            rows.push(OptionRow {
                label: "Go Back".into(),
                path: parent.into(),
                select: false,
                back: true,
            });
        }
        rows.push(OptionRow {
            label: format!("Select {}", listing.working_directory),
            path: listing.working_directory.clone(),
            select: true,
            back: false,
        });
        rows.extend(
            listing
                .directories
                .iter()
                .filter(|d| d.name.to_lowercase().contains(&self.filter.to_lowercase()))
                .map(|d| OptionRow {
                    label: d.name.clone(),
                    path: d.path.clone(),
                    select: false,
                    back: false,
                }),
        );
        rows
    }
    fn cancel(&mut self) {
        if self.is_busy() {
            return;
        }
        self.open = false;
        self.revision += 1;
        self.view = None;
    }
    fn accepts(&self, command: &RemoteCommand) -> bool {
        self.open && self.pending.as_ref() == Some(command)
    }
}
#[derive(Clone)]
struct OptionRow {
    label: String,
    path: String,
    select: bool,
    back: bool,
}
#[derive(Clone)]
enum UiEvent {
    Changed(String),
    Submit,
    Choose(OptionRow),
    Cancel,
    Retry,
}

impl Shell {
    pub(super) fn open_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.loaded
            || self.lifecycle.is_some()
            || self.confirmation.is_some()
            || self.error.is_some()
            || self.remote.is_open()
            || self.remote.is_busy()
            || self.picker_busy
            || self.session_actions_modal()
        {
            return;
        }
        self.remote.revision += 1;
        self.remote.listing = None;
        self.remote.filter.clear();
        self.remote.cursor = 0;
        self.remote.error.clear();
        self.remote.retry = None;
        self.remote.open = true;
        self.mount_remote(window, cx);
        cx.notify();
    }
    // Keep this guard independent of session-actions' internal representation.
    fn session_actions_modal(&self) -> bool {
        self.sessions
            .get(&self.active)
            .is_some_and(|live| live.extension.is_some())
    }
    pub(super) fn focus_remote(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote.open
            && self.lifecycle.is_none()
            && self.confirmation.is_none()
            && self.error.is_none()
            && !self.session_actions_modal()
            && let Some(view) = &self.remote.view
        {
            view.read(cx)
                .input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        }
    }
    pub(super) fn mount_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remote.subscription = None;
        self.remote.view = None;
        if !self.remote.open {
            return;
        }
        let value = if self.remote.listing.is_some() {
            self.remote.filter.clone()
        } else {
            self.remote.connection.clone()
        };
        let view = cx.new(|cx| RemoteDialog::new(value, self.dark, window, cx));
        self.remote.subscription = Some(cx.subscribe_in(
            &view,
            window,
            |s, view, event: &UiEvent, _, cx| {
                if s.remote.view.as_ref() != Some(view)
                    || s.lifecycle.is_some()
                    || s.confirmation.is_some()
                    || s.error.is_some()
                    || s.session_actions_modal()
                    || !s.remote.open
                    || s.remote.is_busy()
                {
                    return;
                }
                match event {
                    UiEvent::Changed(value) => {
                        if s.remote.listing.is_some() {
                            s.remote.filter = value.clone();
                            s.remote.cursor = 0;
                        } else {
                            s.remote.connection = value.clone();
                            s.remote.retry = None;
                            s.remote.error.clear();
                        }
                    }
                    UiEvent::Cancel => {
                        s.remote.cancel();
                        s.restore_input_focus(cx);
                    }
                    UiEvent::Retry => {
                        if let Some(mut command) = s.remote.retry.clone() {
                            s.remote.revision += 1;
                            command.revision = s.remote.revision;
                            s.remote.pending = Some(command.clone());
                            s.remote.error.clear();
                            s.dispatch(Command::Remote(command), cx);
                        }
                    }
                    UiEvent::Submit => s.remote_request(RemoteOperation::Probe, String::new(), cx),
                    UiEvent::Choose(row) => s.remote_request(
                        if row.select {
                            RemoteOperation::Import
                        } else {
                            RemoteOperation::List
                        },
                        row.path.clone(),
                        cx,
                    ),
                }
                s.refresh_window(cx);
                cx.notify();
            },
        ));
        self.remote.view = Some(view);
        self.sync_remote(window, cx);
        self.focus_remote(window, cx);
    }
    fn remote_request(&mut self, operation: RemoteOperation, path: String, cx: &mut Context<Self>) {
        if self.remote.is_busy() {
            return;
        }
        if self.remote.connection.trim().is_empty() {
            self.remote.error = "Enter an SSH connection string.".into();
            return;
        }
        self.remote.revision += 1;
        let command = RemoteCommand {
            revision: self.remote.revision,
            operation,
            connection: self.remote.connection.trim().into(),
            path,
            host: self
                .remote
                .listing
                .as_ref()
                .map(|l| l.host.clone())
                .unwrap_or_default(),
        };
        self.remote.pending = Some(command.clone());
        self.remote.error.clear();
        self.remote.retry = None;
        self.dispatch(Command::Remote(command), cx);
    }
    pub(super) fn remote_failed(&mut self, command: RemoteCommand, cx: &mut Context<Self>) {
        self.remote_event(
            RemoteEvent {
                command,
                result: Err(()),
            },
            cx,
        );
    }
    pub(super) fn remote_event(&mut self, event: RemoteEvent, cx: &mut Context<Self>) {
        if !self.remote.accepts(&event.command) {
            return;
        }
        self.remote.pending = None;
        match event.result {
            Ok(RemoteResult::Listing(listing)) => {
                self.remote.connection = listing.connection_string.clone();
                self.remote.listing = Some(listing);
                self.remote.filter.clear();
                self.remote.cursor = 0;
                self.remote.error.clear();
                self.remote.retry = None;
            }
            Ok(RemoteResult::Imported(project)) => {
                self.remote.open = false;
                self.remote.view = None;
                self.remote.listing = None;
                self.remote.retry = None;
                self.merge_imported_project(project);
                self.restore_input_focus(cx);
            }
            Err(()) => {
                self.remote.error = match event.command.operation {
                    RemoteOperation::Probe => {
                        "Could not connect over SSH. Check the connection and try again."
                    }
                    RemoteOperation::List => "Could not read the remote directory. Try again.",
                    RemoteOperation::Import => "Could not import the remote project. Try again.",
                }
                .into();
                self.remote.retry = Some(event.command);
            }
        }
        self.refresh_window(cx);
        cx.notify();
    }
    fn merge_imported_project(&mut self, project: ProjectSummary) {
        self.merge_workspace_project(project);
    }
    pub(super) fn sync_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.remote.view.clone() else {
            return;
        };
        let directory = self.remote.listing.is_some();
        let value = if directory {
            self.remote.filter.clone()
        } else {
            self.remote.connection.clone()
        };
        let rows = self.remote.options();
        let busy = self.remote.is_busy()
            || self.lifecycle.is_some()
            || self.confirmation.is_some()
            || self.error.is_some()
            || self.session_actions_modal();
        view.update(cx, |v, cx| {
            let transition = v.directory != directory;
            let became_idle = v.busy && !busy;
            let next_path = self
                .remote
                .listing
                .as_ref()
                .map(|l| l.working_directory.clone())
                .unwrap_or_default();
            if v.path != next_path {
                v.cursor = 0;
                v.scroll.scroll_to_item(0);
            }
            v.directory = directory;
            if v.rows.len() != rows.len() {
                v.cursor = 0;
            }
            v.rows = rows;
            v.busy = busy;
            let status = self
                .remote
                .pending
                .as_ref()
                .map(|command| match command.operation {
                    RemoteOperation::Probe => "Connecting…",
                    RemoteOperation::List => "Loading directories…",
                    RemoteOperation::Import => "Importing project…",
                })
                .unwrap_or_default();
            v.set_status(status, self.remote.pending.as_ref().map(|c| c.revision), cx);
            v.error = self.remote.error.clone();
            v.dark = self.dark;
            v.path = self
                .remote
                .listing
                .as_ref()
                .map(|l| l.working_directory.clone())
                .unwrap_or_default();
            v.input.update(cx, |input, cx| {
                input.set_readonly(busy, cx);
                input.set_placeholder(
                    if directory {
                        "Filter directories"
                    } else {
                        "ssh user@example -p 1234"
                    },
                    window,
                    cx,
                );
                let mut style = support::input_style(self.dark);
                if busy {
                    style.foreground = rgb(support::tokens(self.dark).muted).into();
                }
                input.set_editor_style(style);
                if input.value().as_ref() != value {
                    input.set_value(value, window, cx);
                }
            });
            if transition {
                v.cursor = 0;
            }
            if !busy && (transition || became_idle) {
                v.input.read(cx).focus_handle(cx).focus(window, cx);
            }
            cx.notify();
        });
    }
}

pub(super) struct RemoteDialog {
    input: Entity<InputState>,
    focus: FocusHandle,
    directory: bool,
    busy: bool,
    dark: bool,
    error: String,
    status: &'static str,
    status_revision: Option<u64>,
    progress_visible: bool,
    progress_task: Option<gpui_kit::Task<()>>,
    path: String,
    rows: Vec<OptionRow>,
    cursor: usize,
    scroll: ScrollHandle,
    _input_events: Subscription,
    _focus_events: Vec<Subscription>,
}
impl EventEmitter<UiEvent> for RemoteDialog {}
impl RemoteDialog {
    fn new(value: String, dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.bind_keys([
            KeyBinding::new("down", NextRow, Some("NativeRemote > Input")),
            KeyBinding::new("up", PreviousRow, Some("NativeRemote > Input")),
            KeyBinding::new("cmd-up", ParentDirectory, Some("NativeRemote > Input")),
            KeyBinding::new("ctrl-up", ParentDirectory, Some("NativeRemote > Input")),
            KeyBinding::new("cmd-enter", SelectCurrent, Some("NativeRemote > Input")),
            KeyBinding::new("ctrl-enter", SelectCurrent, Some("NativeRemote > Input")),
            KeyBinding::new("enter", Submit, Some("NativeRemote > Input")),
            KeyBinding::new("tab", KeepFocus, Some("NativeRemote > Input")),
            KeyBinding::new("shift-tab", KeepFocus, Some("NativeRemote > Input")),
            KeyBinding::new("escape", Cancel, Some("NativeRemote > Input")),
        ]);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .accessibility_label("SSH Connection String")
                .placeholder("ssh user@example -p 1234")
        });
        input.update(cx, |i, cx| {
            i.set_protect_ime_keys(true);
            i.set_editor_style(support::input_style(dark));
            i.set_value(value, window, cx);
        });
        let events = cx.subscribe_in(&input, window, |s, input, event: &InputEvent, w, cx| {
            if s.busy {
                return;
            }
            match event {
                InputEvent::Change => {
                    s.cursor = 0;
                    cx.emit(UiEvent::Changed(input.read(cx).value().to_string()));
                }
                InputEvent::PressEnter { secondary, .. } => {
                    if !s.can_act(w, cx) {
                        return;
                    }
                    if *secondary && s.directory {
                        if let Some(row) = s.rows.iter().find(|r| r.select).cloned() {
                            cx.emit(UiEvent::Choose(row));
                        }
                    } else {
                        s.submit(cx);
                    }
                }
                _ => {}
            }
        });
        let input_focus = input.read(cx).focus_handle(cx);
        let focus_events = vec![
            cx.on_focus(&input_focus, window, |_, _, cx| cx.notify()),
            cx.on_blur(&input_focus, window, |_, _, cx| cx.notify()),
        ];
        input_focus.focus(window, cx);
        Self {
            input,
            focus: cx.focus_handle(),
            directory: false,
            busy: false,
            dark,
            error: String::new(),
            status: "",
            status_revision: None,
            progress_visible: false,
            progress_task: None,
            path: String::new(),
            rows: vec![],
            cursor: 0,
            scroll: ScrollHandle::new(),
            _input_events: events,
            _focus_events: focus_events,
        }
    }
    pub(super) fn editor(&self) -> Entity<InputState> {
        self.input.clone()
    }

    fn set_status(&mut self, status: &'static str, revision: Option<u64>, cx: &mut Context<Self>) {
        if self.status == status && self.status_revision == revision {
            return;
        }
        // Owning the task cancels it on clear, replacement, or view drop.
        self.progress_task = None;
        self.progress_visible = false;
        self.status = status;
        self.status_revision = revision;
        if !status.is_empty() {
            self.progress_task = Some(cx.spawn(async move |weak, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(180))
                    .await;
                let _ = weak.update(cx, |view, cx| {
                    if view.status_revision == revision && view.status == status {
                        view.progress_visible = true;
                        cx.notify();
                    }
                });
            }));
        }
    }

    fn can_act(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        !self.busy
            && !self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
    }
    fn move_row(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_act(window, cx) || !self.directory {
            return;
        }
        self.cursor = if down {
            (self.cursor + 1).min(self.rows.len().saturating_sub(1))
        } else {
            self.cursor.saturating_sub(1)
        };
        self.scroll.scroll_to_item(self.cursor);
        cx.notify();
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.directory {
            if let Some(row) = self.rows.get(self.cursor).cloned() {
                cx.emit(UiEvent::Choose(row));
            }
        } else {
            cx.emit(UiEvent::Submit);
        }
    }
}
impl Render for RemoteDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let error = !self.error.is_empty();
        let busy = self.busy;
        let field_color = if error { t.danger } else { t.accent };
        let width = (window.viewport_size().width - px(36.))
            .max(px(0.))
            .min(px(if self.directory { 480. } else { 440. }));
        let mut body = div()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(if self.directory { 8. } else { 6. }))
            .child(
                InputBase::new("remote-field")
                    .key_context("NativeRemote")
                    .w_full()
                    .h(px(28.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgb(if error {
                        t.danger
                    } else if focused {
                        t.accent
                    } else {
                        t.border
                    }))
                    .bg(rgb(t.canvas))
                    .hover(move |field| {
                        if error {
                            field
                                .border_color(rgb(t.danger))
                                .bg(remote_mix(t.danger, t.canvas, 0.05))
                        } else if !busy && !focused {
                            field.border_color(rgb(t.hover_border))
                        } else {
                            field
                        }
                    })
                    .when(focused, |field| {
                        field.shadow(vec![
                            gpui_kit::BoxShadow::new(
                                px(0.),
                                px(0.),
                                gpui_kit::Rgba {
                                    a: if error { 0.20 } else { 0.22 },
                                    ..rgb(field_color)
                                }
                                .into(),
                            )
                            .spread_radius(px(2.)),
                        ])
                    })
                    .px(px(8.))
                    .pb(px(2.))
                    .flex()
                    .items_center()
                    .font_family(support::mono())
                    .text_size(px(11.))
                    .child(Input::new(&self.input)),
            )
            .when(!self.error.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(t.danger))
                        .child(self.error.clone()),
                )
            })
            .when(self.progress_visible, |d| {
                d.child(
                    div()
                        .id("remote-status")
                        .test_support()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(11.))
                        .text_color(rgb(t.muted))
                        .child(remote_spinner(t.muted, cx.reduce_motion()))
                        .child(self.status),
                )
            });
        if self.directory {
            let mut list = div()
                .id("remote-directory-list")
                .max_h(
                    (window.viewport_size().height - px(150.))
                        .max(px(0.))
                        .min(px(360.)),
                )
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .flex()
                .flex_col();
            for (ix, row) in self.rows.iter().cloned().enumerate() {
                list = list.child(
                    div()
                        .id(format!("remote-option-{ix}"))
                        .test_support()
                        .relative()
                        .h(px(28.))
                        .rounded(px(6.))
                        .when(!self.busy, |d| d.cursor_pointer())
                        .hover(move |d| d.bg(rgb(t.hover)))
                        .tooltip({
                            let path = row.path.clone();
                            let dark = self.dark;
                            move |_, cx| {
                                cx.new(|_| DirectoryPathHint {
                                    path: path.clone(),
                                    dark,
                                })
                                .into()
                            }
                        })
                        .flex_shrink_0()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(rgb(if self.busy { t.muted } else { t.text }))
                        .when(ix == self.cursor, |d| d.bg(rgb(t.hover)))
                        .when(ix == self.cursor, |d| {
                            d.child(
                                div()
                                    .id(format!("remote-cursor-rail-{ix}"))
                                    .test_support()
                                    .absolute()
                                    .top(px(3.))
                                    .bottom(px(3.))
                                    .left_0()
                                    .w(px(2.))
                                    .rounded_r(px(1.))
                                    .bg(rgb(t.accent)),
                            )
                        })
                        .child(row.label.clone())
                        .on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
                            window.prevent_default()
                        })
                        .on_hover(cx.listener(move |s, hover, _, cx| {
                            if *hover && !s.busy {
                                s.cursor = ix;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |s, _, _, cx| {
                            if !s.busy {
                                cx.emit(UiEvent::Choose(row.clone()));
                            }
                        })),
                );
            }
            body = body.child(list);
        }
        if !self.error.is_empty() {
            body = body.child(
                support::button_sm(
                    "remote-retry",
                    "Retry",
                    support::ButtonVariant::Secondary,
                    self.dark,
                )
                .disabled(self.busy)
                .on_click(cx.listener(|s, _, _, cx| {
                    if !s.busy {
                        cx.emit(UiEvent::Retry);
                    }
                })),
            );
        }
        let panel = div()
            .id("remote-dialog")
            .test_support()
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                gpui_kit::BoxShadow::new(
                    px(0.),
                    px(14.),
                    rgba(if self.dark { 0x00000099 } else { 0x00000033 }).into(),
                )
                .blur_radius(px(40.)),
            ])
            .max_h((window.viewport_size().height - px(120.)).max(px(0.)))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_action(cx.listener(|s, _: &NextRow, w, cx| s.move_row(true, w, cx)))
            .on_action(cx.listener(|s, _: &PreviousRow, w, cx| s.move_row(false, w, cx)))
            .on_action(cx.listener(|s, _: &Submit, w, cx| {
                if s.can_act(w, cx) {
                    s.submit(cx);
                }
            }))
            .on_action(cx.listener(|s, _: &SelectCurrent, w, cx| {
                if s.can_act(w, cx) {
                    if s.directory {
                        if let Some(row) = s.rows.iter().find(|r| r.select).cloned() {
                            cx.emit(UiEvent::Choose(row));
                        }
                    } else {
                        s.submit(cx);
                    }
                }
            }))
            .on_action(cx.listener(|s, _: &ParentDirectory, w, cx| {
                if s.can_act(w, cx)
                    && s.directory
                    && let Some(row) = s.rows.iter().find(|r| r.back).cloned()
                {
                    cx.emit(UiEvent::Choose(row));
                }
            }))
            .on_action(cx.listener(|s, _: &KeepFocus, w, cx| {
                s.input.read(cx).focus_handle(cx).focus(w, cx);
            }))
            .on_action(cx.listener(|s, _: &Cancel, w, cx| {
                if s.can_act(w, cx) {
                    cx.emit(UiEvent::Cancel);
                }
            }))
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .text_size(px(12.))
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .child(if self.directory {
                        "Choose Remote Working Directory"
                    } else {
                        "SSH Connection"
                    }),
            )
            .child(body);
        let weak = cx.entity().downgrade();
        support::root(self.dark)
            .absolute()
            .top_0()
            .left_0()
            .bg(rgba(0))
            .child(
                Dialog::new(cx)
                    .focus_handle(self.focus.clone())
                    .close_on_escape(!self.busy)
                    .close_on_backdrop_press(!self.busy)
                    .on_cancel(move |_, _, cx| {
                        let _ = weak.update(cx, |s, cx| {
                            if !s.busy {
                                cx.emit(UiEvent::Cancel);
                            }
                        });
                        false
                    })
                    .backdrop(
                        DialogBackdrop::new()
                            .absolute()
                            .size_full()
                            .bg(rgba(t.scrim)),
                    )
                    .popup(
                        DialogPopup::new()
                            .absolute()
                            .top(px(68.))
                            .left((window.viewport_size().width - width) / 2.)
                            .w(width)
                            .child(panel),
                    ),
            )
    }
}

fn remote_mix(a: u32, b: u32, weight: f32) -> gpui_kit::Rgba {
    let a = rgb(a);
    let b = rgb(b);
    gpui_kit::Rgba {
        r: a.r * weight + b.r * (1. - weight),
        g: a.g * weight + b.g * (1. - weight),
        b: a.b * weight + b.b * (1. - weight),
        a: 1.,
    }
}

struct DirectoryPathHint {
    path: String,
    dark: bool,
}
impl Render for DirectoryPathHint {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        div()
            .id("remote-path-tooltip")
            .test_support()
            .max_w(
                (window.viewport_size().width - px(16.))
                    .max(px(0.))
                    .min(px(500.)),
            )
            .px(px(7.))
            .py(px(3.))
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .text_color(rgb(t.text))
            .text_size(px(10.))
            .child(self.path.clone())
    }
}

fn remote_spinner(color: u32, reduced_motion: bool) -> gpui_kit::AnyElement {
    use gpui_kit::{Animation, AnimationExt};
    let spinner = div()
        .id("remote-spinner")
        .relative()
        .size(px(8.))
        .flex_shrink_0();
    let paint = move |d: gpui_kit::Stateful<gpui_kit::Div>, phase: f32| {
        let step = (phase * 8.).floor() as usize;
        d.children(
            [
                (0., 0.),
                (3., 0.),
                (6., 0.),
                (6., 3.),
                (6., 6.),
                (3., 6.),
                (0., 6.),
                (0., 3.),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (x, y))| {
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .size(px(2.))
                    .bg(rgb(color))
                    .opacity(if reduced_motion {
                        0.45
                    } else {
                        [1., 0.72, 0.5, 0.34, 0.24, 0.17, 0.12, 0.08][(step + 7 - i) % 8]
                    })
            }),
        )
    };
    if reduced_motion {
        paint(spinner, 0.).into_any_element()
    } else {
        spinner
            .with_animation(
                "remote-spinner-clock",
                Animation::new(std::time::Duration::from_millis(640)).repeat(),
                paint,
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "remote_parity_tests.rs"]
mod parity_tests;

#[cfg(test)]
#[path = "tests/service_remote.rs"]
mod regressions;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_shell_fake_commands_cover_probe_browse_retry_and_import() {
        super::super::tests::with_shell(32, |s, w, cx, commands| {
            s.window = Some(w.window_handle());
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![],
                active_project_path: String::new(),
                pi_path: None,
            });
            s.open_remote(w, cx);
            s.remote.connection = "ssh fixture".into();
            s.remote_request(RemoteOperation::Probe, String::new(), cx);
            s.remote_request(RemoteOperation::Probe, String::new(), cx);
            let Command::Remote(probe) = commands.try_recv().unwrap() else {
                panic!("probe");
            };
            assert!(commands.try_recv().is_err());
            s.remote_event(
                RemoteEvent {
                    command: probe,
                    result: Ok(RemoteResult::Listing(RemoteDirectoryListing {
                        connection_string: "ssh fixture".into(),
                        working_directory: "/home/me".into(),
                        host: "fixture".into(),
                        directories: vec![],
                    })),
                },
                cx,
            );
            s.sync_remote(w, cx);
            assert!(s.remote.view.as_ref().unwrap().read(cx).directory);
            s.remote.filter = "draft filter".into();
            s.remote_request(RemoteOperation::List, "/home".into(), cx);
            let Command::Remote(list) = commands.try_recv().unwrap() else {
                panic!("list");
            };
            s.remote_failed(list.clone(), cx);
            assert_eq!(s.remote.filter, "draft filter");
            assert_eq!(
                s.remote.listing.as_ref().unwrap().working_directory,
                "/home/me"
            );
            assert!(!s.remote.error.is_empty());
            s.remote_request(RemoteOperation::Import, "/home/me".into(), cx);
            let Command::Remote(import) = commands.try_recv().unwrap() else {
                panic!("import");
            };
            s.remote_request(RemoteOperation::Import, "/home/me".into(), cx);
            assert!(commands.try_recv().is_err());
            s.remote_event(
                RemoteEvent {
                    command: list,
                    result: Err(()),
                },
                cx,
            );
            assert!(s.remote.is_busy());
            s.remote_event(
                RemoteEvent {
                    command: import,
                    result: Ok(RemoteResult::Imported(super::super::tests::project())),
                },
                cx,
            );
            assert!(!s.remote.is_open());
            assert!(!s.remote.is_busy());
            assert!(s.workspace.is_some());
            assert!(s.error.is_none());
        });
    }
    #[test]
    fn directory_keyboard_filter_and_select_current_use_retained_root_controls() {
        use std::{cell::RefCell, rc::Rc};
        let mut app = super::super::tests::app();
        let mut dialog = None;
        let mut window = app.open_window(|w, cx| {
            let view = cx.new(|cx| RemoteDialog::new(String::new(), false, w, cx));
            dialog = Some(view.clone());
            Root::new(view, w, cx)
        });
        let dialog = dialog.unwrap();
        let events = Rc::new(RefCell::new(Vec::<UiEvent>::new()));
        let received = events.clone();
        let _subscription = window.update(|_, _, cx| {
            dialog.update(cx, |s, cx| {
                s.directory = true;
                s.rows = vec![
                    OptionRow {
                        label: "Go Back".into(),
                        path: "/".into(),
                        select: false,
                        back: true,
                    },
                    OptionRow {
                        label: "Select /home".into(),
                        path: "/home".into(),
                        select: true,
                        back: false,
                    },
                    OptionRow {
                        label: "Alpha".into(),
                        path: "/home/Alpha".into(),
                        select: false,
                        back: false,
                    },
                ];
                cx.subscribe(&cx.entity(), move |_, _, event: &UiEvent, _| {
                    received.borrow_mut().push(event.clone())
                })
            })
        });
        window.draw();
        window.simulate_keystroke("down");
        window.simulate_keystroke("down");
        window.simulate_keystroke("enter");
        assert!(
            matches!(events.borrow().last(), Some(UiEvent::Choose(row)) if row.path == "/home/Alpha")
        );
        window.simulate_keystroke("cmd-enter");
        assert!(matches!(events.borrow().last(), Some(UiEvent::Choose(row)) if row.select));
        window.simulate_keystroke("cmd-up");
        assert!(matches!(events.borrow().last(), Some(UiEvent::Choose(row)) if row.path == "/"));
        window.update(|_, _, cx| dialog.update(cx, |s, _| s.busy = true));
        window.draw();
        let count = events.borrow().len();
        window.simulate_keystroke("enter");
        window.simulate_keystroke("escape");
        assert_eq!(events.borrow().len(), count);
    }
    #[test]
    fn close_and_window_remount_keep_connection_and_filter_drafts() {
        super::super::tests::with_shell(16, |s, w, cx, _| {
            s.remote.connection = "ssh retained".into();
            s.open_remote(w, cx);
            s.remote.open = false;
            s.remote.view = None;
            s.open_remote(w, cx);
            assert_eq!(
                s.remote
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .input
                    .read(cx)
                    .value()
                    .as_ref(),
                "ssh retained"
            );
            s.mount_remote(w, cx);
            assert_eq!(s.remote.connection, "ssh retained");
        });
    }
    #[test]
    fn stale_operation_connection_path_and_cancel_are_fenced() {
        let command = RemoteCommand {
            revision: 2,
            operation: RemoteOperation::List,
            connection: "ssh fixture".into(),
            path: "/home".into(),
            host: "fixture".into(),
        };
        let mut state = State {
            open: true,
            pending: Some(command.clone()),
            ..State::default()
        };
        assert!(state.accepts(&command));
        for changed in [
            RemoteCommand {
                revision: 1,
                ..command.clone()
            },
            RemoteCommand {
                operation: RemoteOperation::Import,
                ..command.clone()
            },
            RemoteCommand {
                path: "/other".into(),
                ..command.clone()
            },
            RemoteCommand {
                connection: "ssh other".into(),
                ..command.clone()
            },
        ] {
            assert!(!state.accepts(&changed));
        }
        state.open = false;
        assert!(!state.accepts(&command));
    }
    #[test]
    fn filter_keeps_parent_and_select_and_root_has_no_parent() {
        let mut state = State {
            filter: "ALP".into(),
            listing: Some(RemoteDirectoryListing {
                connection_string: "ssh fixture".into(),
                host: "fixture".into(),
                working_directory: "/home/me".into(),
                directories: vec![tau_core::models::RemoteDirectoryEntry {
                    name: "Alpha".into(),
                    path: "/home/me/Alpha".into(),
                }],
            }),
            ..State::default()
        };
        let rows = state.options();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].path, "/home");
        assert_eq!(rows[0].label, "Go Back");
        assert!(rows[0].back);
        assert_eq!(rows[1].label, "Select /home/me");
        assert!(rows[1].select);
        assert!(!rows[1].back);
        assert!(!rows[2].back);
        state.listing.as_mut().unwrap().working_directory = "/".into();
        let rows = state.options();
        assert!(rows[0].select);
        assert_eq!(rows[0].label, "Select /");
        assert!(!rows.iter().any(|row| row.back));
    }
}

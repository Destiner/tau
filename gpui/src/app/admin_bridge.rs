//! Production Shell hooks for the single shared admin gate and retained reporter.
use super::*;
use crate::components::admin::Event as ReportEvent;
use gpui_kit::{Focusable, KeyDownEvent};

impl Shell {
    pub(super) fn mount_admin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let command = self
            .admin
            .mount(self.dark, window, cx, |s, event, w, cx| match event {
                ReportEvent::Submit {
                    description,
                    include_session,
                } => {
                    let id = s.admin_session_id();
                    let command = if s.admin_blocked(cx) {
                        None
                    } else {
                        s.admin.submit(description.clone(), *include_session, id)
                    };
                    if let Some(command) = command {
                        s.dispatch_admin(command, cx);
                    } else if let Some(view) = &s.admin.view {
                        view.update(cx, |view, cx| view.complete(false, w, cx));
                    }
                }
                ReportEvent::Dismissed => {
                    if !s.admin_blocked(cx) {
                        s.restore_input_focus(cx);
                    }
                }
            });
        self.dispatch_admin(command, cx);
    }

    fn admin_session_id(&self) -> Option<String> {
        self.sessions
            .get(&self.active)
            .map(|live| live.controller.state().identity.id.clone())
            .filter(|id| !id.is_empty())
    }

    pub(super) fn admin_blocked(&self, cx: &App) -> bool {
        self.remote.is_open()
            || self.palette.is_open(cx)
            || self.lifecycle.is_some()
            || self.confirmation.is_some()
            || self.error.is_some()
            || self.picker_busy
            || self
                .sessions
                .get(&self.active)
                .is_some_and(|live| live.extension.is_some())
    }

    pub(super) fn admin_reporter_open(&self, cx: &App) -> bool {
        self.admin.enabled()
            && !self.admin_blocked(cx)
            && self
                .admin
                .view
                .as_ref()
                .is_some_and(|view| view.read(cx).is_open())
    }

    pub(super) fn sync_admin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let blocked = self.admin_blocked(cx);
        let has_session = self.admin_session_id().is_some();
        self.admin.sync(self.dark, has_session, blocked, window, cx);
        let visible = self.admin.enabled() && self.admin.view.is_some();
        if self.admin.sidebar_control != Some(visible)
            && let Some(sidebar) = &self.sidebar
        {
            let control = if visible {
                self.admin.view.clone().map(Into::into)
            } else {
                None
            };
            sidebar.update(cx, |sidebar, cx| sidebar.set_admin_control(control, cx));
            self.admin.sidebar_control = Some(visible);
        }
    }

    pub(super) fn admin_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_open(cx) {
            return;
        }
        let editable = self.header.as_ref().is_some_and(|h| {
            h.read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }) || self.remote.view.as_ref().is_some_and(|v| {
            v.read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }) || self.palette.view.as_ref().is_some_and(|v| {
            v.read(cx)
                .search_editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }) || self.admin.view.as_ref().is_some_and(|v| {
            v.read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }) || self.confirmation.as_ref().is_some_and(|v| {
            v.read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }) || self.sessions.values().any(|live| {
            let composer = live.composer.read(cx);
            composer
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
                || composer
                    .search_editor()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
                || live.extension.as_ref().is_some_and(|(_, v)| {
                    v.read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                })
        });
        if let Some(command) = self
            .admin
            .key(event, self.epoch.elapsed(), editable, window)
        {
            self.dispatch_admin(command, cx);
            self.sync_admin(window, cx);
            cx.notify();
        }
    }

    pub(super) fn dispatch_admin(&mut self, command: admin::Command, cx: &mut Context<Self>) {
        // A queued write has no safe retry deadline: only its reply or worker
        // closure can settle it. Retain only a content-free token while waiting.
        let (revision, token) = match &command {
            admin::Command::Load { revision } => (
                *revision,
                admin::Command::Load {
                    revision: *revision,
                },
            ),
            admin::Command::SetMode { revision, enabled } => (
                *revision,
                admin::Command::SetMode {
                    revision: *revision,
                    enabled: *enabled,
                },
            ),
            admin::Command::Submit { revision, .. } => (
                *revision,
                admin::Command::Submit {
                    revision: *revision,
                    description: String::new(),
                    session_id: None,
                },
            ),
        };
        self.dispatch(Command::Admin(command), cx);
        let commands = self.commands.clone();
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let done = weak
                    .update(cx, |s, cx| {
                        if !s.admin.pending(revision) {
                            return true;
                        }
                        if commands.is_closed() {
                            s.admin_failed(token.clone(), cx);
                            return true;
                        }
                        false
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn admin_failed(&mut self, command: admin::Command, cx: &mut Context<Self>) {
        let accepted = self.admin.failed(command);
        if accepted && self.admin.mode_failed() {
            self.error = Some(admin::MODE_ERROR.into());
        }
        self.refresh_window(cx);
    }

    pub(super) fn admin_event(&mut self, reply: admin::Reply, cx: &mut Context<Self>) {
        let accepted = self.admin.accept(reply);
        if accepted && self.admin.mode_failed() {
            self.error = Some(admin::MODE_ERROR.into());
        }
        self.refresh_window(cx);
    }

    pub(super) fn open_admin_reporter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_admin(window, cx);
        if !self.admin_blocked(cx) {
            self.admin.open_reporter(window, cx);
            cx.notify();
        }
    }
}

#[cfg(test)]
#[path = "admin_bridge_tests.rs"]
mod tests;

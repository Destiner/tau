//! Window-owned admin UI and a content-free, revisioned worker boundary.
use std::time::Duration;

use crate::components::admin::{AdminControls, Event as UiEvent};
use gpui_kit::{App, AppContext, Context, Entity, KeyDownEvent, Subscription, Window};

pub const MODE_ERROR: &str = "Admin mode could not be saved. The displayed state reflects this run only. Retry the admin action to save your preference.";

/// Do not log these commands: Submit intentionally contains user-provided content.
#[derive(Clone)]
pub enum Command {
    Load {
        revision: u64,
    },
    SetMode {
        revision: u64,
        enabled: bool,
    },
    Submit {
        revision: u64,
        description: String,
        session_id: Option<String>,
    },
}

/// Backend failures cross this boundary as reviewed outcomes, never raw errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Loaded {
        revision: u64,
        enabled: bool,
    },
    ModeSet {
        revision: u64,
        enabled: bool,
        saved: bool,
    },
    Submitted {
        revision: u64,
        saved: bool,
    },
}

#[derive(Default)]
pub struct Matcher {
    matched: usize,
    last_press: Duration,
}
impl Matcher {
    /// Vue does not filter repeats or composition events. Their single-character keys
    /// count outside inputs; inside Base inputs *all* keys reset the partial sequence.
    pub fn press(&mut self, event: &KeyDownEvent, now: Duration, editable: bool) -> bool {
        let modifiers = event.keystroke.modifiers;
        let key = &event.keystroke.key;
        if editable
            || modifiers.platform
            || modifiers.control
            || modifiers.alt
            || key.encode_utf16().count() != 1
        {
            self.matched = 0;
            return false;
        }
        if self.matched > 0 && now.saturating_sub(self.last_press) > Duration::from_secs(2) {
            self.matched = 0;
        }
        self.last_press = now;
        let key = key.to_lowercase();
        let code = b"iddqd";
        self.matched = if key.as_bytes() == &code[self.matched..self.matched + 1] {
            self.matched + 1
        } else {
            usize::from(key == "i")
        };
        if self.matched == code.len() {
            self.matched = 0;
            true
        } else {
            false
        }
    }
}

/// Reads the focused dispatch path, not the capture listener's own context.
/// Every Base Input/Textarea registers `Input`, including readonly editors.
/// `actual_editor_focused` covers a newly focused Base editor before its next paint.
pub fn focused_input(window: &Window, actual_editor_focused: bool) -> bool {
    actual_editor_focused
        || window
            .context_stack()
            .iter()
            .any(|context| context.contains("Input"))
}

#[derive(Default)]
pub struct State {
    pub view: Option<Entity<AdminControls>>,
    subscription: Option<Subscription>,
    matcher: Matcher,
    revision: u64,
    mode_request: Option<(u64, bool)>,
    load_request: Option<u64>,
    submission: Option<u64>,
    submission_view: Option<Entity<AdminControls>>,
    enabled: bool,
    backend_enabled: bool,
    backend_revision: u64,
    desired: bool,
    mode_failed: bool,
    completion: Option<(Option<Entity<AdminControls>>, bool)>,
    pub(super) sidebar_control: Option<bool>,
}
impl State {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn mode_failed(&self) -> bool {
        self.mode_failed
    }

    fn next_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }

    /// Parent retains the subscription here; replacing a mounted view drops the old one.
    pub fn mount<T: 'static>(
        &mut self,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<T>,
        on_event: impl Fn(&mut T, &UiEvent, &mut Window, &mut Context<T>) + 'static,
    ) -> Command {
        self.subscription = None;
        // Accepted writes outlive a mount; their acknowledgement belongs to the
        // originating control, never to a replacement control's draft.
        self.sidebar_control = None;
        self.mode_request = None;
        self.mode_failed = false;
        self.matcher = Matcher::default();
        let view = cx.new(|cx| AdminControls::new(dark, window, cx));
        self.subscription = Some(
            cx.subscribe_in(&view, window, move |shell, _, event, w, cx| {
                on_event(shell, event, w, cx);
            }),
        );
        self.view = Some(view);
        let revision = self.next_revision();
        self.load_request = Some(revision);
        Command::Load { revision }
    }

    /// Pass the *current* identity availability and top-modal priority on every projection.
    /// No session ID is retained by either this state or the view.
    pub fn sync(
        &mut self,
        dark: bool,
        has_session: bool,
        blocked: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(view) = &self.view {
            view.update(cx, |view, cx| {
                view.set_theme(dark, window, cx);
                view.set_enabled(self.enabled, window, cx);
                view.set_session_available(has_session, cx);
                view.set_suspended(blocked, cx);
            });
        }
        self.sync_view_completion(window, cx);
    }

    /// The root capture listener must not stop propagation or prevent character input.
    /// No matcher bindings are installed: handled Base actions need not reach this hook.
    pub fn key(
        &mut self,
        event: &KeyDownEvent,
        now: Duration,
        actual_editor_focused: bool,
        window: &Window,
    ) -> Option<Command> {
        if self
            .matcher
            .press(event, now, focused_input(window, actual_editor_focused))
        {
            Some(self.set_mode(!self.desired))
        } else {
            None
        }
    }

    /// Off hides immediately; replies reconcile the actual backend gate even when
    /// persistence fails. A live switch is not an acknowledgement of durability.
    pub fn set_mode(&mut self, enabled: bool) -> Command {
        let revision = self.next_revision();
        self.load_request = None;
        self.mode_request = Some((revision, enabled));
        self.mode_failed = false;
        self.desired = enabled;
        if !enabled {
            self.enabled = false;
        }
        Command::SetMode { revision, enabled }
    }

    /// Called synchronously for UiEvent::Submit. Capture the controller identity *here*;
    /// never use the sidebar key, a cached popup ID, session path, or transcript contents.
    pub fn submit(
        &mut self,
        description: String,
        include_session: bool,
        current_id: Option<String>,
    ) -> Option<Command> {
        if !self.enabled
            || self.submission.is_some()
            || self.completion.is_some()
            || description.trim().is_empty()
            || description.encode_utf16().count() > 10_000
        {
            return None;
        }
        let revision = self.next_revision();
        self.submission = Some(revision);
        self.submission_view = self.view.clone();
        Some(Command::Submit {
            revision,
            description: description.trim().to_owned(),
            session_id: if include_session {
                current_id.filter(|id| !id.is_empty())
            } else {
                None
            },
        })
    }

    /// Returns whether a current mode reply was accepted. Parent can surface
    /// `mode_failed()` through its existing operation-failure confirmation, not raw errors.
    #[cfg(test)]
    pub fn reply(&mut self, reply: Reply, window: &mut Window, cx: &mut App) -> bool {
        let accepted = self.accept(reply);
        self.sync_view_completion(window, cx);
        accepted
    }

    fn sync_view_completion(&mut self, window: &mut Window, cx: &mut App) {
        if let Some((Some(view), saved)) = self.completion.take() {
            view.update(cx, |view, cx| view.complete(saved, window, cx));
        }
    }

    /// No Window is required: service replies must settle even with the window closed.
    pub fn accept(&mut self, reply: Reply) -> bool {
        if let Reply::ModeSet {
            revision, enabled, ..
        } = &reply
            && *revision <= self.revision
            && *revision > self.backend_revision
        {
            self.backend_revision = *revision;
            self.backend_enabled = *enabled;
            // A superseded command can still have changed the live backend. If the
            // newer command failed to enqueue, its failure was not a backend rollback.
            if self.mode_request.is_none() && self.load_request.is_none() {
                self.enabled = *enabled;
                self.desired = *enabled;
            }
        }
        match reply {
            Reply::Loaded { revision, enabled } if self.load_request == Some(revision) => {
                self.load_request = None;
                self.mode_failed = false;
                self.enabled = enabled;
                self.backend_enabled = enabled;
                self.backend_revision = revision;
                self.desired = enabled;
                true
            }
            Reply::ModeSet {
                revision,
                enabled,
                saved,
            } if self
                .mode_request
                .is_some_and(|(pending, _)| pending == revision) =>
            {
                self.mode_request = None;
                self.mode_failed = !saved;
                self.enabled = enabled;
                self.backend_enabled = enabled;
                self.desired = enabled;
                true
            }
            Reply::Submitted { revision, saved } if self.submission == Some(revision) => {
                self.submission = None;
                self.completion = Some((self.submission_view.take(), saved));
                false
            }
            _ => false,
        }
    }

    pub fn pending(&self, revision: u64) -> bool {
        self.load_request == Some(revision)
            || self
                .mode_request
                .is_some_and(|(pending, _)| pending == revision)
            || self.submission == Some(revision)
    }

    pub fn failed(&mut self, command: Command) -> bool {
        let reply = match command {
            Command::Load { revision } => Reply::Loaded {
                revision,
                enabled: self.backend_enabled,
            },
            Command::SetMode { revision, .. } => {
                if !self
                    .mode_request
                    .is_some_and(|(pending, _)| pending == revision)
                {
                    return false;
                }
                self.mode_request = None;
                self.mode_failed = true;
                self.enabled = self.backend_enabled;
                self.desired = self.backend_enabled;
                return true;
            }
            Command::Submit { revision, .. } => Reply::Submitted {
                revision,
                saved: false,
            },
        };
        self.accept(reply)
    }

    pub fn open_reporter(&self, window: &mut Window, cx: &mut App) {
        if let Some(view) = &self.view {
            view.update(cx, |view, cx| view.open_reporter(window, cx));
        }
    }
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;

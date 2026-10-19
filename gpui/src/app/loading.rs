use super::*;
use std::time::Duration;

#[derive(Clone, PartialEq, Eq)]
struct Binding {
    runtime: String,
    attempt: u64,
    id: String,
    path: String,
}

#[derive(Default)]
pub(super) struct State {
    binding: Option<Binding>,
    visible: bool,
    task: Option<gpui_kit::Task<()>>,
}

fn needs_loading(live: &LiveSession) -> bool {
    let state = live.controller.state();
    !live.ephemeral
        && state.transcript.is_empty()
        && state.error.is_none()
        && !live
            .presented_history
            .as_ref()
            .is_some_and(|(id, path)| id == &state.identity.id && path == &state.identity.path)
        && (live.starting || !state.ready || state.history_loading)
}

impl Shell {
    pub(super) fn active_session_loading(&self) -> bool {
        self.sessions.get(&self.active).is_some_and(needs_loading)
    }

    fn loading_binding(&self) -> Option<Binding> {
        self.sessions
            .get(&self.active)
            .filter(|live| needs_loading(live))
            .map(|live| {
                let state = live.controller.state();
                Binding {
                    runtime: self.active.clone(),
                    attempt: live.start_attempt,
                    id: state.identity.id.clone(),
                    path: state.identity.path.clone(),
                }
            })
    }

    pub(super) fn loading_indicator_visible(&self) -> bool {
        self.session_loading.visible && self.session_loading.binding == self.loading_binding()
    }

    pub(super) fn sync_session_loading(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(live) = self.sessions.get_mut(&self.active) {
            let state = live.controller.state();
            if !live.starting
                && state.ready
                && !state.history_loading
                && state.error.is_none()
                && !live.presented_history.as_ref().is_some_and(|(id, path)| {
                    id == &state.identity.id && path == &state.identity.path
                })
            {
                live.presented_history =
                    Some((state.identity.id.clone(), state.identity.path.clone()));
            }
        }
        let binding = self.loading_binding();
        if binding == self.session_loading.binding {
            return false;
        }
        let finished = self.session_loading.binding.is_some() && binding.is_none();
        self.session_loading.task = None;
        self.session_loading.visible = false;
        self.session_loading.binding = binding.clone();
        if let Some(binding) = binding {
            self.session_loading.task = Some(cx.spawn(async move |owner, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let _ = owner.update(cx, |s, cx| {
                    if s.session_loading.binding.as_ref() == Some(&binding)
                        && s.loading_binding().as_ref() == Some(&binding)
                    {
                        s.session_loading.visible = true;
                        s.refresh_window(cx);
                        cx.notify();
                    }
                });
            }));
        }
        finished
    }
}

#[cfg(test)]
#[path = "loading_tests.rs"]
mod tests;

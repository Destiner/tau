//! Shell-owned recovery is a draft edit, never a prompt or queue RPC.
use super::*;
use gpui_kit::Focusable;
use std::cell::Cell;

pub(super) struct Recovery {
    key: String,
    queue: Entity<MessageQueue>,
    generation: u64,
    index: usize,
    owned: Vec<String>,
    edited: Rc<Cell<bool>>,
    _changes: Subscription,
}

impl Shell {
    pub(super) fn can_recover_queue(&self, key: &str, cx: &App) -> bool {
        self.active == key
            && self.loaded
            && !self
                .fallback
                .as_ref()
                .is_some_and(|(_, dialog)| dialog.read(cx).is_open())
            && self.lifecycle.is_none()
            && self.exit_revision.is_none()
            && self.confirmation.is_none()
            && self.error.is_none()
            && !self.remote.is_open()
            && !self.picker_busy
            && !self.preview_open()
            && !self.palette.is_open(cx)
            && !self.admin_reporter_open(cx)
            && self.sessions.get(key).is_some_and(|live| {
                live.extension.is_none()
                    && live.controller.state().extension_dialogs.is_empty()
                    && live.controller.state().draft.is_empty()
                    && live.shown_draft.is_empty()
                    && live.composer.read(cx).editor().read(cx).value().is_empty()
            })
    }

    /// Capture authoritative owned text, not text supplied by a renderer/callback.
    pub(super) fn queue_recovery(
        &self,
        key: &str,
        queue: &Entity<MessageQueue>,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<Recovery> {
        if !self.can_recover_queue(key, cx) {
            return None;
        }
        let live = self.sessions.get(key)?;
        if &live.queue != queue {
            return None;
        }
        let owned: Vec<_> = live
            .restored_recovered
            .iter()
            .chain(&live.controller.state().recovered_drafts)
            .cloned()
            .collect();
        if owned.get(index)?.is_empty() || queue.read(cx).props().failed != owned {
            return None;
        }
        let edited = Rc::new(Cell::new(false));
        let changed = edited.clone();
        let editor = live.composer.read(cx).editor();
        let changes = cx.subscribe(&editor, move |_, _, event: &InputEvent, _| {
            if matches!(event, InputEvent::Change) {
                changed.set(true);
            }
        });
        Some(Recovery {
            key: key.into(),
            queue: queue.clone(),
            generation: live.controller.generation(),
            index,
            owned,
            edited,
            _changes: changes,
        })
    }

    /// Commit synchronously; if a caller defers, recheck draft, runtime and ownership.
    pub(super) fn restore_queue_draft(
        &mut self,
        recovery: Recovery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if recovery.edited.get() || !self.can_recover_queue(&recovery.key, cx) {
            return false;
        }
        let Some(live) = self.sessions.get_mut(&recovery.key) else {
            return false;
        };
        let owned: Vec<_> = live
            .restored_recovered
            .iter()
            .chain(&live.controller.state().recovered_drafts)
            .cloned()
            .collect();
        if live.queue != recovery.queue
            || live.controller.generation() != recovery.generation
            || owned != recovery.owned
        {
            return false;
        }
        let Some(text) = owned.get(recovery.index).filter(|s| !s.is_empty()).cloned() else {
            return false;
        };
        if recovery.index < live.restored_recovered.len() {
            live.restored_recovered.remove(recovery.index);
        } else {
            let index = recovery.index - live.restored_recovered.len();
            if !live.controller.restore_recovered_draft(index, &text) {
                return false;
            }
        }
        live.controller.set_draft(text.clone());
        live.shown_draft = text.clone();
        live.dirty = true;
        let editor = live.composer.read(cx).editor();
        editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        editor.read(cx).focus_handle(cx).focus(window, cx);
        self.sidebar_state.invalidate_projection();
        self.persist(false, cx);
        self.refresh_window(cx);
        cx.notify();
        true
    }
}

#[cfg(test)]
#[path = "queue_recovery_tests.rs"]
mod tests;

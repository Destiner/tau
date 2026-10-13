use super::*;
use async_channel::{Receiver, Sender};
use tau_session::session::SavedPreviewToken;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    ticket: u64,
    runtime: String,
    project: String,
    id: String,
    path: String,
    attempt: u64,
    token: SavedPreviewToken,
}
struct Job {
    binding: Binding,
    read_generation: u64,
}
struct Reply {
    binding: Binding,
    messages: Option<Vec<serde_json::Value>>,
}

trait Reader: Send + Sync + 'static {
    fn begin(&self) -> u64;
    fn cancel(&self);
    fn read(&self, job: &Job) -> Option<Vec<serde_json::Value>>;
}
#[cfg(not(test))]
struct CoreReader;
#[cfg(not(test))]
impl Reader for CoreReader {
    fn begin(&self) -> u64 {
        tau_core::saved_transcript::begin_saved_transcript_read()
    }
    fn cancel(&self) {
        tau_core::saved_transcript::cancel_saved_transcript();
    }
    fn read(&self, job: &Job) -> Option<Vec<serde_json::Value>> {
        tau_core::saved_transcript::read_saved_transcript_at_generation(
            job.binding.project.clone(),
            job.binding.id.clone(),
            job.binding.path.clone(),
            job.read_generation,
        )
        .messages
    }
}
struct Worker {
    jobs: Sender<Job>,
    pending: Receiver<Job>,
    reader: std::sync::Arc<dyn Reader>,
}
impl Worker {
    fn launch(reader: std::sync::Arc<dyn Reader>) -> (Self, Receiver<Reply>) {
        let (jobs, pending) = async_channel::bounded::<Job>(1);
        let (events, replies) = async_channel::bounded(1);
        let receiver = pending.clone();
        let backend = reader.clone();
        std::thread::spawn(move || {
            while let Ok(job) = receiver.recv_blocking() {
                if events.is_closed() {
                    break;
                }
                let messages = backend.read(&job);
                if events
                    .send_blocking(Reply {
                        binding: job.binding,
                        messages,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        (
            Self {
                jobs,
                pending,
                reader,
            },
            replies,
        )
    }
    fn cancel(&self) {
        // Cancellation never queues behind a blocked SSH read or a full reply channel.
        self.reader.cancel();
        while self.pending.try_recv().is_ok() {}
    }
    fn submit(&self, binding: Binding) {
        let read_generation = self.reader.begin();
        // One running and one newest pending read; no thread per navigation.
        while self.pending.try_recv().is_ok() {}
        let _ = self.jobs.try_send(Job {
            binding,
            read_generation,
        });
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        self.jobs.close();
    }
}
#[derive(Default)]
pub(super) struct State {
    ticket: u64,
    binding: Option<Binding>,
    worker: Option<Worker>,
    task: Option<gpui_kit::Task<()>>,
}
impl Shell {
    pub(super) fn sync_saved_history(&mut self) {
        let current = self.saved_history.binding.as_ref().is_none_or(|binding| {
            self.lifecycle.is_none()
                && self.active == binding.runtime
                && self.sessions.get(&binding.runtime).is_some_and(|live| {
                    live.start_attempt == binding.attempt
                        && live.project.path == binding.project
                        && live.source_id == binding.id
                        && live.source_path == binding.path
                        && live.controller.saved_preview_admitted(&binding.token)
                })
        });
        if !current {
            self.saved_history.binding = None;
            if let Some(worker) = &self.saved_history.worker {
                worker.cancel();
            }
        }
    }
    pub(super) fn navigate_saved_history(&mut self, cx: &mut Context<Self>) {
        self.saved_history.ticket = self.saved_history.ticket.wrapping_add(1);
        self.saved_history.binding = None;
        if let Some(worker) = &self.saved_history.worker {
            worker.cancel();
        }
        let Some(live) = self.sessions.get(&self.active) else {
            return;
        };
        let Some(token) = live.controller.saved_preview_token() else {
            return;
        };
        if live.source_id != live.controller.state().identity.id
            || live.source_path != live.controller.state().identity.path
            || self.lifecycle.is_some()
        {
            return;
        }
        let binding = Binding {
            ticket: self.saved_history.ticket,
            runtime: self.active.clone(),
            project: live.project.path.clone(),
            id: live.source_id.clone(),
            path: live.source_path.clone(),
            attempt: live.start_attempt,
            token,
        };
        if self.saved_history.worker.is_none() {
            #[cfg(not(test))]
            let backend: std::sync::Arc<dyn Reader> = std::sync::Arc::new(CoreReader);
            #[cfg(test)]
            let backend: std::sync::Arc<dyn Reader> = std::sync::Arc::new(tests::MissReader);
            let (worker, replies) = Worker::launch(backend);
            self.saved_history.worker = Some(worker);
            self.saved_history.task = Some(cx.spawn(async move |owner, cx| {
                while let Ok(reply) = replies.recv().await {
                    let _ = owner.update(cx, |s, cx| {
                        s.saved_history_reply(reply, cx);
                    });
                }
            }));
        }
        self.saved_history.binding = Some(binding.clone());
        self.saved_history.worker.as_ref().unwrap().submit(binding);
    }
    fn saved_history_reply(&mut self, reply: Reply, cx: &mut Context<Self>) {
        if self.lifecycle.is_some()
            || self.active != reply.binding.runtime
            || self.saved_history.binding.as_ref() != Some(&reply.binding)
        {
            return;
        }
        let Some(live) = self.sessions.get_mut(&reply.binding.runtime) else {
            return;
        };
        if live.start_attempt != reply.binding.attempt
            || live.project.path != reply.binding.project
            || live.source_id != reply.binding.id
            || live.source_path != reply.binding.path
        {
            return;
        }
        let Some(messages) = reply.messages else {
            return;
        };
        if live
            .controller
            .apply_saved_preview(&reply.binding.token, &messages)
        {
            live.dirty = true;
            live.projection_dirty = true;
            self.refresh_window(cx);
            cx.notify();
        }
    }
}

#[cfg(test)]
#[path = "saved_history_tests.rs"]
mod tests;

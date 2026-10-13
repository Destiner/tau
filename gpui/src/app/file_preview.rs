//! Shell integration contract: run `load` on the background executor; paint only
//! results accepted by `PreviewOwner`. The shell, not a virtual transcript row,
//! retains the FileViewer and origin focus handle.
use crate::components::files::{
    FileContent, preview_path,
    preview_type::{self, PreviewKind},
};
use gpui_kit::{Image, ImageFormat, SvgRenderer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[path = "file_preview_image.rs"]
mod image_admission;
use tau_core::file_preview::{PreparedPreview, PreviewService, Snapshot, read_text};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewContext {
    pub session: String,
    pub source: String,
    pub base_path: String,
    pub remote_project: Option<String>,
    /// Present while replacing a Markdown preview, absent in transcript prose.
    pub document_source_path: Option<String>,
    pub project_root: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRequest {
    pub generation: u64,
    pub context: PreviewContext,
    pub written_path: String,
    pub resolved_path: String,
}
pub struct LoadedPreview {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Retains the preview snapshot until its owner releases it"
        )
    )]
    pub snapshot: Arc<Snapshot>,
    pub content: FileContent,
    pub truncated: bool,
    pub image: Option<Arc<Image>>,
}
pub enum PreviewResult {
    Ready(LoadedPreview),
    Directory,
    Busy,
    Unavailable,
}

/// No worker updates an entity directly. Closing, source replacement and session
/// switching invalidate the token, and dropping stale Ready results releases them.
#[derive(Default)]
pub struct PreviewOwner {
    generation: u64,
    pending: Option<PreviewRequest>,
    active: Option<LoadedPreview>,
    decode_cancelled: Arc<AtomicBool>,
}
impl PreviewOwner {
    pub fn begin(
        &mut self,
        context: PreviewContext,
        written_path: String,
    ) -> Option<PreviewRequest> {
        if self.pending.is_some() {
            return None;
        }
        self.decode_cancelled.store(true, Ordering::Relaxed);
        self.decode_cancelled = Arc::new(AtomicBool::new(false));
        self.generation = self.generation.wrapping_add(1);
        let resolved_path = context.document_source_path.as_ref().map_or_else(
            || written_path.clone(),
            |source| {
                preview_path::resolve_context(
                    &written_path,
                    source,
                    context.project_root.as_deref(),
                )
            },
        );
        let request = PreviewRequest {
            generation: self.generation,
            context,
            written_path,
            resolved_path,
        };
        self.pending = Some(request.clone());
        Some(request)
    }
    pub fn is_current(&self, request: &PreviewRequest) -> bool {
        self.pending.as_ref() == Some(request)
    }
    pub fn invalidate(&mut self) {
        self.decode_cancelled.store(true, Ordering::Relaxed);
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.active = None;
    }
    /// Returns non-ready results for local feedback/directory opening. A Ready
    /// result replaces the owned snapshot only after exact request admission.
    pub fn complete(
        &mut self,
        request: &PreviewRequest,
        result: PreviewResult,
    ) -> Option<PreviewResult> {
        if !self.is_current(request) {
            return None;
        }
        self.decode_cancelled.store(true, Ordering::Relaxed);
        self.pending = None;
        match result {
            PreviewResult::Ready(preview) => {
                self.active = Some(preview);
                None
            }
            other => Some(other),
        }
    }
    pub fn active(&self) -> Option<&LoadedPreview> {
        self.active.as_ref()
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
pub fn load(service: &PreviewService, request: &PreviewRequest) -> PreviewResult {
    let prepared = service.prepare(
        request.context.remote_project.as_deref(),
        &request.context.base_path,
        &request.resolved_path,
    );
    let snapshot = match prepared {
        Ok(PreparedPreview::Ready(snapshot)) => snapshot,
        Ok(PreparedPreview::Directory) => return PreviewResult::Directory,
        Ok(PreparedPreview::Busy) => return PreviewResult::Busy,
        Err(_) => return PreviewResult::Unavailable,
    };
    load_snapshot(request, snapshot)
}

/// Separate from preparation so the shell can apply Vue's 15s body/decode
/// deadline after the snapshot's bounded 30s preparation has completed.
#[cfg(test)]
pub fn load_snapshot(request: &PreviewRequest, snapshot: Arc<Snapshot>) -> PreviewResult {
    load_snapshot_cancellable(request, snapshot, &AtomicBool::new(false))
}

fn load_snapshot_cancellable(
    _request: &PreviewRequest,
    snapshot: Arc<Snapshot>,
    cancelled: &AtomicBool,
) -> PreviewResult {
    if cancelled.load(Ordering::Relaxed) {
        return PreviewResult::Unavailable;
    }
    let kind = preview_type::classify(&snapshot.filename).kind;
    let mut image = None;
    let (content, truncated) = match kind {
        PreviewKind::Text => match read_text(&snapshot) {
            Ok((text, truncated)) => (FileContent::Text(text), truncated),
            Err(_) => (
                FileContent::Error("Couldn’t preview this file.".into()),
                false,
            ),
        },
        PreviewKind::Image => {
            let ext = snapshot
                .filename
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let format = match ext.as_str() {
                "png" => Some(ImageFormat::Png),
                "jpeg" | "jpg" => Some(ImageFormat::Jpeg),
                "webp" => Some(ImageFormat::Webp),
                "gif" => Some(ImageFormat::Gif),
                "svg" => Some(ImageFormat::Svg),
                "bmp" => Some(ImageFormat::Bmp),
                "ico" => Some(ImageFormat::Ico),
                _ => None,
            };
            image = format.and_then(|format| {
                let bytes = std::fs::read(&snapshot.path).ok()?;
                image_admission::admit(format, &bytes, cancelled)?;
                if cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                let candidate = Arc::new(
                    Image::from_bytes(format, bytes)
                        .with_decode_limits(image_admission::DECODE_LIMITS),
                );
                // XML excludes parse-time raster resources; GPUI guards physical output.
                // The raw image retains the same limits for the display decode.
                candidate
                    .to_image_data(SvgRenderer::new(Arc::new(())))
                    .ok()?;
                (!cancelled.load(Ordering::Relaxed)).then_some(candidate)
            });
            if image.is_some() {
                (FileContent::Image, false)
            } else if ext == "avif" {
                (FileContent::Unsupported, false)
            } else {
                (
                    FileContent::Error("Couldn’t preview this file.".into()),
                    false,
                )
            }
        }
        _ => (FileContent::Unsupported, false),
    };
    PreviewResult::Ready(LoadedPreview {
        snapshot,
        content,
        truncated,
        image,
    })
}

/// Remote directories copy the path as written; never open an SSH path locally.
#[cfg(test)]
pub fn directory_copy_path(request: &PreviewRequest) -> Option<&str> {
    request
        .context
        .remote_project
        .as_ref()
        .map(|_| request.written_path.as_str())
}
pub fn can_copy_full_path(path: &str, context: &PreviewContext) -> bool {
    path.starts_with('/')
        || (!context.base_path.is_empty()
            && (context.remote_project.is_none() || !path.starts_with("~/")))
}

use super::Shell;
use crate::components::{
    files::{CloseRequested, FileViewer},
    markdown::LinkTarget,
};
use gpui_kit::{
    Action, App, AppContext, ClipboardItem, Context, Entity, FocusHandle, Pixels, Point,
    Subscription, Task, Window,
};
use std::time::Duration;

#[derive(Clone, PartialEq, Eq)]
struct Binding {
    runtime: String,
    generation: u64,
    identity: (String, String),
    source: (String, String),
    project: (String, String, Option<String>),
}

#[derive(Clone)]
pub(super) enum LinkSource {
    Project,
    Transcript {
        id: String,
        revision: u64,
    },
    Queue {
        preview: (String, String),
    },
    Dialog {
        id: String,
        entity: Entity<crate::components::dialogs::NativeDialog>,
    },
    Viewer {
        entity: Entity<FileViewer>,
        snapshot: Arc<Snapshot>,
    },
}
#[derive(Clone)]
struct LinkCapture {
    binding: Binding,
    source: LinkSource,
    context: PreviewContext,
    target: LinkTarget,
    anchor: Point<Pixels>,
    origin: Option<FocusHandle>,
    window: gpui_kit::AnyWindowHandle,
    full_allowed: bool,
}
#[derive(Clone, PartialEq, Action)]
#[action(namespace = tau_file_link, no_json)]
pub(super) struct CopyLink {
    pub token: u64,
    pub full: bool,
}

/// Owned by the shell, never by a transcript row or the Markdown cache.
#[cfg(test)]
type PrepareWorker = Arc<dyn Fn(&PreviewRequest) -> Result<PreparedPreview, String> + Send + Sync>;

#[derive(Default)]
pub(super) struct State {
    viewer: Option<Entity<FileViewer>>,
    source: Option<LinkSource>,
    anchor: Option<Point<Pixels>>,
    request_anchor: Option<Point<Pixels>>,
    menu: Option<(u64, LinkCapture)>,
    menu_revision: u64,
    menu_view: Option<Entity<crate::components::menus::Menu>>,
    menu_subscription: Option<Subscription>,
    feedback_capture: Option<LinkCapture>,
    feedback_task: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
    tasks: Vec<Task<()>>,
    links: Option<Task<()>>,
    body_loading: bool,
    opening_due: bool,
    owner: PreviewOwner,
    service: PreviewService,
    #[cfg(test)]
    prepare_worker: Option<PrepareWorker>,
    #[cfg(test)]
    prepare_receiver: Option<async_channel::Receiver<Result<PreparedPreview, String>>>,
    #[cfg(test)]
    body_receiver: Option<async_channel::Receiver<PreviewResult>>,
    binding: Option<Binding>,
    context: Option<PreviewContext>,
    origin: Option<FocusHandle>,
    snapshot: Option<Arc<Snapshot>>,
    /// Link-local feedback; parent may paint it near the captured origin.
    pub(super) feedback: Option<&'static str>,
    feedback_revision: u64,
}
impl State {
    fn invalidate(&mut self) {
        self.tasks.clear();
        self.links = None;
        self.body_loading = false;
        self.opening_due = false;
        self.subscriptions.clear();
        self.viewer = None;
        self.snapshot = None;
        self.owner.invalidate();
        self.binding = None;
        self.context = None;
        self.origin = None;
        self.feedback = None;
        self.source = None;
        self.anchor = None;
        self.request_anchor = None;
        self.menu = None;
        self.menu_view = None;
        self.menu_subscription = None;
        self.feedback_capture = None;
        self.feedback_task = None;
        self.feedback_revision = self.feedback_revision.wrapping_add(1);
    }
}
pub(super) fn notice_file_base<'a>(
    entry: &'a tau_session::transcript::Entry,
    working_directory: &'a str,
) -> Option<&'a str> {
    entry
        .arguments
        .get("basePath")
        .and_then(|value| value.as_str())
        .or_else(|| {
            entry
                .source
                .starts_with("local:notify:")
                .then_some(working_directory)
        })
        .filter(|base| !base.is_empty())
}
pub(super) fn full_copy_path(path: &str, context: &PreviewContext) -> Option<String> {
    if !can_copy_full_path(path, context)
        || path.is_empty()
        || path
            .chars()
            .chain(context.base_path.chars())
            .any(|ch| ch <= '\u{1f}' || ch == '\u{7f}')
    {
        return None;
    }
    if context.document_source_path.is_none() {
        let home = if context.remote_project.is_some() {
            if path == "~"
                || path.starts_with("~/")
                || (!path.starts_with('/') && !context.base_path.starts_with('/'))
            {
                return None;
            }
            None
        } else if path.starts_with("~/") {
            Some(
                tau_core::file_preview::resolve_local_path("", "~/")
                    .ok()?
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        };
        return Some(
            crate::components::markdown::file_reference::resolve_file_path(
                &context.base_path,
                path,
                home.as_deref(),
            ),
        );
    }
    let resolved = context.document_source_path.as_ref().map_or_else(
        || path.to_owned(),
        |source| preview_path::resolve_context(path, source, context.project_root.as_deref()),
    );
    if context.remote_project.is_some() {
        if resolved == "~" || resolved.starts_with("~/") {
            return None;
        }
        if resolved.starts_with('/') {
            return Some(preview_path::normalize(&resolved));
        }
        if !context.base_path.starts_with('/') {
            return None;
        }
        Some(preview_path::normalize(&format!(
            "{}/{resolved}",
            context.base_path
        )))
    } else {
        tau_core::file_preview::resolve_local_path(&context.base_path, &resolved)
            .ok()
            .map(|path| preview_path::normalize(&path.to_string_lossy()))
    }
}
impl Shell {
    pub(super) fn transcript_link_source(
        &self,
        runtime: &str,
        key: u64,
        cx: &App,
    ) -> Option<LinkSource> {
        let live = self.sessions.get(runtime)?;
        let id = live.transcript.read(cx).source_id(key)?;
        let entry = live
            .controller
            .state()
            .transcript
            .iter()
            .find(|entry| entry.id == id)?;
        Some(LinkSource::Transcript {
            id: id.into(),
            revision: entry.revision,
        })
    }
    fn link_source_current(&self, runtime: &str, source: &LinkSource, cx: &App) -> bool {
        let Some(live) = self.sessions.get(runtime) else {
            return false;
        };
        match source {
            LinkSource::Project => true,
            LinkSource::Transcript { id, revision } => live
                .controller
                .state()
                .transcript
                .iter()
                .any(|entry| &entry.id == id && entry.revision == *revision),
            LinkSource::Queue { preview } => live.queue.read(cx).preview() == Some(preview),
            LinkSource::Dialog { id, entity } => {
                live.extension
                    .as_ref()
                    .is_some_and(|(current, dialog)| current == id && dialog == entity)
                    && live
                        .controller
                        .state()
                        .extension_dialogs
                        .first()
                        .is_some_and(|request| &request.id == id)
            }
            LinkSource::Viewer { entity, snapshot } => {
                self.preview.viewer.as_ref() == Some(entity)
                    && self
                        .preview
                        .snapshot
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, snapshot))
            }
        }
    }
    fn link_context(
        &self,
        runtime: &str,
        source: &LinkSource,
        _cx: &App,
    ) -> Option<PreviewContext> {
        let live = self.sessions.get(runtime)?;
        let mut base = live.project.working_directory.clone();
        let mut source_path = live.source_path.clone();
        let mut document_source_path = None;
        match source {
            LinkSource::Transcript { id, .. } => {
                let entry = live
                    .controller
                    .state()
                    .transcript
                    .iter()
                    .find(|entry| &entry.id == id)?;
                source_path = id.clone();
                if entry.kind == "error" {
                    base.clear();
                }
                if entry.kind == "notice" {
                    // Runtime notifications have that runtime's cwd; history notices do not.
                    base = notice_file_base(entry, &live.project.working_directory)
                        .unwrap_or_default()
                        .into();
                }
            }
            LinkSource::Viewer { snapshot, .. } => {
                document_source_path = Some(snapshot.source_path.clone());
                if let Some(context) = &self.preview.context {
                    base = context.base_path.clone();
                }
            }
            _ => {}
        }
        Some(PreviewContext {
            session: runtime.into(),
            source: source_path,
            base_path: base,
            remote_project: live
                .project
                .connection_string
                .as_ref()
                .and_then(|_| (!live.project.path.is_empty()).then(|| live.project.path.clone())),
            document_source_path,
            project_root: Some(live.project.working_directory.clone()),
        })
    }
    fn link_source_admitted(&self, runtime: &str, source: &LinkSource) -> bool {
        (!self.preview_open() || matches!(source, LinkSource::Viewer { .. }))
            && (!self
                .sessions
                .get(runtime)
                .is_some_and(|live| !live.controller.state().extension_dialogs.is_empty())
                || matches!(
                    source,
                    LinkSource::Dialog { .. } | LinkSource::Viewer { .. }
                ))
    }
    fn capture_current(&self, capture: &LinkCapture, cx: &App) -> bool {
        capture.binding.runtime == self.active
            && self.link_source_admitted(&capture.binding.runtime, &capture.source)
            && !self.preview_blocked(cx)
            && self.preview_binding(&capture.binding.runtime).as_ref() == Some(&capture.binding)
            && self.link_source_current(&capture.binding.runtime, &capture.source, cx)
            && self
                .link_context(&capture.binding.runtime, &capture.source, cx)
                .as_ref()
                == Some(&capture.context)
    }
    pub(super) fn open_link_menu(
        &mut self,
        runtime: &str,
        source: LinkSource,
        target: LinkTarget,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_preview(cx);
        if runtime != self.active
            || self.preview_blocked(cx)
            || !self.link_source_current(runtime, &source, cx)
        {
            return;
        }
        if !self.link_source_admitted(runtime, &source) {
            return;
        }
        let Some(binding) = self.preview_binding(runtime) else {
            return;
        };
        let Some(context) = self.link_context(runtime, &source, cx) else {
            return;
        };
        if !matches!(target, LinkTarget::Web(_)) && context.base_path.is_empty() {
            return;
        }
        let full = !(binding.project.2.is_some() && context.remote_project.is_none())
            && match &target {
                LinkTarget::Web(_) => false,
                LinkTarget::WrittenPath(path) | LinkTarget::DocumentPath(path) => {
                    full_copy_path(path, &context).is_some()
                }
            };
        self.preview.menu_revision = self.preview.menu_revision.wrapping_add(1);
        let token = self.preview.menu_revision;
        let label = if matches!(target, LinkTarget::Web(_)) {
            "Copy URL"
        } else {
            "Copy Path"
        };
        self.preview.menu = Some((
            token,
            LinkCapture {
                binding,
                source,
                context,
                target,
                anchor,
                origin: window.focused(cx),
                window: window.window_handle(),
                full_allowed: full,
            },
        ));
        use crate::components::menus::{Menu, MenuEvent, MenuItem};
        let item = |id: &str, label: &str| MenuItem {
            id: id.into(),
            element_id: format!("file-link-{id}"),
            label: label.into(),
            shortcut: None,
            disabled: false,
        };
        let mut items = vec![item("copy", label)];
        if full {
            items.push(item("copy-full", "Copy Full Path"));
        }
        let menu = cx.new(|cx| Menu::new_context(self.dark, items, cx));
        self.preview.menu_subscription = Some(cx.subscribe_in(
            &menu,
            window,
            move |shell, menu, event: &MenuEvent, window, cx| {
                if shell.preview.menu_view.as_ref() != Some(menu)
                    || shell.preview.menu.as_ref().map(|(current, _)| *current) != Some(token)
                {
                    return;
                }
                // Menu owns focus restoration. Outside dismissal must not steal clicked focus.
                shell.preview.menu_view = None;
                match event {
                    MenuEvent::Selected(id) if id == "copy" || id == "copy-full" => {
                        shell.copy_link(
                            &CopyLink {
                                token,
                                full: id == "copy-full",
                            },
                            window,
                            cx,
                        );
                    }
                    _ => {
                        shell.preview.menu = None;
                        shell.sync_preview(cx);
                        cx.notify();
                    }
                }
            },
        ));
        let origin = self
            .preview
            .menu
            .as_ref()
            .and_then(|(_, capture)| capture.origin.clone());
        menu.update(cx, |menu, cx| menu.open_at(anchor, origin, window, cx));
        self.preview.menu_view = Some(menu);
        self.sync_preview(cx);
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn copy_link(
        &mut self,
        action: &CopyLink,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((token, capture)) = self.preview.menu.clone() else {
            return;
        };
        if token != action.token {
            return;
        }
        self.preview.menu_view = None;
        self.preview.menu = None;
        self.sync_preview(cx);
        if !self.capture_current(&capture, cx)
            || capture.window != window.window_handle()
            || (action.full && !capture.full_allowed)
        {
            return;
        }
        let value = match &capture.target {
            LinkTarget::Web(url) if !action.full => Some(url.clone()),
            LinkTarget::WrittenPath(path) | LinkTarget::DocumentPath(path) => {
                if action.full {
                    full_copy_path(path, &capture.context)
                } else {
                    Some(path.clone())
                }
            }
            _ => None,
        };
        self.preview.anchor = Some(capture.anchor);
        self.preview.feedback = if let Some(value) = value {
            cx.write_to_clipboard(ClipboardItem::new_string(value));
            Some("Copied")
        } else {
            Some("Couldn’t copy path. Try again.")
        };
        self.preview.feedback_capture = Some(capture);
        self.expire_preview_feedback(cx);
        cx.notify();
    }
    pub(super) fn preview_menu_view(&self) -> Option<Entity<crate::components::menus::Menu>> {
        self.preview.menu_view.clone()
    }
    fn preview_menu_open(&self, cx: &App) -> bool {
        self.preview
            .menu_view
            .as_ref()
            .is_some_and(|menu| menu.read(cx).is_open())
    }
    pub(super) fn preview_anchor(&self) -> Option<Point<Pixels>> {
        self.preview.anchor
    }
    pub(super) fn sync_preview_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &self.preview.menu_view {
            menu.update(cx, |menu, cx| menu.set_theme(self.dark, window, cx));
        }
        if let Some(viewer) = &self.preview.viewer {
            viewer.update(cx, |view, cx| view.set_theme(self.dark, window, cx));
        }
    }
    pub(super) fn preview_feedback(&self) -> Option<&'static str> {
        self.preview.feedback
    }
    fn expire_preview_feedback(&mut self, cx: &mut Context<Self>) {
        self.preview.feedback_revision = self.preview.feedback_revision.wrapping_add(1);
        let revision = self.preview.feedback_revision;
        self.preview.feedback_task = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1_200))
                .await;
            let _ = weak.update(cx, |shell, cx| {
                shell.sync_preview(cx);
                if shell.preview.feedback_revision == revision {
                    shell.preview.feedback_capture = None;
                    if shell.preview.owner.pending() && !shell.preview.body_loading {
                        shell.preview.feedback = Some("Opening…");
                        shell.preview.anchor = shell.preview.request_anchor;
                    } else {
                        shell.preview.feedback = None;
                    }
                    cx.notify();
                }
            });
        }));
    }
    fn preview_binding(&self, runtime: &str) -> Option<Binding> {
        let live = self.sessions.get(runtime)?;
        Some(Binding {
            runtime: runtime.into(),
            generation: live.controller.generation(),
            identity: (
                live.controller.state().identity.id.clone(),
                live.controller.state().identity.path.clone(),
            ),
            source: (live.source_id.clone(), live.source_path.clone()),
            project: (
                live.project.path.clone(),
                live.project.working_directory.clone(),
                live.project.connection_string.clone(),
            ),
        })
    }
    fn preview_blocked(&self, cx: &gpui_kit::App) -> bool {
        self.lifecycle.is_some()
            || self.confirmation.is_some()
            || self.error.is_some()
            || self
                .fallback
                .as_ref()
                .is_some_and(|(_, dialog)| dialog.read(cx).is_open())
            || self.picker_busy
            || self.remote.is_open()
            || self.palette.is_open(cx)
            || self.admin_reporter_open(cx)
    }
    pub(super) fn preview_open(&self) -> bool {
        self.preview.viewer.is_some()
    }
    pub(super) fn preview_view(&self) -> Option<Entity<FileViewer>> {
        self.preview.viewer.clone()
    }
    pub(super) fn sync_preview(&mut self, cx: &mut Context<Self>) {
        if self
            .preview
            .menu
            .as_ref()
            .is_some_and(|(_, capture)| !self.capture_current(capture, cx))
        {
            self.preview.menu = None;
            self.preview.menu_view = None;
            self.preview.menu_subscription = None;
        }
        if self
            .preview
            .feedback_capture
            .as_ref()
            .is_some_and(|capture| !self.capture_current(capture, cx))
        {
            self.preview.feedback_capture = None;
            self.preview.feedback_revision = self.preview.feedback_revision.wrapping_add(1);
            if self.preview.opening_due
                && self.preview.owner.pending()
                && !self.preview.body_loading
            {
                self.preview.feedback = Some("Opening…");
                self.preview.anchor = self.preview.request_anchor;
            } else {
                self.preview.feedback = None;
            }
        }
        if self.preview_blocked(cx)
            || (self.preview.owner.pending()
                && !self.preview.body_loading
                && self
                    .preview
                    .binding
                    .as_ref()
                    .zip(self.preview.source.as_ref())
                    .is_some_and(|(binding, source)| {
                        !self.link_source_admitted(&binding.runtime, source)
                    }))
        {
            self.preview.feedback = None;
            if self.preview.owner.pending() && !self.preview.body_loading {
                self.preview
                    .owner
                    .decode_cancelled
                    .store(true, Ordering::Relaxed);
                self.preview.owner.pending = None;
                self.preview.tasks.clear();
            }
        }
        if let Some(viewer) = self.preview.viewer.clone() {
            let enabled = !self.preview_blocked(cx) && !self.preview_menu_open(cx);
            viewer.update(cx, |viewer, _| viewer.set_keyboard_enabled(enabled));
        }
        if self.preview.binding.as_ref().is_some_and(|binding| {
            binding.runtime != self.active
                || self.preview_binding(&binding.runtime).as_ref() != Some(binding)
                || self
                    .preview
                    .source
                    .as_ref()
                    .is_some_and(|source| !self.link_source_current(&binding.runtime, source, cx))
        }) {
            // Retire the preview without discarding another still-current link's menu/status.
            let menu = self
                .preview
                .menu
                .take()
                .filter(|(_, capture)| self.capture_current(capture, cx));
            let feedback = self
                .preview
                .feedback_capture
                .take()
                .filter(|capture| self.capture_current(capture, cx));
            let retained = feedback.map(|capture| {
                (
                    capture,
                    self.preview.feedback,
                    self.preview.anchor,
                    self.preview.feedback_revision,
                    self.preview.feedback_task.take(),
                )
            });
            let menu_view = self.preview.menu_view.take().filter(|_| menu.is_some());
            let menu_subscription = self
                .preview
                .menu_subscription
                .take()
                .filter(|_| menu.is_some());
            self.preview.invalidate();
            self.preview.menu = menu;
            self.preview.menu_view = menu_view;
            self.preview.menu_subscription = menu_subscription;
            if let Some((capture, message, anchor, revision, task)) = retained {
                self.preview.feedback_capture = Some(capture);
                self.preview.feedback = message;
                self.preview.anchor = anchor;
                self.preview.feedback_revision = revision;
                self.preview.feedback_task = task;
            }
            cx.notify();
        }
    }
    pub(super) fn close_preview(&mut self, cx: &mut Context<Self>) {
        let origin = self.preview.origin.clone();
        self.preview.invalidate();
        if !self.preview_blocked(cx) {
            self.restore_input_focus_from(origin, cx);
        }
        cx.notify();
    }
    #[cfg(test)]
    pub(super) fn open_preview_link(
        &mut self,
        runtime: &str,
        target: LinkTarget,
        origin: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = if let (Some(entity), Some(snapshot)) =
            (&self.preview.viewer, &self.preview.snapshot)
        {
            LinkSource::Viewer {
                entity: entity.clone(),
                snapshot: snapshot.clone(),
            }
        } else {
            LinkSource::Project
        };
        self.open_source_link(
            runtime,
            source,
            target,
            origin,
            window.mouse_position(),
            window,
            cx,
        );
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Source admission requires the runtime, origin focus and pointer alongside the link"
    )]
    pub(super) fn open_source_link(
        &mut self,
        runtime: &str,
        source: LinkSource,
        target: LinkTarget,
        origin: Option<FocusHandle>,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_preview(cx);
        if runtime != self.active
            || self.preview_blocked(cx)
            || !self.link_source_current(runtime, &source, cx)
        {
            return;
        }
        if !self.link_source_admitted(runtime, &source) {
            return;
        }
        if let LinkTarget::Web(url) = target {
            cx.open_url(&url);
            return;
        }
        let path = match target {
            LinkTarget::WrittenPath(path) | LinkTarget::DocumentPath(path) => path,
            LinkTarget::Web(_) => unreachable!(),
        };
        let Some(binding) = self.preview_binding(runtime) else {
            return;
        };
        let Some(context) = self.link_context(runtime, &source, cx) else {
            return;
        };
        if context.base_path.is_empty() {
            return;
        }
        let remote = self.sessions[runtime].project.connection_string.is_some();
        let remote_project = context.remote_project.clone();
        if remote && remote_project.is_none() {
            self.preview.anchor = Some(anchor);
            self.preview.binding = Some(binding.clone());
            self.preview.source = Some(source.clone());
            self.preview.feedback_capture = None;
            self.preview.tasks.clear();
            cx.write_to_clipboard(ClipboardItem::new_string(path));
            self.preview.feedback = Some("Copied");
            self.expire_preview_feedback(cx);
            cx.notify();
            return;
        }

        let Some(request) = self.preview.owner.begin(context.clone(), path) else {
            return;
        };
        self.preview.anchor = Some(anchor);
        self.preview.request_anchor = Some(anchor);
        self.preview.source = Some(source);
        self.preview.feedback_capture = None;
        self.preview.tasks.clear();
        self.preview.feedback_revision = self.preview.feedback_revision.wrapping_add(1);
        self.preview.body_loading = false;
        self.preview.opening_due = false;
        if self.preview.viewer.is_none() {
            self.preview.origin = origin;
        }
        self.preview.binding = Some(binding);
        self.preview.context = Some(context);
        self.preview.feedback = None;
        let handle = window.window_handle();
        let timer_request = request.clone();
        self.preview.tasks.push(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = weak.update(cx, |shell, cx| {
                shell.sync_preview(cx);
                if shell.preview.owner.is_current(&timer_request) && !shell.preview.body_loading {
                    shell.preview.opening_due = true;
                    if shell.preview.feedback.is_none()
                        || shell.preview.feedback == Some("Opening…")
                    {
                        shell.preview.feedback = Some("Opening…");
                        shell.preview.anchor = shell.preview.request_anchor;
                        cx.notify();
                    }
                }
            });
        }));
        let deadline_request = request.clone();
        self.preview.tasks.push(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
            let _ = weak.update(cx, |shell, cx| {
                shell.sync_preview(cx);
                if shell.preview.owner.is_current(&deadline_request) && !shell.preview.body_loading
                {
                    shell
                        .preview
                        .owner
                        .complete(&deadline_request, PreviewResult::Unavailable);
                    shell.preview.feedback = Some("Preview unavailable");
                    shell.preview.feedback_capture = None;
                    shell.preview.anchor = shell.preview.request_anchor;
                    shell.expire_preview_feedback(cx);
                    cx.notify();
                }
            });
        }));
        let service = self.preview.service.clone();
        let worker_request = request.clone();
        #[cfg(test)]
        let fake = self.preview.prepare_worker.clone();
        #[cfg(test)]
        let receiver = self.preview.prepare_receiver.clone();
        let worker = cx.background_executor().spawn(async move {
            #[cfg(test)]
            if let Some(receiver) = receiver {
                return receiver
                    .recv()
                    .await
                    .unwrap_or_else(|_| Err("Unavailable".into()));
            }
            #[cfg(test)]
            if let Some(fake) = fake {
                return fake(&worker_request);
            }
            service.prepare(
                worker_request.context.remote_project.as_deref(),
                &worker_request.context.base_path,
                &worker_request.resolved_path,
            )
        });
        self.preview.tasks.push(cx.spawn(async move |weak, cx| {
            let prepared = worker.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = weak.update(cx, |shell, cx| {
                    shell.preview_prepared(request, prepared, window, cx)
                });
            });
        }));
        cx.notify();
    }
    fn preview_prepared(
        &mut self,
        request: PreviewRequest,
        prepared: Result<PreparedPreview, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_preview(cx);
        if !self.preview.owner.is_current(&request) {
            return;
        }
        if self.preview_blocked(cx) {
            self.preview
                .owner
                .complete(&request, PreviewResult::Unavailable);
            self.preview.feedback = None;
            return;
        }
        let snapshot = match prepared {
            Ok(PreparedPreview::Ready(snapshot)) => snapshot,
            other => {
                self.preview.anchor = self.preview.request_anchor;
                self.preview.feedback_capture = None;
                let result = match other {
                    Ok(PreparedPreview::Directory) => PreviewResult::Directory,
                    Ok(PreparedPreview::Busy) => PreviewResult::Busy,
                    _ => PreviewResult::Unavailable,
                };
                if matches!(result, PreviewResult::Directory) {
                    if request.context.remote_project.is_some() {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            request.written_path.clone(),
                        ));
                        self.preview.feedback = Some("Copied");
                    } else if let Ok(path) = tau_core::file_preview::resolve_local_path(
                        &request.context.base_path,
                        &request.resolved_path,
                    ) {
                        cx.open_with_system(&path);
                        self.preview.feedback = None;
                    } else {
                        self.preview.feedback = Some("Couldn’t open path. Try again.");
                    }
                } else {
                    self.preview.feedback = Some(if matches!(result, PreviewResult::Busy) {
                        "Can’t open right now"
                    } else {
                        "Preview unavailable"
                    });
                }
                self.preview.owner.complete(&request, result);
                if self.preview.feedback.is_some() {
                    self.expire_preview_feedback(cx);
                }
                cx.notify();
                return;
            }
        };
        if self.preview.feedback == Some("Opening…") {
            self.preview.feedback = None;
        }
        self.preview.body_loading = true;
        self.preview.links = None;
        // Release the replaced snapshot only once its old viewer has been unmounted.
        self.preview.subscriptions.clear();
        self.preview.viewer = None;
        self.preview.snapshot = Some(snapshot.clone());
        self.preview.owner.active = None;
        let directory = preview_path::directory_for_path(
            &snapshot.source_path,
            request.context.project_root.as_deref(),
        );
        let viewer = cx.new(|cx| {
            FileViewer::new(
                snapshot.filename.clone(),
                directory,
                FileContent::Loading,
                None,
                self.dark,
                window,
                cx,
            )
        });
        self.preview.subscriptions.push(cx.subscribe_in(
            &viewer,
            window,
            |shell, view, _: &CloseRequested, _, cx| {
                if shell.preview.viewer.as_ref() == Some(view) {
                    shell.close_preview(cx);
                }
            },
        ));
        self.preview.viewer = Some(viewer.clone());
        // The initiating viewer has been replaced; the new document is project-owned.
        if matches!(self.preview.source, Some(LinkSource::Viewer { .. })) {
            self.preview.source = Some(LinkSource::Project);
        }
        let context_owner = cx.entity().downgrade();
        let context_viewer = viewer.downgrade();
        let context_snapshot = snapshot.clone();
        let context_runtime = request.context.session.clone();
        viewer.update(cx, |view, cx| {
            view.set_context_handler(
                move |target, event, window, cx| {
                    let Some(entity) = context_viewer.upgrade() else {
                        return;
                    };
                    let source = LinkSource::Viewer {
                        entity,
                        snapshot: context_snapshot.clone(),
                    };
                    let _ = context_owner.update(cx, |shell, cx| {
                        shell.open_link_menu(
                            &context_runtime,
                            source,
                            target,
                            event.position,
                            window,
                            cx,
                        );
                    });
                },
                cx,
            )
        });
        let (sender, receiver) = async_channel::unbounded();
        viewer.update(cx, |viewer, cx| {
            viewer.set_link_handler(
                move |event| {
                    let _ = sender.try_send((event.target, event.anchor));
                },
                cx,
            )
        });
        let runtime = request.context.session.clone();
        let handle = window.window_handle();
        let viewer = viewer.downgrade();
        self.preview.links = Some(cx.spawn(async move |weak, cx| {
            while let Ok((target, anchor)) = receiver.recv().await {
                let _ = handle.update(cx, |_, window, cx| {
                    let Some(viewer) = viewer.upgrade() else {
                        return;
                    };
                    let _ =
                        weak.update(cx, |shell, cx| {
                            if shell.preview.viewer.as_ref() == Some(&viewer) {
                                let source = shell.preview.snapshot.clone().map(|snapshot| {
                                    LinkSource::Viewer {
                                        entity: viewer.clone(),
                                        snapshot,
                                    }
                                });
                                if let Some(source) = source {
                                    shell.open_source_link(
                                        &runtime, source, target, None, anchor, window, cx,
                                    );
                                    shell.preview.anchor = Some(anchor);
                                    shell.preview.request_anchor = Some(anchor);
                                }
                            }
                        });
                });
            }
        }));
        for (delay, deadline) in [
            (Duration::from_millis(200), false),
            (Duration::from_secs(15), true),
        ] {
            let request = request.clone();
            self.preview.tasks.push(cx.spawn(async move |weak, cx| {
                cx.background_executor().timer(delay).await;
                let _ = weak.update(cx, |shell, cx| {
                    shell.sync_preview(cx);
                    if !shell.preview.owner.is_current(&request) {
                        return;
                    }
                    if deadline {
                        shell
                            .preview
                            .owner
                            .complete(&request, PreviewResult::Unavailable);
                    }
                    if let Some(viewer) = shell.preview.viewer.clone() {
                        viewer.update(cx, |viewer, cx| {
                            if deadline {
                                viewer.set_content(
                                    FileContent::Error("Couldn’t preview this file.".into()),
                                    cx,
                                );
                            } else {
                                viewer.set_loading_visible(true, cx);
                            }
                        });
                    }
                    cx.notify();
                });
            }));
        }
        let worker_request = request.clone();
        let decode_cancelled = self.preview.owner.decode_cancelled.clone();
        #[cfg(test)]
        let receiver = self.preview.body_receiver.clone();
        let worker = cx.background_executor().spawn(async move {
            #[cfg(test)]
            if let Some(receiver) = receiver {
                return receiver.recv().await.unwrap_or(PreviewResult::Unavailable);
            }
            load_snapshot_cancellable(&worker_request, snapshot, &decode_cancelled)
        });
        self.preview.tasks.push(cx.spawn(async move |weak, cx| {
            let result = worker.await;
            let _ = weak.update(cx, |shell, cx| {
                shell.sync_preview(cx);
                if !shell.preview.owner.is_current(&request) {
                    return;
                }
                shell.preview.owner.complete(&request, result);
                if let (Some(loaded), Some(viewer)) =
                    (shell.preview.owner.active(), shell.preview.viewer.clone())
                {
                    viewer.update(cx, |viewer, cx| {
                        if let Some(image) = &loaded.image {
                            viewer.set_image(image.clone(), cx);
                        } else {
                            viewer.set_content(loaded.content.clone(), cx);
                        }
                        viewer.set_truncated(loaded.truncated, cx);
                        if let Some(presentation) = viewer.presentation(cx) {
                            let remote = request.context.remote_project.is_some();
                            presentation.update(cx, |presentation, cx| {
                                presentation.set_copy_paths(remote, cx)
                            });
                        }
                    });
                } else if let Some(viewer) = shell.preview.viewer.clone() {
                    viewer.update(cx, |viewer, cx| {
                        viewer.set_content(
                            FileContent::Error("Couldn’t preview this file.".into()),
                            cx,
                        )
                    });
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
#[path = "file_preview_tests.rs"]
mod tests;

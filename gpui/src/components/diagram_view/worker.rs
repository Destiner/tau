//! Two workers, sixteen waiting jobs, fifty prepared entries / 16 MiB SVG+source.
//! Jobs contain no view handles; cancelling or dropping a view only leaves bounded data.
use super::super::diagram::*;
use gpui_kit::{Image, ImageFormat, RenderImage, SvgRenderer};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::Duration,
};

pub const MAX_SOURCE_BYTES: usize = 24_000;
pub(super) fn source_allowed(source: &str) -> bool {
    source.len() <= MAX_SOURCE_BYTES && source.encode_utf16().take(8001).count() <= 8000
}
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const QUEUE_SIZE: usize = 16;
const WORKERS: usize = 2;
type Reply = Result<(Diagram, Arc<RenderImage>, Arc<RenderImage>), DiagramError>;
pub struct Job {
    pub source: Arc<str>,
    pub dark: bool,
    pub diagram: Option<Diagram>,
    pub camera: Option<RasterCamera>,
    pub operation: Operation,
    pub decoder: SvgRenderer,
    pub reply: mpsc::SyncSender<Reply>,
}
struct Entry {
    source: Arc<str>,
    dark: bool,
    diagram: Diagram,
}
#[derive(Default)]
struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
}
impl Cache {
    fn get(&mut self, source: &str, dark: bool) -> Option<Diagram> {
        let index = self
            .entries
            .iter()
            .position(|e| e.dark == dark && &*e.source == source)?;
        let entry = self.entries.remove(index)?;
        let diagram = entry.diagram.clone();
        self.entries.push_back(entry);
        Some(diagram)
    }
    fn insert(&mut self, source: Arc<str>, dark: bool, diagram: Diagram) {
        let bytes = source.len() + diagram.svg().len();
        if bytes > CACHE_BYTES {
            return;
        }
        while self.entries.len() >= 50 || self.bytes + bytes > CACHE_BYTES {
            if let Some(e) = self.entries.pop_front() {
                self.bytes -= e.source.len() + e.diagram.svg().len();
            } else {
                break;
            }
        }
        self.bytes += bytes;
        self.entries.push_back(Entry {
            source,
            dark,
            diagram,
        });
    }
}
fn bounded_queue() -> (mpsc::SyncSender<Job>, mpsc::Receiver<Job>) {
    mpsc::sync_channel(QUEUE_SIZE)
}
fn service() -> &'static mpsc::SyncSender<Job> {
    static SERVICE: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let (tx, rx) = bounded_queue();
        let rx = Arc::new(Mutex::new(rx));
        let cache = Arc::new(Mutex::new(Cache::default()));
        // Initialization, including fonts, happens exclusively on these workers.
        let renderer = Arc::new(OnceLock::<Result<DiagramRenderer, DiagramError>>::new());
        for _ in 0..WORKERS {
            let (rx, cache, renderer) = (rx.clone(), cache.clone(), renderer.clone());
            std::thread::spawn(move || work(rx, |job| execute(job, &cache, &renderer)));
        }
        tx
    })
}
fn work(rx: Arc<Mutex<mpsc::Receiver<Job>>>, mut execute: impl FnMut(&Job) -> Reply) {
    loop {
        let Ok(job) = rx.lock().unwrap().recv() else {
            break;
        };
        let result = contain_panic(|| execute(&job));
        let _ = job.reply.try_send(result);
    }
}
fn execute(
    job: &Job,
    cache: &Mutex<Cache>,
    renderer: &OnceLock<Result<DiagramRenderer, DiagramError>>,
) -> Reply {
    job.operation.checkpoint()?;
    let renderer = renderer
        .get_or_init(DiagramRenderer::new)
        .as_ref()
        .map_err(|e| *e)?;
    job.operation.checkpoint()?;
    let diagram = if let Some(d) = &job.diagram {
        d.clone()
    } else {
        let cached = cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&job.source, job.dark);
        match cached {
            Some(d) => d,
            None => {
                let d =
                    renderer.render(&job.source, DiagramTheme::ayu(job.dark), &job.operation)?;
                job.operation.checkpoint()?;
                cache.lock().unwrap_or_else(|e| e.into_inner()).insert(
                    job.source.clone(),
                    job.dark,
                    d.clone(),
                );
                d
            }
        }
    };
    let b = diagram.view_box();
    let camera = job.camera.unwrap_or(RasterCamera {
        viewport: b,
        backing_scale: 2.,
        zoom: (800. / b.width).min(1.).min(800. / b.height),
    });
    let raster = renderer.rasterize(&diagram, camera, &job.operation)?;
    let icon_source = include_str!("expand.svg").replace(
        "#ffffff",
        &format!("#{:06x}", super::super::support::tokens(job.dark).muted),
    );
    let icon = Image::from_bytes(ImageFormat::Svg, icon_source.into_bytes())
        .to_image_data(job.decoder.clone())
        .map_err(|_| DiagramError::Internal)?;
    let image = upload(raster, job.decoder.clone())?;
    job.operation.checkpoint()?;
    Ok((diagram, image, icon))
}
fn contain_panic(f: impl FnOnce() -> Reply) -> Reply {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(DiagramError::Internal))
}

/// One pending job per view, retried by its weak task; queue wait shares the render deadline.
/// No blocking send and no deadline renewal when the sixteen waiting slots are full.
pub struct Request {
    sender: mpsc::SyncSender<Job>,
    pending: Option<Job>,
    receiver: mpsc::Receiver<Reply>,
    operation: Operation,
}
impl Request {
    pub fn poll(&mut self) -> Option<Reply> {
        if let Err(e) = self.operation.checkpoint() {
            self.pending = None;
            return Some(Err(e));
        }
        if let Some(job) = self.pending.take() {
            match self.sender.try_send(job) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(job)) => {
                    self.pending = Some(job);
                    return None;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Some(Err(DiagramError::Internal));
                }
            }
        }
        match self.receiver.try_recv() {
            Ok(reply) => Some(reply),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(DiagramError::Internal)),
        }
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        self.operation.cancel();
    }
}

pub fn submit(
    source: Arc<str>,
    dark: bool,
    diagram: Option<Diagram>,
    camera: Option<RasterCamera>,
    decoder: SvgRenderer,
) -> Result<(Operation, Request), DiagramError> {
    if !source_allowed(&source) {
        return Err(DiagramError::SourceLimit);
    }
    let operation = Operation::new(Duration::from_secs(5));
    let (reply, rx) = mpsc::sync_channel(1);
    let request = Request {
        sender: service().clone(),
        pending: Some(Job {
            source,
            dark,
            diagram,
            camera,
            operation: operation.clone(),
            decoder,
            reply,
        }),
        receiver: rx,
        operation: operation.clone(),
    };
    Ok((operation, request))
}
pub(super) fn upload(
    raster: RasterImage,
    decoder: SvgRenderer,
) -> Result<Arc<RenderImage>, DiagramError> {
    // tiny-skia's PNG encoder demultiplies RGBA; GPUI decodes and swaps R/B.
    // Passing premultiplied RGBA directly to RenderImage would get both wrong.
    let pixmap = resvg::tiny_skia::Pixmap::from_vec(
        raster.rgba,
        resvg::tiny_skia::IntSize::from_wh(raster.width, raster.height)
            .ok_or(DiagramError::RasterLimit)?,
    )
    .ok_or(DiagramError::RasterLimit)?;
    let png = pixmap.encode_png().map_err(|_| DiagramError::Internal)?;
    Image::from_bytes(ImageFormat::Png, png)
        .to_image_data(decoder)
        .map_err(|_| DiagramError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(sender: &mpsc::SyncSender<Job>, timeout: Duration) -> Request {
        let operation = Operation::new(timeout);
        let (reply, receiver) = mpsc::sync_channel(1);
        Request {
            sender: sender.clone(),
            pending: Some(Job {
                source: Arc::from("flowchart TD\nA --> B"),
                dark: false,
                diagram: None,
                camera: None,
                operation: operation.clone(),
                decoder: SvgRenderer::new(Arc::new(crate::components::Assets)),
                reply,
            }),
            receiver,
            operation,
        }
    }
    fn wait(request: &mut Request) -> Reply {
        let limit = std::time::Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(reply) = request.poll() {
                return reply;
            }
            assert!(std::time::Instant::now() < limit, "request stuck pending");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn fill(sender: &mpsc::SyncSender<Job>) -> Vec<Request> {
        (0..QUEUE_SIZE)
            .map(|_| {
                let mut r = request(sender, Duration::from_secs(5));
                assert!(r.poll().is_none());
                assert!(r.pending.is_none());
                r
            })
            .collect()
    }
    #[test]
    fn saturated_queue_retries_then_renders_without_resetting_operation() {
        let (tx, rx) = bounded_queue();
        let mut blockers = fill(&tx);
        let mut retry = request(&tx, Duration::from_secs(5));
        for _ in 0..32 {
            assert!(retry.poll().is_none());
            assert!(retry.pending.is_some());
        }
        // Release one real queue slot; the previously rejected job must now enter.
        let first = rx.recv().unwrap();
        assert!(retry.poll().is_none());
        assert!(retry.pending.is_none());
        let cache = Mutex::new(Cache::default());
        let renderer = OnceLock::new();
        let worker = std::thread::spawn(move || {
            let _ = first
                .reply
                .try_send(contain_panic(|| execute(&first, &cache, &renderer)));
            work(Arc::new(Mutex::new(rx)), |job| {
                execute(job, &cache, &renderer)
            })
        });
        let (diagram, image, _) = wait(&mut retry).unwrap();
        assert!(diagram.svg().contains("<svg"));
        assert!(image.size(0).width > gpui_kit::DevicePixels(1));
        for blocker in &mut blockers {
            assert!(wait(blocker).is_ok());
        }
        drop((retry, blockers));
        drop(tx);
        worker.join().unwrap();
    }
    #[test]
    fn full_queue_deadline_and_cancellation_never_admit_late_work() {
        let (tx, rx) = bounded_queue();
        let _blockers = fill(&tx);
        let mut expired = request(&tx, Duration::from_millis(20));
        assert!(expired.poll().is_none());
        assert!(matches!(wait(&mut expired), Err(DiagramError::Deadline)));
        assert!(expired.pending.is_none());
        drop(rx.recv().unwrap());
        assert!(matches!(expired.poll(), Some(Err(DiagramError::Deadline))));
        assert_eq!(rx.try_iter().count(), QUEUE_SIZE - 1);
        let _blockers = fill(&tx);
        let mut cancelled = request(&tx, Duration::from_secs(5));
        assert!(cancelled.poll().is_none());
        cancelled.operation.cancel();
        assert!(matches!(
            cancelled.poll(),
            Some(Err(DiagramError::Cancelled))
        ));
        assert!(cancelled.pending.is_none());
        assert_eq!(rx.try_iter().count(), QUEUE_SIZE);
    }
    #[test]
    fn worker_panic_replies_and_keeps_servicing_valid_jobs() {
        let (tx, rx) = bounded_queue();
        let cache = Mutex::new(Cache::default());
        let renderer = OnceLock::new();
        let worker = std::thread::spawn(move || {
            let mut first = true;
            work(Arc::new(Mutex::new(rx)), |job| {
                if std::mem::take(&mut first) {
                    panic!("controlled worker panic");
                }
                execute(job, &cache, &renderer)
            });
        });
        let mut first = request(&tx, Duration::from_secs(5));
        assert!(matches!(wait(&mut first), Err(DiagramError::Internal)));
        let mut next = request(&tx, Duration::from_secs(5));
        assert!(wait(&mut next).is_ok());
        drop((first, next, tx));
        worker.join().unwrap();
    }
    #[test]
    fn pending_request_drop_cancels_without_enqueueing() {
        let (tx, rx) = bounded_queue();
        let _blockers = fill(&tx);
        let mut pending = request(&tx, Duration::from_secs(5));
        let op = pending.operation.clone();
        assert!(pending.poll().is_none());
        drop(pending);
        assert_eq!(op.checkpoint(), Err(DiagramError::Cancelled));
        assert_eq!(rx.try_iter().count(), QUEUE_SIZE);
    }
    #[test]
    fn source_guard_precedes_worker_admission() {
        let decoder = SvgRenderer::new(Arc::new(crate::components::Assets));
        assert!(matches!(
            submit(Arc::from("x".repeat(24001)), false, None, None, decoder),
            Err(DiagramError::SourceLimit)
        ));
    }
    #[test]
    fn png_route_demultiplies_and_swaps_channels() {
        let decoder = SvgRenderer::new(Arc::new(crate::components::Assets));
        let image = upload(
            RasterImage {
                width: 1,
                height: 1,
                rgba: vec![128, 0, 0, 128],
            },
            decoder,
        )
        .unwrap();
        assert_eq!(image.as_bytes(0).unwrap(), &[0, 0, 255, 128]);
    }
    #[test]
    fn prepared_cache_is_entry_bounded_and_promotes_hits() {
        let renderer = DiagramRenderer::new().unwrap();
        let op = Operation::new(Duration::from_secs(5));
        let d = renderer
            .render("flowchart TD\nA --> B", DiagramTheme::ayu(false), &op)
            .unwrap();
        let mut cache = Cache::default();
        for i in 0..60 {
            cache.insert(Arc::from(format!("source-{i}")), false, d.clone());
        }
        assert_eq!(cache.entries.len(), 50);
        assert!(cache.bytes <= CACHE_BYTES);
        assert!(cache.get("source-0", false).is_none());
        assert!(cache.get("source-10", false).is_some());
        assert!(cache.get("source-10", true).is_none());
        cache.insert(Arc::from("last"), false, d);
        assert!(cache.get("source-10", false).is_some());
        assert!(cache.get("source-11", false).is_none());
    }
    #[test]
    fn prepared_cache_evicts_by_bytes_before_fifty_entries() {
        let renderer = DiagramRenderer::new().unwrap();
        let diagram = renderer
            .render(
                "flowchart TD\nA --> B",
                DiagramTheme::ayu(false),
                &Operation::new(Duration::from_secs(5)),
            )
            .unwrap();
        let mut cache = Cache::default();
        // Exercise the byte budget independently of the public source-admission guard.
        for i in 0..8 {
            cache.insert(
                Arc::from(format!("{i}{}", "x".repeat(CACHE_BYTES / 4))),
                false,
                diagram.clone(),
            );
            assert!(cache.bytes <= CACHE_BYTES);
        }
        assert_eq!(cache.entries.len(), 3);
        let bytes = cache.bytes;
        cache.insert(Arc::from("x".repeat(CACHE_BYTES)), false, diagram);
        assert_eq!(cache.bytes, bytes);
        assert_eq!(cache.entries.len(), 3);
    }
    #[test]
    fn invalid_preparation_returns_content_free_fallback() {
        let decoder = SvgRenderer::new(Arc::new(crate::components::Assets));
        let (_, mut request) = submit(
            Arc::from("not-a-mermaid-diagram"),
            false,
            None,
            None,
            decoder,
        )
        .unwrap();
        assert!(matches!(wait(&mut request), Err(DiagramError::Invalid)));
    }
    #[test]
    fn queue_admission_is_bounded_and_cancelled_jobs_are_discardable() {
        let (tx, rx) = bounded_queue();
        let operation = Operation::new(Duration::from_secs(5));
        let decoder = SvgRenderer::new(Arc::new(crate::components::Assets));
        let make = || {
            let (reply, _) = mpsc::sync_channel(1);
            Job {
                source: Arc::from("flowchart TD\nA --> B"),
                dark: false,
                diagram: None,
                camera: None,
                operation: operation.clone(),
                decoder: decoder.clone(),
                reply,
            }
        };
        for _ in 0..QUEUE_SIZE {
            tx.try_send(make())
                .unwrap_or_else(|_| panic!("queue filled too early"));
        }
        assert!(matches!(
            tx.try_send(make()),
            Err(mpsc::TrySendError::Full(_))
        ));
        operation.cancel();
        for _ in 0..QUEUE_SIZE {
            let job = rx.try_recv().unwrap();
            assert_eq!(job.operation.checkpoint(), Err(DiagramError::Cancelled));
        }
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn cancellation_is_shared() {
        let op = Operation::new(Duration::from_secs(5));
        let copy = op.clone();
        op.cancel();
        assert_eq!(copy.checkpoint(), Err(DiagramError::Cancelled));
        assert_eq!(WORKERS, 2);
        assert_eq!(QUEUE_SIZE, 16);
    }
}

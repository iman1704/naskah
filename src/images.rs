//! Process-wide image loading and caching for the reader.
//!
//! Reading a note returns immediately even when its images are remote: the
//! reader collects the note's image targets, this module loads them on a
//! worker thread, and the event loop refreshes the reader once they arrive.
//! Drawing only ever reads the cache, so it never blocks on image I/O.
//!
//! Local images are re-read when their size or modification time changes.
//! Remote images are cached for the lifetime of the process. SVG bytes are
//! rasterized to PNG before they are stored, so every later read reuses the
//! converted image. Failed URLs are remembered briefly so broken images are
//! not retried by every open.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant, SystemTime};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use pulldown_cmark_mdcat::resources::{
    DispatchingResourceHandler, FileResourceHandler, MimeData, ResourceUrlHandler, filter_schemes,
};
use pulldown_cmark_mdcat::{markdown_options, strip_frontmatter};
use url::Url;

use crate::svg::SvgAwareResources;

/// Largest local image read for inline display.
const FILE_READ_LIMIT: u64 = 20_000_000;

/// Largest remote image download for inline display.
const REMOTE_READ_LIMIT: u64 = 10_000_000;

/// Timeout for a whole remote image download.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

/// Largest total image payload kept in memory before least-recently-used
/// entries are evicted.
const CACHE_BUDGET: usize = 128 * 1024 * 1024;

/// How long a failed URL is remembered before another prefetch retries it.
const FAILURE_TTL: Duration = Duration::from_secs(30);

/// Shared image cache and the work queue feeding the loader thread.
#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    jobs: mpsc::Sender<Url>,
    cache_budget: usize,
    failure_ttl: Duration,
    remote_enabled: AtomicBool,
}

#[derive(Debug, Default)]
struct State {
    /// Loaded images keyed by their resolved URL.
    cache: HashMap<Url, CachedImage>,
    /// URLs queued on or being processed by the worker.
    in_flight: HashSet<Url>,
    /// Failed URLs and when they failed, so broken images are not retried by
    /// every prefetch.
    failures: HashMap<Url, Instant>,
    /// Total payload bytes in `cache`.
    bytes: usize,
    /// Monotonic use counter for least-recently-used eviction.
    tick: u64,
}

#[derive(Debug)]
struct CachedImage {
    data: MimeData,
    /// Identity of a local file when it was read; `None` for remote images.
    stamp: Option<FileStamp>,
    /// Use counter of the last successful read.
    last_used: u64,
}

/// Size and modification time used to notice local image changes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

/// Handle to the process-wide image store.
///
/// Clones share one cache, one worker queue, and one completion channel. The
/// store also implements [`ResourceUrlHandler`], so it can be handed directly
/// to mdcat: cached images return their bytes and everything else reports
/// [`io::ErrorKind::WouldBlock`].
#[derive(Debug, Clone)]
pub struct ImageStore {
    shared: Arc<Shared>,
    completions: Arc<Mutex<mpsc::Receiver<Url>>>,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageStore {
    /// Create an independent store with its own worker thread.
    ///
    /// Most callers want [`ImageStore::shared`], so the whole process reuses
    /// one cache. Tests use this constructor to keep caches apart.
    pub fn new() -> Self {
        Self::with_limits(CACHE_BUDGET, FAILURE_TTL)
    }

    fn with_limits(cache_budget: usize, failure_ttl: Duration) -> Self {
        let (jobs, job_rx) = mpsc::channel();
        let (completed, completed_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            jobs,
            cache_budget,
            failure_ttl,
            remote_enabled: AtomicBool::new(true),
        });
        let worker_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("naskah-images".to_owned())
            .spawn(move || {
                let resources = SvgAwareResources::new(DispatchingResourceHandler::new(vec![
                    Box::new(FileResourceHandler::new(FILE_READ_LIMIT)),
                    Box::new(RemoteResourceHandler::new(
                        REMOTE_READ_LIMIT,
                        REMOTE_TIMEOUT,
                    )),
                ]));
                run_worker(job_rx, completed, worker_shared, resources);
            })
            .expect("spawn the image worker thread");
        Self {
            shared,
            completions: Arc::new(Mutex::new(completed_rx)),
        }
    }

    /// The process-wide store shared by every reader.
    pub fn shared() -> Self {
        static SHARED: OnceLock<ImageStore> = OnceLock::new();
        SHARED.get_or_init(Self::new).clone()
    }

    /// Allow or block remote `http`/`https` fetching.
    ///
    /// Local images are unaffected. Disabling this does not clear images
    /// already cached, and requests already queued finish normally.
    pub fn set_fetch_remote(&self, enabled: bool) {
        self.shared.remote_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Collect and queue the images referenced by `source`.
    ///
    /// Targets resolve against `base_url`, which is usually the note's
    /// directory. Cached and in-flight URLs are skipped, so calling this
    /// again after a reload only fetches what is new or stale.
    pub fn prefetch_note(&self, source: &str, base_url: Option<&Url>) {
        for url in collect_image_targets(source, base_url) {
            self.prefetch(&url);
        }
    }

    /// Queue `url` for loading unless it is cached or already in flight.
    pub fn prefetch(&self, url: &Url) {
        if !is_supported(url) {
            return;
        }
        if is_remote(url) && !self.shared.remote_enabled.load(Ordering::Relaxed) {
            return;
        }

        // Local files are validated outside the lock. Metadata is cheap and
        // prefetch is not the drawing path.
        let stamp = file_stamp(url);
        let mut state = lock(&self.shared.state);
        if let Some(cached) = state.cache.get(url) {
            let fresh = !is_local(url) || (stamp.is_some() && cached.stamp == stamp);
            if fresh {
                return;
            }
            state.remove(url);
        }
        if state
            .failures
            .get(url)
            .is_some_and(|failed_at| failed_at.elapsed() < self.shared.failure_ttl)
        {
            return;
        }
        state.failures.remove(url);
        if !state.in_flight.insert(url.clone()) {
            return;
        }
        if self.shared.jobs.send(url.clone()).is_err() {
            state.in_flight.remove(url);
        }
    }

    /// Whether any prefetched image is still loading.
    pub fn has_pending(&self) -> bool {
        !lock(&self.shared.state).in_flight.is_empty()
    }

    /// Take every image that loaded since the last call.
    ///
    /// All successful completions available at once are returned together so
    /// the caller can refresh and redraw once per batch. Failed loads are
    /// omitted because they change nothing on screen.
    pub fn drain_completions(&self) -> Vec<Url> {
        let completed = lock(&self.completions);
        let mut urls = Vec::new();
        while let Ok(url) = completed.try_recv() {
            urls.push(url);
        }
        urls
    }
}

impl ResourceUrlHandler for ImageStore {
    fn read_resource(&self, url: &Url) -> io::Result<MimeData> {
        if !is_supported(url) {
            return Err(unsupported(url));
        }
        let mut state = lock(&self.shared.state);
        match state.touch(url) {
            Some(data) => Ok(data),
            None => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("image {url} is not loaded yet"),
            )),
        }
    }
}

impl State {
    /// Update the access time of `url` and clone its payload.
    fn touch(&mut self, url: &Url) -> Option<MimeData> {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        let cached = self.cache.get_mut(url)?;
        cached.last_used = tick;
        Some(cached.data.clone())
    }

    /// Remove a cached payload and keep the byte total in sync.
    fn remove(&mut self, url: &Url) -> Option<CachedImage> {
        let cached = self.cache.remove(url)?;
        self.bytes = self.bytes.saturating_sub(cached.data.data.len());
        Some(cached)
    }

    /// Insert a payload, replacing anything cached for the same URL and
    /// evicting least-recently-used entries over `budget`.
    fn insert(&mut self, url: Url, data: MimeData, stamp: Option<FileStamp>, budget: usize) {
        self.remove(&url);
        self.tick = self.tick.wrapping_add(1);
        let len = data.data.len();
        self.bytes = self.bytes.saturating_add(len);
        let last_used = self.tick;
        self.cache.insert(
            url,
            CachedImage {
                data,
                stamp,
                last_used,
            },
        );
        self.evict(budget);
    }

    fn evict(&mut self, budget: usize) {
        // Keep at least one image even when a single payload exceeds the
        // budget; otherwise nothing could ever be displayed.
        while self.bytes > budget && self.cache.len() > 1 {
            let Some(least) = self
                .cache
                .iter()
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(url, _)| url.clone())
            else {
                break;
            };
            self.remove(&least);
        }
    }
}

/// Load queued URLs until every [`ImageStore`] handle is dropped.
fn run_worker(
    jobs: mpsc::Receiver<Url>,
    completions: mpsc::Sender<Url>,
    shared: Arc<Shared>,
    resources: SvgAwareResources,
) {
    while let Ok(url) = jobs.recv() {
        let loaded = load(&resources, &url);
        let mut state = lock(&shared.state);
        match loaded {
            Ok((data, stamp)) => {
                state.insert(url.clone(), data, stamp, shared.cache_budget);
                state.failures.remove(&url);
                // Report successful loads only; a failed image changes
                // nothing on screen, so it does not need a refresh.
                if completions.send(url.clone()).is_err() {
                    break;
                }
            }
            Err(_) => {
                state.failures.insert(url.clone(), Instant::now());
            }
        }
        state.in_flight.remove(&url);
    }
}

/// Read and, for SVG, rasterize one image. The returned stamp identifies the
/// local file at read time so a later prefetch can notice a change.
fn load(resources: &SvgAwareResources, url: &Url) -> io::Result<(MimeData, Option<FileStamp>)> {
    if !is_supported(url) {
        return Err(unsupported(url));
    }
    let data = resources.read_resource(url)?;
    Ok((data, file_stamp(url)))
}

/// Resolve `target` against the note's base URL. Returns `None` for targets
/// that cannot be parsed.
pub(crate) fn resolve_reference(base_url: Option<&Url>, target: &str) -> Option<Url> {
    match base_url {
        Some(base) => Url::parse(target).or_else(|_| base.join(target)).ok(),
        None => Url::parse(target).ok(),
    }
}

/// One `![alt](target)` image found in note source.
#[derive(Debug)]
pub(crate) struct ImageAsset {
    pub target: String,
    pub alt: String,
}

/// Collect image assets in document order, skipping frontmatter and code.
pub(crate) fn collect_assets(source: &str) -> Vec<ImageAsset> {
    let body = strip_frontmatter(source);
    let mut images = Vec::new();
    let mut current: Option<ImageAsset> = None;
    for event in Parser::new_ext(body, markdown_options(false)) {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                current = Some(ImageAsset {
                    target: dest_url.to_string(),
                    alt: String::new(),
                });
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(image) = &mut current {
                    image.alt.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(image) = &mut current {
                    image.alt.push(' ');
                }
            }
            Event::End(TagEnd::Image) => {
                if let Some(image) = current.take() {
                    images.push(image);
                }
            }
            _ => {}
        }
    }
    images
}

/// Image targets in document order, resolved against `base_url` and deduped.
///
/// Frontmatter is skipped and code blocks contribute nothing, matching what
/// mdcat renders.
pub fn collect_image_targets(source: &str, base_url: Option<&Url>) -> Vec<Url> {
    let mut targets: Vec<Url> = Vec::new();
    for asset in collect_assets(source) {
        if let Some(url) = resolve_reference(base_url, &asset.target)
            && !targets.contains(&url)
        {
            targets.push(url);
        }
    }
    targets
}

/// Image schemes this store knows how to load.
fn is_supported(url: &Url) -> bool {
    is_local(url) || is_remote(url)
}

fn is_local(url: &Url) -> bool {
    url.scheme() == "file"
}

fn is_remote(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

fn unsupported(url: &Url) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("unsupported image URL {url}"),
    )
}

fn file_stamp(url: &Url) -> Option<FileStamp> {
    if url.scheme() != "file" {
        return None;
    }
    let metadata = std::fs::metadata(url.to_file_path().ok()?).ok()?;
    Some(FileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Reads `http`/`https` image URLs with a timeout and a size cap.
///
/// Anything else reports `Unsupported` so the dispatcher can try another
/// handler. Caching lives in the store, so this handler always downloads.
#[derive(Debug, Clone)]
struct RemoteResourceHandler {
    read_limit: u64,
    agent: ureq::Agent,
}

impl RemoteResourceHandler {
    fn new(read_limit: u64, timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        Self { read_limit, agent }
    }
}

impl ResourceUrlHandler for RemoteResourceHandler {
    fn read_resource(&self, url: &Url) -> io::Result<MimeData> {
        filter_schemes(&["http", "https"], url)?;
        let mut response = self
            .agent
            .get(url.as_str())
            .call()
            .map_err(io::Error::other)?;
        let mut data = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(self.read_limit + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > self.read_limit {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!("contents of {url} exceeded {} bytes", self.read_limit),
            ));
        }
        Ok(MimeData {
            mime_type: None,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{SocketAddr, TcpListener};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn wait_until_idle(store: &ImageStore) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while store.has_pending() {
            assert!(
                Instant::now() < deadline,
                "image workers did not finish in time"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn write_png(path: &Path, width: u32, height: u32) {
        let picture = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        picture.save(path).expect("write png");
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let picture = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode png");
        png
    }

    fn file_url(path: &Path) -> Url {
        Url::from_file_path(path).expect("file URL")
    }

    #[test]
    fn drawing_sees_a_miss_until_the_worker_loads_the_file() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("pic.png");
        write_png(&path, 20, 40);
        let store = ImageStore::new();
        let url = file_url(&path);

        let miss = store.read_resource(&url).expect_err("image is not loaded");
        assert_eq!(miss.kind(), io::ErrorKind::WouldBlock);

        store.prefetch(&url);
        wait_until_idle(&store);

        let hit = store.read_resource(&url).expect("image is loaded");
        assert_eq!(hit.data, std::fs::read(&path).expect("read png"));
    }

    #[test]
    fn changed_local_files_are_reloaded() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("pic.png");
        write_png(&path, 20, 40);
        let store = ImageStore::new();
        let url = file_url(&path);
        store.prefetch(&url);
        wait_until_idle(&store);
        let first = store.read_resource(&url).expect("first load").data;

        write_png(&path, 30, 40);
        store.prefetch(&url);
        wait_until_idle(&store);
        let second = store.read_resource(&url).expect("second load").data;

        assert_ne!(first, second);
        assert_eq!(second, std::fs::read(&path).expect("read png"));
    }

    #[test]
    fn cached_svg_is_stored_as_png() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("pic.svg");
        std::fs::write(
            &path,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"40\">\
             <rect width=\"20\" height=\"40\" fill=\"red\"/></svg>",
        )
        .expect("write svg");
        let store = ImageStore::new();
        let url = file_url(&path);

        store.prefetch(&url);
        wait_until_idle(&store);
        let cached = store.read_resource(&url).expect("cached svg");

        assert_eq!(cached.mime_type_essence(), Some("image/png"));
        assert!(cached.data.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn prefetching_an_in_flight_url_starts_one_request() {
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let addr = serve_counting(
            png_response(png_bytes(20, 40)),
            Arc::clone(&requests),
            Arc::clone(&stop),
        );
        let store = ImageStore::new();
        let url = Url::parse(&format!("http://{addr}/pic.png")).expect("parse url");

        store.prefetch(&url);
        store.prefetch(&url);
        wait_until_idle(&store);
        std::thread::sleep(Duration::from_millis(20));
        stop.store(true, Ordering::SeqCst);

        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(store.read_resource(&url).is_ok());
    }

    #[test]
    fn prefetching_a_cached_url_skips_the_request() {
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let addr = serve_counting(
            png_response(png_bytes(20, 40)),
            Arc::clone(&requests),
            Arc::clone(&stop),
        );
        let store = ImageStore::new();
        let url = Url::parse(&format!("http://{addr}/pic.png")).expect("parse url");

        store.prefetch(&url);
        wait_until_idle(&store);
        store.prefetch(&url);
        std::thread::sleep(Duration::from_millis(20));
        stop.store(true, Ordering::SeqCst);

        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(!store.has_pending());
    }

    #[test]
    fn collects_image_targets_and_skips_code() {
        let base = Url::parse("file:///notes/").expect("base url");
        let source = "![One](a.png)\n\n```\n![Fake](b.png)\n```\n\n![Two](a.png)\n";

        let targets = collect_image_targets(source, Some(&base));

        assert_eq!(targets, [Url::parse("file:///notes/a.png").expect("url"),]);
    }

    #[test]
    fn unsupported_schemes_are_rejected_by_the_handler() {
        let store = ImageStore::new();
        let url = Url::parse("ftp://example.com/pic.png").expect("parse url");

        let error = store.read_resource(&url).expect_err("must reject");

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn remote_handler_rejects_non_http_schemes() {
        let handler = RemoteResourceHandler::new(REMOTE_READ_LIMIT, REMOTE_TIMEOUT);
        let url = Url::parse("file:///notes/pic.png").expect("parse url");

        let error = handler.read_resource(&url).expect_err("must reject");

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn evicts_least_recently_used_images_over_budget() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let mut urls = Vec::new();
        let mut size = 0;
        for name in ["a.png", "b.png", "c.png"] {
            let path = temp.path().join(name);
            write_png(&path, 20, 40);
            size = std::fs::read(&path).expect("read png").len();
            urls.push(file_url(&path));
        }
        let store = ImageStore::with_limits(size * 2, FAILURE_TTL);
        let (a, b, c) = (&urls[0], &urls[1], &urls[2]);

        store.prefetch(a);
        wait_until_idle(&store);
        store.prefetch(b);
        wait_until_idle(&store);
        // A is now more recently used than B.
        store.read_resource(a).expect("read a");
        store.prefetch(c);
        wait_until_idle(&store);

        assert!(store.read_resource(a).is_ok());
        assert!(store.read_resource(c).is_ok());
        assert_eq!(
            store.read_resource(b).expect_err("b was evicted").kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn failed_urls_are_not_retried_before_the_ttl() {
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let addr = serve_counting(failure_response(), Arc::clone(&requests), Arc::clone(&stop));
        let store = ImageStore::with_limits(CACHE_BUDGET, Duration::from_millis(50));
        let url = Url::parse(&format!("http://{addr}/broken.png")).expect("parse url");

        store.prefetch(&url);
        wait_until_idle(&store);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        // A failed image does not need a redraw.
        assert!(store.drain_completions().is_empty());

        store.prefetch(&url);
        wait_until_idle(&store);
        assert_eq!(requests.load(Ordering::SeqCst), 1);

        std::thread::sleep(Duration::from_millis(80));
        store.prefetch(&url);
        wait_until_idle(&store);
        stop.store(true, Ordering::SeqCst);

        assert_eq!(requests.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn remote_fetching_can_be_disabled_without_affecting_local_images() {
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let addr = serve_counting(
            png_response(png_bytes(20, 40)),
            Arc::clone(&requests),
            Arc::clone(&stop),
        );
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("local.png");
        write_png(&path, 20, 40);
        let store = ImageStore::new();
        store.set_fetch_remote(false);

        let remote = Url::parse(&format!("http://{addr}/pic.png")).expect("parse url");
        store.prefetch(&remote);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        assert!(!store.has_pending());

        let local = file_url(&path);
        store.prefetch(&local);
        wait_until_idle(&store);
        assert!(store.read_resource(&local).is_ok());
        stop.store(true, Ordering::SeqCst);
    }

    fn png_response(png: Vec<u8>) -> Vec<u8> {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            png.len()
        );
        let mut response = header.into_bytes();
        response.extend(png);
        response
    }

    fn failure_response() -> Vec<u8> {
        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec()
    }

    /// Serve `response` for every request until `stop` is set, counting requests.
    fn serve_counting(
        response: Vec<u8>,
        requests: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
    ) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener
            .set_nonblocking(true)
            .expect("non-blocking listener");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        requests.fetch_add(1, Ordering::SeqCst);
                        let mut request = [0u8; 4096];
                        let _ = stream.read(&mut request);
                        let _ = stream.write_all(&response);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        addr
    }
}

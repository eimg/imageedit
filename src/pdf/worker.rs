//! PDF parsing and rasterization run in a disposable process. Cancel kills and
//! reaps it, including while the PDF library is inside an uninterruptible call.
use super::{PdfBackground, PdfControl, PdfInfo, PdfOptions, PdfProgress, PdfSource};
use anyhow::{Context, Result, anyhow, ensure};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const FLAG: &str = "--lime-pdf-worker";
const MAX_REPLY: u64 = 2 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
enum Request {
    Inspect,
    Pages(PdfOptions),
    Thumbnails {
        pages: Vec<usize>,
        background: PdfBackground,
    },
}
#[derive(Serialize, Deserialize)]
struct Page {
    number: usize,
    width: u32,
    height: u32,
    error: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Reply {
    info: Option<PdfInfo>,
    pages: Vec<Page>,
    error: Option<String>,
}

pub(super) fn enabled() -> bool {
    !cfg!(test)
}

/// Recognized before GPUI initialization so the worker never opens a window.
pub fn run_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(FLAG)) {
        return false;
    }
    let result = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("Missing PDF worker directory"))
        .and_then(|dir| {
            ensure!(args.next().is_none(), "Invalid PDF worker arguments");
            #[cfg(target_os = "windows")]
            let _limits = resource_limits()?;
            #[cfg(not(target_os = "windows"))]
            resource_limits()?;
            let result = std::panic::catch_unwind(|| execute(&dir))
                .unwrap_or_else(|_| Err(anyhow!("The PDF renderer stopped unexpectedly.")));
            let reply = match result {
                Ok(reply) => reply,
                Err(error) => Reply {
                    info: None,
                    pages: vec![],
                    error: Some(format!("{error:#}")),
                },
            };
            atomic_json(&dir.join("reply.json"), &reply)
        });
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing output directory")?)?;
    serde_json::to_writer(&mut file, value)?;
    file.flush()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}
fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    let file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= MAX_REPLY,
        "PDF worker metadata exceeded its limit"
    );
    Ok(serde_json::from_reader(file.take(MAX_REPLY + 1))?)
}
fn write_page(dir: &Path, number: usize, image: &RgbaImage) -> Result<Page> {
    super::Document::validate_size(image.width(), image.height())?;
    let mut file = fs::File::create(dir.join(format!("page-{number}.rgba")))?;
    file.write_all(image.as_raw())?;
    Ok(Page {
        number,
        width: image.width(),
        height: image.height(),
        error: None,
    })
}
fn execute(dir: &Path) -> Result<Reply> {
    let request: Request = read_json(&dir.join("request.json"))?;
    let control = PdfControl::default();
    let bytes = super::read_pdf(&dir.join("source.pdf"), &control)?;
    let data = hayro::hayro_syntax::PdfData::from(bytes);
    let mut reply = Reply {
        info: None,
        pages: vec![],
        error: None,
    };
    match request {
        Request::Inspect => {
            reply.info = Some(super::info_from_pdf(&super::load_pdf(data)?, &control)?);
        }
        Request::Pages(options) => {
            super::render_each(
                data,
                &options,
                &control,
                &|progress| {
                    let _ = atomic_json(&dir.join("progress.json"), &progress);
                },
                |number, image| {
                    reply.pages.push(write_page(dir, number, &image)?);
                    Ok(())
                },
            )?;
        }
        Request::Thumbnails { pages, background } => {
            ensure!(
                pages.len() <= super::thumbnails::THUMBNAIL_BATCH,
                "Too many page previews requested"
            );
            let info = super::info_from_pdf(&super::load_pdf(data.clone())?, &control)?;
            super::validate_page_numbers(&pages, info.pages)?;
            let source = PdfSource {
                data,
                info,
                render_lock: Default::default(),
            };
            for thumbnail in super::thumbnails::render_local(&source, &pages, background, &control)?
            {
                reply.pages.push(match thumbnail.pixels {
                    Ok(image) => write_page(dir, thumbnail.page, &image)?,
                    Err(error) => Page {
                        number: thumbnail.page,
                        width: 0,
                        height: 0,
                        error: Some(error),
                    },
                });
            }
        }
    }
    Ok(reply)
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(
    data: &[u8],
    request: &Request,
    control: &PdfControl,
    progress: &impl Fn(PdfProgress),
    deadline: Duration,
) -> Result<(tempfile::TempDir, Reply)> {
    control.check()?;
    let dir = tempfile::Builder::new().prefix("lime-pdf-").tempdir()?;
    let mut source = fs::File::create(dir.path().join("source.pdf"))?;
    for chunk in data.chunks(64 * 1024) {
        control.check()?;
        source.write_all(chunk)?;
    }
    source.flush()?;
    drop(source);
    atomic_json(&dir.path().join("request.json"), request)?;
    let mut child = Running(
        Command::new(std::env::current_exe()?)
            .arg(FLAG)
            .arg(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Could not start the PDF renderer")?,
    );
    wait_child(&mut child, dir.path(), control, progress, deadline)?;
    control.check()?;
    let reply: Reply =
        read_json(&dir.path().join("reply.json")).context("Invalid PDF renderer response")?;
    if let Some(error) = &reply.error {
        return Err(anyhow!(error.clone()));
    }
    Ok((dir, reply))
}
fn wait_child(
    child: &mut Running,
    dir: &Path,
    control: &PdfControl,
    progress: &impl Fn(PdfProgress),
    deadline: Duration,
) -> Result<()> {
    let start = Instant::now();
    let mut previous = None;
    loop {
        control.check()?;
        ensure!(
            start.elapsed() <= deadline,
            "The PDF renderer exceeded its time limit. Try fewer pages or a lower DPI."
        );
        if let Ok(update) = read_json::<PdfProgress>(&dir.join("progress.json"))
            && previous != Some(update)
        {
            progress(update);
            previous = Some(update);
        }
        if let Some(status) = child.0.try_wait()? {
            ensure!(
                status.success(),
                "The PDF renderer stopped unexpectedly. Try fewer pages or a lower DPI."
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn read_page(dir: &Path, page: &Page, budget: &mut u64, control: &PdfControl) -> Result<RgbaImage> {
    control.check()?;
    super::Document::validate_size(page.width, page.height)?;
    let bytes = u64::from(page.width) * u64::from(page.height) * 4;
    *budget = budget
        .checked_sub(bytes)
        .context("PDF renderer output exceeded its memory limit")?;
    let path = dir.join(format!("page-{}.rgba", page.number));
    let mut file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() == bytes,
        "Incomplete PDF renderer output"
    );
    let mut pixels = Vec::with_capacity(bytes as usize);
    let mut chunk = [0; 64 * 1024];
    loop {
        control.check()?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        pixels.extend_from_slice(&chunk[..count]);
    }
    RgbaImage::from_raw(page.width, page.height, pixels).context("Invalid PDF renderer pixels")
}
pub(super) fn inspect(
    data: &hayro::hayro_syntax::PdfData,
    control: &PdfControl,
) -> Result<PdfInfo> {
    let (_, reply) = run(
        data.as_ref(),
        &Request::Inspect,
        control,
        &|_| {},
        Duration::from_secs(30),
    )?;
    let info = reply
        .info
        .context("The PDF renderer returned no page information")?;
    ensure!(
        !info.sizes.is_empty()
            && info.sizes.len() == info.pages
            && info.pages <= super::MAX_PDF_PAGES,
        "Invalid PDF page information"
    );
    Ok(info)
}
pub(super) fn pages(
    source: &PdfSource,
    options: &PdfOptions,
    control: &PdfControl,
    progress: &impl Fn(PdfProgress),
) -> Result<Vec<RgbaImage>> {
    let sizes = super::validate_options(&source.info, options)?;
    let timeout = Duration::from_secs((options.pages.len() as u64 * 60).clamp(60, 600));
    // Rendering consumes the retained bytes, not the original path. Keep
    // non-UTF-8 native filenames out of the JSON protocol.
    let mut request_options = options.clone();
    request_options.path = PathBuf::new();
    let (dir, reply) = run(
        source.data.as_ref(),
        &Request::Pages(request_options),
        control,
        progress,
        timeout,
    )?;
    ensure!(
        reply.pages.len() == options.pages.len(),
        "The PDF renderer returned an incomplete page selection"
    );
    let mut budget = super::MAX_STACK_BYTES;
    reply
        .pages
        .iter()
        .zip(&options.pages)
        .zip(sizes)
        .map(|((page, number), size)| {
            ensure!(
                page.number == *number && (page.width, page.height) == size && page.error.is_none(),
                "The PDF renderer returned an unexpected page"
            );
            read_page(dir.path(), page, &mut budget, control)
        })
        .collect()
}
pub(super) fn thumbnails(
    source: &PdfSource,
    pages: &[usize],
    background: PdfBackground,
    control: &PdfControl,
) -> Result<Vec<super::thumbnails::PdfThumbnail>> {
    let (dir, reply) = run(
        source.data.as_ref(),
        &Request::Thumbnails {
            pages: pages.to_vec(),
            background,
        },
        control,
        &|_| {},
        Duration::from_secs(30),
    )?;
    ensure!(
        reply.pages.len() == pages.len(),
        "Incomplete PDF page previews"
    );
    let mut budget = u64::from(super::thumbnails::THUMBNAIL_EDGE).pow(2)
        * 4
        * super::thumbnails::THUMBNAIL_BATCH as u64;
    reply
        .pages
        .iter()
        .zip(pages)
        .map(|(page, number)| {
            ensure!(
                page.number == *number
                    && page.width <= super::thumbnails::THUMBNAIL_EDGE
                    && page.height <= super::thumbnails::THUMBNAIL_EDGE,
                "Unexpected PDF page preview"
            );
            let pixels = if let Some(error) = &page.error {
                Err(error.clone())
            } else {
                read_page(dir.path(), page, &mut budget, control).map_err(|error| error.to_string())
            };
            Ok(super::thumbnails::PdfThumbnail {
                page: *number,
                pixels,
            })
        })
        .collect()
}

// macOS exposes RLIMIT_AS as RSS, which is not a reliable address-space cap.
// Limit CPU/output in the kernel and use a resident high-water watchdog there.
#[cfg(unix)]
fn resource_limits() -> Result<()> {
    let limits = [
        (libc::RLIMIT_CPU, 120),
        (libc::RLIMIT_FSIZE, super::MAX_STACK_BYTES),
    ];
    for (kind, value) in limits {
        let limit = libc::rlimit {
            rlim_cur: value as libc::rlim_t,
            rlim_max: value as libc::rlim_t,
        };
        // SAFETY: setrlimit copies a properly initialized value in this worker only.
        ensure!(
            unsafe { libc::setrlimit(kind, &limit) } == 0,
            "Could not constrain the PDF worker"
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let memory = libc::rlimit {
            rlim_cur: 2 * 1024 * 1024 * 1024,
            rlim_max: 2 * 1024 * 1024 * 1024,
        };
        ensure!(
            unsafe { libc::setrlimit(libc::RLIMIT_AS, &memory) } == 0,
            "Could not constrain PDF memory"
        );
    }
    #[cfg(target_os = "macos")]
    std::thread::spawn(|| {
        loop {
            // SAFETY: getrusage writes exactly one initialized, correctly sized record.
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0
                && usage.ru_maxrss > 2 * 1024 * 1024 * 1024
            {
                std::process::exit(137);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    Ok(())
}
#[cfg(target_os = "windows")]
struct WorkerJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(target_os = "windows")]
impl Drop for WorkerJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(target_os = "windows")]
fn resource_limits() -> Result<WorkerJob> {
    use windows_sys::Win32::System::{JobObjects::*, Threading::GetCurrentProcess};
    // SAFETY: a private unnamed job owns this worker only; no borrowed pointers
    // outlive the calls. Closing its handle releases the OS object.
    unsafe {
        let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        ensure!(!handle.is_null(), "Could not create PDF worker limits");
        let job = WorkerJob(handle);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_PROCESS_TIME;
        limits.BasicLimitInformation.PerProcessUserTimeLimit = 120 * 10_000_000;
        limits.ProcessMemoryLimit = 2 * 1024 * 1024 * 1024;
        ensure!(
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32
            ) != 0
                && AssignProcessToJobObject(handle, GetCurrentProcess()) != 0,
            "Could not constrain the PDF worker"
        );
        Ok(job)
    }
}
#[cfg(not(any(unix, target_os = "windows")))]
fn resource_limits() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_wait_fixture() {
        if std::env::var_os("LIME_TEST_PDF_WAIT").is_some() {
            std::thread::sleep(Duration::from_secs(20));
        }
    }
    fn sleeping_child() -> Running {
        Running(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "pdf::worker::tests::child_wait_fixture",
                    "--nocapture",
                ])
                .env("LIME_TEST_PDF_WAIT", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    #[test]
    fn cancellation_and_deadline_terminate_and_reap_a_running_worker() {
        for canceled in [true, false] {
            let mut child = sleeping_child();
            let control = PdfControl::default();
            let request = control.clone();
            let trigger = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                if canceled {
                    request.cancel();
                }
            });
            let dir = tempfile::tempdir().unwrap();
            let start = Instant::now();
            let result = wait_child(
                &mut child,
                dir.path(),
                &control,
                &|_| {},
                if canceled {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(100)
                },
            );
            assert!(result.is_err());
            if canceled {
                assert!(result.unwrap_err().is::<super::super::PdfCanceled>());
            }
            drop(child);
            trigger.join().unwrap();
            assert!(start.elapsed() < Duration::from_secs(3));
        }
    }
}

//! Release discovery only. Downloads and installation always stay with the user.
mod ui;

use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, bail};
use gpui::{App, Global};
use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;

pub(crate) use ui::{badge, show};

const API: &str = "https://api.github.com/repos/eimg/lime/releases/latest";
const RELEASES: &str = "https://github.com/eimg/lime/releases";
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_BODY: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Clone, Debug)]
struct Available {
    version: String,
    changelog: String,
    download: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
struct Cache {
    checked_at: u64,
    #[serde(default)]
    succeeded: bool,
    release: Option<Release>,
}

#[derive(Default)]
enum CheckStatus {
    #[default]
    Unchecked,
    Current,
    Failed,
}

#[derive(Default)]
struct UpdateState {
    checking: bool,
    available: Option<Available>,
    status: CheckStatus,
}
impl Global for UpdateState {}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(UpdateState::default());
    // Updates are rare; notifying windows here avoids adding state to the editor.
    cx.observe_global::<UpdateState>(|cx| cx.refresh_windows())
        .detach();
    cx.spawn(async |cx| {
        cx.background_executor()
            .timer(Duration::from_secs(15))
            .await;
        loop {
            if cx.update(|cx| check(false, cx)).is_err() {
                break;
            }
            cx.background_executor().timer(INTERVAL).await;
        }
    })
    .detach();
}

pub(crate) fn check(force: bool, cx: &mut App) {
    if cx.global::<UpdateState>().checking {
        return;
    }
    cx.global_mut::<UpdateState>().checking = true;
    let job = cx
        .background_executor()
        .spawn(async move { run_check(force, cache_path(), now()) });
    cx.spawn(async move |cx| {
        let (release, status) = job.await;
        let _ = cx.update_global::<UpdateState, _>(|state, _| {
            state.checking = false;
            state.status = status;
            if let Some(release) = release {
                state.available = release
                    .available(
                        env!("CARGO_PKG_VERSION"),
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                    )
                    .ok()
                    .flatten();
            }
        });
    })
    .detach();
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn due(checked_at: u64, now: u64) -> bool {
    checked_at == 0
        || now
            .checked_sub(checked_at)
            .is_none_or(|age| age >= INTERVAL.as_secs())
}

fn cache_path() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(|root| PathBuf::from(root).join("Lime/Cache/update-check.json"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|root| PathBuf::from(root).join("Library/Caches/com.eimg.lime/update-check.json"))
    } else if cfg!(target_os = "linux") {
        linux_cache_path(std::env::var_os("XDG_CACHE_HOME"), std::env::var_os("HOME"))
    } else {
        None
    }
}

fn linux_cache_path(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    xdg.map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".cache"))
        })
        .map(|path| path.join("lime/update-check.json"))
}

fn run_check(force: bool, path: Option<PathBuf>, timestamp: u64) -> (Option<Release>, CheckStatus) {
    run_check_with(force, path, timestamp, fetch)
}

fn run_check_with(
    force: bool,
    path: Option<PathBuf>,
    timestamp: u64,
    fetch: impl FnOnce() -> Result<Release>,
) -> (Option<Release>, CheckStatus) {
    let mut cache = path
        .as_ref()
        .and_then(|path| {
            let mut data = Vec::new();
            fs::File::open(path)
                .ok()?
                .take(MAX_BODY + 1)
                .read_to_end(&mut data)
                .ok()?;
            (data.len() as u64 <= MAX_BODY)
                .then(|| serde_json::from_slice::<Cache>(&data).ok())
                .flatten()
        })
        .unwrap_or_default();
    // Validate cached versions as strictly as a network response.
    if cache
        .release
        .as_ref()
        .is_some_and(|release| release.stable_version().is_err())
    {
        cache = Cache::default();
    }
    if !force && !due(cache.checked_at, timestamp) {
        let status = if cache.succeeded {
            CheckStatus::Current
        } else {
            CheckStatus::Failed
        };
        return (cache.release, status);
    }
    let status = match fetch() {
        Ok(release) => {
            cache.release = Some(release);
            CheckStatus::Current
        }
        Err(error) => {
            eprintln!("Could not check for Lime updates: {error:#}");
            CheckStatus::Failed
        }
    };
    // Throttle failed automatic attempts too, including across application restarts.
    cache.checked_at = timestamp;
    cache.succeeded = matches!(status, CheckStatus::Current);
    if let Some(path) = path {
        let write = || -> Result<()> {
            let parent = path.parent().context("missing cache directory")?;
            fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer(file.as_file_mut(), &cache)?;
            file.persist(&path)?;
            Ok(())
        };
        if let Err(error) = write() {
            eprintln!("Could not save update-check cache: {error:#}");
        }
    }
    (cache.release, status)
}

fn fetch() -> Result<Release> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Lime/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()?;
    let response = client
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()?
        .error_for_status()?;
    if !response.status().is_success() {
        bail!("Unexpected release response: {}", response.status());
    }
    let mut data = Vec::new();
    response.take(MAX_BODY + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_BODY {
        bail!("Release metadata exceeds its size limit");
    }
    let release: Release = serde_json::from_slice(&data)?;
    release.stable_version()?;
    Ok(release)
}

impl Release {
    fn stable_version(&self) -> Result<Version> {
        let version = Version::parse(self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name))?;
        if self.draft || self.prerelease || !version.pre.is_empty() {
            bail!("Release is not a published stable version");
        }
        Ok(version)
    }

    fn available(&self, current: &str, os: &str, arch: &str) -> Result<Option<Available>> {
        let version = self.stable_version()?;
        if !version.cmp_precedence(&Version::parse(current)?).is_gt() {
            return Ok(None);
        }
        let mut changelog = Url::parse(RELEASES)?;
        changelog
            .path_segments_mut()
            .unwrap()
            .push("tag")
            .push(&self.tag_name);
        let version = version.to_string();
        let names: Vec<String> = match (os, arch) {
            ("macos", "aarch64") => vec![
                format!("Lime-{version}-macos-arm64.dmg"),
                format!("Lime-{version}-macos-aarch64.dmg"),
                format!("Lime-{version}.dmg"),
            ],
            ("macos", "x86_64") => vec![
                format!("Lime-{version}-macos-x64.dmg"),
                format!("Lime-{version}-macos-x86_64.dmg"),
            ],
            ("windows", "x86_64") => vec![
                format!("Lime-{version}-windows-x64.exe"),
                format!("Lime-{version}-windows-x64.zip"),
            ],
            ("linux", "x86_64" | "aarch64") => vec![
                format!("Lime-{version}-linux-{arch}.tar.gz"),
                format!("Lime-{version}-linux-{arch}"),
            ],
            _ => Vec::new(),
        };
        let download = names.iter().find_map(|name| {
            self.assets.iter().find_map(|asset| {
                if &asset.name != name || asset.state != "uploaded" {
                    return None;
                }
                let mut expected = Url::parse(RELEASES).ok()?;
                expected
                    .path_segments_mut()
                    .ok()?
                    .push("download")
                    .push(&self.tag_name)
                    .push(name);
                // Never open a URL supplied by release metadata outside this repository.
                (Url::parse(&asset.browser_download_url).ok()? == expected)
                    .then(|| expected.to_string())
            })
        });
        Ok(Some(Available {
            version,
            changelog: changelog.into(),
            download,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> Release {
        let tag = format!("v{version}");
        Release {
            tag_name: tag.clone(),
            draft: false,
            prerelease: false,
            assets: [
                format!("Lime-{version}.dmg"),
                format!("Lime-{version}-windows-x64.exe"),
                format!("Lime-{version}-windows-x64.zip"),
                format!("Lime-{version}-linux-x86_64.tar.gz"),
                format!("Lime-{version}-linux-aarch64.tar.gz"),
            ]
            .into_iter()
            .map(|name| Asset {
                browser_download_url: format!("{RELEASES}/download/{tag}/{name}"),
                name,
                state: "uploaded".into(),
            })
            .collect(),
        }
    }

    #[test]
    fn versions_use_semver_precedence_and_only_stable_releases() {
        for version in ["1.0.0", "1.0.1", "1.0.1+rebuilt"] {
            assert!(
                release(version)
                    .available("1.0.1", "macos", "aarch64")
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            release("1.10.0")
                .available("1.9.0", "macos", "aarch64")
                .unwrap()
                .is_some()
        );
        assert!(
            release("1.0.1")
                .available("1.0.1-beta.1", "macos", "aarch64")
                .unwrap()
                .is_some()
        );
        for version in ["bad", "2.0.0-beta.1"] {
            assert!(release(version).stable_version().is_err());
        }
        let mut candidate = release("2.0.0");
        candidate.draft = true;
        assert!(candidate.stable_version().is_err());
        candidate.draft = false;
        candidate.prerelease = true;
        assert!(candidate.stable_version().is_err());
    }

    #[test]
    fn downloads_match_platform_architecture_and_published_asset() {
        let mut candidate = release("2.0.0");
        let get = |r: &Release, os, arch| r.available("1.0.1", os, arch).unwrap().unwrap();
        assert!(
            get(&candidate, "macos", "aarch64")
                .download
                .unwrap()
                .ends_with(".dmg")
        );
        assert!(
            get(&candidate, "windows", "x86_64")
                .download
                .unwrap()
                .ends_with(".exe")
        );
        for (os, arch) in [
            ("macos", "x86_64"),
            ("windows", "aarch64"),
            ("linux", "arm"),
        ] {
            assert!(get(&candidate, os, arch).download.is_none());
        }
        candidate.assets.remove(1);
        assert!(
            get(&candidate, "windows", "x86_64")
                .download
                .unwrap()
                .ends_with(".zip")
        );
        candidate.assets[0].state = "new".into();
        assert!(get(&candidate, "macos", "aarch64").download.is_none());
    }

    #[test]
    fn linux_downloads_match_architecture_and_fall_back_to_raw_binaries() {
        let mut candidate = release("2.0.0");
        for arch in ["x86_64", "aarch64"] {
            let expected = format!("Lime-2.0.0-linux-{arch}.tar.gz");
            assert!(
                candidate
                    .available("1.0.0", "linux", arch)
                    .unwrap()
                    .unwrap()
                    .download
                    .unwrap()
                    .ends_with(&expected)
            );
        }
        candidate
            .assets
            .retain(|asset| !asset.name.ends_with("aarch64.tar.gz"));
        assert!(
            candidate
                .available("1.0.0", "linux", "aarch64")
                .unwrap()
                .unwrap()
                .download
                .is_none()
        );
        let name = "Lime-2.0.0-linux-aarch64";
        candidate.assets.push(Asset {
            name: name.into(),
            browser_download_url: format!("{RELEASES}/download/v2.0.0/{name}"),
            state: "uploaded".into(),
        });
        assert!(
            candidate
                .available("1.0.0", "linux", "aarch64")
                .unwrap()
                .unwrap()
                .download
                .unwrap()
                .ends_with(name)
        );
        candidate.assets.last_mut().unwrap().browser_download_url =
            "https://evil.example/linux".into();
        assert!(
            candidate
                .available("1.0.0", "linux", "aarch64")
                .unwrap()
                .unwrap()
                .download
                .is_none()
        );
    }

    #[test]
    fn linux_update_cache_follows_xdg_and_rejects_relative_roots() {
        let path = |xdg: Option<&str>, home: Option<&str>| {
            linux_cache_path(xdg.map(Into::into), home.map(Into::into))
        };
        assert_eq!(
            path(Some("/cache"), Some("/home/test")),
            Some(PathBuf::from("/cache/lime/update-check.json"))
        );
        assert_eq!(
            path(None, Some("/home/test")),
            Some(PathBuf::from("/home/test/.cache/lime/update-check.json"))
        );
        assert_eq!(
            path(Some("relative"), Some("/home/test")),
            path(None, Some("/home/test"))
        );
        assert_eq!(path(Some(""), Some("relative")), None);
        assert_eq!(path(None, None), None);
    }

    #[test]
    fn metadata_cannot_redirect_the_download_to_another_location() {
        let mut candidate = release("2.0.0");
        for url in [
            "http://github.com/eimg/lime/releases/download/v2.0.0/Lime-2.0.0.dmg",
            "https://evil.example/Lime-2.0.0.dmg",
            "file:///tmp/Lime-2.0.0.dmg",
            "https://github.com/other/lime/releases/download/v2.0.0/Lime-2.0.0.dmg",
            "https://github.com/eimg/lime/releases/download/v2.0.0/Lime-2.0.0.dmg?redirect=1",
        ] {
            candidate.assets[0].browser_download_url = url.into();
            assert!(
                candidate
                    .available("1.0.1", "macos", "aarch64")
                    .unwrap()
                    .unwrap()
                    .download
                    .is_none()
            );
        }
    }

    #[test]
    fn automatic_checks_are_throttled_across_restarts_and_clock_changes() {
        assert!(due(0, 100));
        assert!(!due(100, 101));
        assert!(!due(100, 100 + INTERVAL.as_secs() - 1));
        assert!(due(100, 100 + INTERVAL.as_secs()));
        assert!(due(101, 100));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("check.json");
        fs::write(
            &path,
            serde_json::to_vec(&Cache {
                checked_at: 100,
                succeeded: true,
                release: Some(release("2.0.0")),
            })
            .unwrap(),
        )
        .unwrap();
        let (cached, status) = run_check(false, Some(path), 101);
        assert!(matches!(status, CheckStatus::Current));
        assert_eq!(cached.unwrap().tag_name, "v2.0.0");
    }

    #[test]
    fn failures_keep_the_cached_release_and_manual_checks_bypass_the_cooldown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.json");
        let (cached, status) =
            run_check_with(false, Some(path.clone()), 100, || Ok(release("2.0.0")));
        assert!(matches!(status, CheckStatus::Current));
        assert_eq!(cached.unwrap().tag_name, "v2.0.0");
        let (cached, status) = run_check_with(true, Some(path.clone()), 101, || bail!("offline"));
        assert!(matches!(status, CheckStatus::Failed));
        assert_eq!(cached.unwrap().tag_name, "v2.0.0");
        let (_, status) = run_check_with(false, Some(path.clone()), 102, || {
            panic!("automatic retry must be throttled")
        });
        assert!(matches!(status, CheckStatus::Failed));
        let (cached, status) =
            run_check_with(true, Some(path.clone()), 103, || Ok(release("3.0.0")));
        assert!(matches!(status, CheckStatus::Current));
        assert_eq!(cached.unwrap().tag_name, "v3.0.0");
        fs::write(&path, "invalid cache").unwrap();
        let (cached, _) = run_check_with(false, Some(path), 104, || Ok(release("3.0.0")));
        assert_eq!(cached.unwrap().tag_name, "v3.0.0");
    }
}

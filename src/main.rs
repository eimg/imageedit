#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod adjustments;
mod commands;
mod document;
mod filters;
mod geometry;
mod icons;
#[cfg(target_os = "macos")]
mod macos;
mod paint;
mod preview;
mod project;
mod selection;
mod transform;
mod workspace;

use std::path::PathBuf;

use anyhow::Result;
use futures::{StreamExt, channel::mpsc};
use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowHandle,
    WindowOptions, px, size,
};
use url::Url;
use workspace::Workspace;

fn main() {
    let app = Application::new().with_assets(icons::Icons);
    let (file_opens, mut requests) = mpsc::unbounded();
    // Launch Services can deliver URLs before GPUI has created the first window.
    app.on_open_urls(move |urls| {
        let paths: Vec<_> = urls
            .iter()
            .filter_map(|value| Url::parse(value).ok()?.to_file_path().ok())
            .collect();
        if !paths.is_empty() {
            let _ = file_opens.unbounded_send(paths);
        }
    });
    app.on_reopen(|cx| {
        if let Err(error) = ensure_workspace(cx) {
            eprintln!("Could not reopen ImageEdit: {error:#}");
        }
    });
    app.run(|cx| {
        #[cfg(target_os = "macos")]
        macos::set_dock_icon();
        commands::register(cx);
        let paths = std::env::args_os().skip(1).map(PathBuf::from).collect();
        if let Err(error) = open_paths(paths, cx) {
            eprintln!("Could not start ImageEdit: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);

        cx.spawn(async move |cx| {
            while let Some(paths) = requests.next().await {
                match cx.update(|cx| open_paths(paths, cx)) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => eprintln!("Could not open images: {error:#}"),
                    Err(_) => break,
                }
            }
        })
        .detach();
    });
}

fn ensure_workspace(cx: &mut App) -> Result<WindowHandle<Workspace>> {
    if let Some(handle) = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast())
    {
        return Ok(handle);
    }
    let bounds = Bounds::centered(None, size(px(1140.), px(740.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(800.), px(520.))),
            titlebar: Some(TitlebarOptions {
                title: Some("ImageEdit".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Workspace::new(window, cx)),
    )
}

fn open_paths(paths: Vec<PathBuf>, cx: &mut App) -> Result<()> {
    ensure_workspace(cx)?.update(cx, |workspace, window, cx| {
        window.activate_window();
        if !paths.is_empty() {
            workspace.open_paths(paths, cx);
        }
    })
}

fn open_dialog(cx: &mut App) {
    let result = ensure_workspace(cx).and_then(|handle| {
        handle.update(cx, |workspace, window, cx| {
            workspace.open_dialog(window, cx)
        })
    });
    if let Err(error) = result {
        eprintln!("Could not open the file picker: {error:#}");
    }
}

fn quit(cx: &mut App) {
    let Some(handle) = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<Workspace>())
    else {
        cx.quit();
        return;
    };
    let _ = handle.update(cx, |workspace, window, cx| {
        if workspace.request_exit(true, window, cx) {
            cx.quit();
        }
    });
}

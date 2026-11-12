# Windows menus and GitHub build handoff

## Objective and current state

Implement native Windows menus and a GitHub Actions build that gives the user a
Windows executable to review manually in Parallels on this Mac. The user has
accepted this validation arrangement. Do not require interactive UI testing on
the Mac before delivering the Windows artifact.

- Repository: https://github.com/eimg/lime
- Working directory: `/Users/eimg/Desktop/ImageEdit`
- Starting branch: `codex/text-font-options`, tracking the same branch on origin.
- Starting commit: `9f7d227b0cc11751a125ad660588c344e4f18507` (already pushed).
- At the time this handoff was written, the only newer changes are this document
  and its link in AGENTS.md. Inspect Git status before starting; preserve any
  newer user or agent edits. The default GitHub branch remains `main`; the latest
  work is on the feature branch, so start from that feature branch/commit.
- Read AGENTS.md, README.md, and `/Users/eimg/AGENTS.md`. The latter describes ISP
  filtering and standing Olex VPN permission; restore prior VPN state if used.
- Product branding is **Lime — Lite Image Editor**. New projects use `.lime`;
  legacy `.imageedit` projects remain supported without changing format version 6
  or its magic. Keep the existing legacy golden fixture unchanged.
- The user explicitly reverted the lime-slice logo. Keep the original blue icon
  in `macos/AppIcon.svg` and `macos/AppIcon.icns`. Do not redesign it.
- No Windows workflow, installer, executable icon resource, or shell association
  implementation exists yet. macOS packaging is `scripts/bundle-macos.sh`, with
  output `dist/Lime.app`; preserve it.

Last verified on this starting commit:

- `cargo fmt --check`
- `cargo clippy --locked -- -D warnings`
- `cargo test --locked`: 225 passed, 2 intentionally ignored benchmarks.
- Signed macOS bundle builds successfully.
- `cargo check --locked --target x86_64-pc-windows-gnu` passes from macOS.
  This proves type-checking only, not Windows linking, release shaders, or runtime.
- No actual Windows execution has been reviewed yet.

## 1. Implement a Windows native menu adapter

Recommended approach discussed with the user: a Windows-only adapter using the
`muda` crate, attached to GPUI's Win32 window handle. Implement this in Lime;
keep GPUI pinned at 0.2.2 and avoid a framework fork or broad dependency migration.
If an actual integration blocker changes this recommendation, explain it before
substituting a different menu architecture.

### Existing code to reuse

- `src/commands.rs`: `application_menus(macos: bool)` is the canonical menu tree.
  It already defines Windows File -> Exit and platform-specific shortcuts.
  `register()` installs menus with `cx.set_menus(...)` and registers global actions.
- `src/main.rs`: `ensure_workspace()` creates the window; `main()` has an existing
  async channel pattern for routing incoming file-open requests to the UI thread.
- `src/workspace/view.rs` and the other workspace modules register action handlers
  conditionally for document state. Reuse their actions rather than duplicating
  image operations or bypassing pending-edit and unsaved-change handling.
- `src/workspace.rs`: Workspace lifetime/state and notifications.

The actual pinned GPUI Windows implementation is available in the local Cargo
registry under `gpui-0.2.2/src/platform/windows/platform.rs`. Its `set_menus()`
merely stores `OwnedMenu` data; `get_menus()` returns it. It does not install a
native menu bar. Our menu-definition tests therefore do not prove a menu exists.

### Implementation requirements

1. Add an isolated module, e.g. `src/windows_menu.rs`, behind
   `#[cfg(target_os = "windows")]`. Add any dependencies only for Windows where
   possible. Inspect current official muda APIs and choose a compatible release;
   update Cargo.lock intentionally and keep it tracked.
2. Convert the shared menu tree (`cx.get_menus()` or a narrowly exposed builder)
   into native menus. Preserve order, separators, all nested submenus, names,
   and File -> Exit. Omit macOS Services/Hide application menu on Windows.
   Use one canonical tree, not a second hand-maintained Windows command list.
3. Obtain the native HWND via
   `raw_window_handle::HasWindowHandle::window_handle(window)` and match Win32.
   GPUI's inherent `window.window_handle()` instead returns its own logical
   handle; it is not the HWND. Add a direct raw-window-handle dependency if needed.
4. Attach the menu when the workspace window is created. Keep native menu owners
   and callback/subclass state alive on the UI thread for the window lifetime.
   Handle window recreation and teardown without dangling HWNDs or duplicate
   handlers. Keep unsafe native calls narrow and document their lifetime rules.
5. Map native item IDs to the existing boxed GPUI actions, including parameterized
   actions. Forward activation IDs through an async channel to GPUI's UI thread.
   Dispatch after the native callback returns, ideally to the originating window.
   Do not synchronously borrow/re-enter GPUI from Win32 menu callbacks: callbacks
   can occur while GPUI already owns a window/app borrow.
6. Sync enabled states with current action availability (including global actions)
   and document/focus changes. GPUI has `App::is_action_available()` and
   `Window::is_action_available()`; the App method itself updates the active
   window, so use it only from a safe context where that window is not borrowed.
   Defer validation if necessary. Refresh as state changes and revalidate before
   dispatch. Avoid constant background polling just to update menus.
7. Keep existing GPUI Ctrl shortcuts as the command execution path. Native menu
   shortcut labels should agree with those bindings. Muda accelerator execution
   needs message-loop integration; do not introduce duplicate shortcut execution
   or assume attaching a menu automatically handles accelerator translation.
   Provide conventional Alt mnemonics and native arrow/Enter/Escape navigation.
8. Preserve focus and editing behavior: text/numeric edits, font search, open
   dialogs, busy documents, previews, undo/redo, save/export, and exit confirmation
   must continue through existing handlers. Menu opening must not insert text on
   the canvas or discard a draft. Disabled/planned commands stay disabled.
9. Native menus may follow the OS theme. Keep the GPUI canvas/workspace layout and
   macOS menus intact; do not add a new top toolbar or rebuild tool panels.

Useful primary sources:

- https://docs.rs/muda/latest/muda/ (HWND attachment, events, menu types).
- https://docs.rs/muda/latest/muda/struct.Menu.html (attachment/accelerator details).
- Pinned GPUI source: `src/platform/app_menu.rs`, `src/app.rs`, `src/window.rs`,
  `src/platform/windows/platform.rs`, and `src/platform/windows/window.rs` in the
  local Cargo registry. Verify against this exact version, not current Zed APIs.

### Focused validation

- Test conversion/ID mapping and nested-menu preservation using meaningful tests.
  Cover parameterized actions and disabled commands; definition snapshots alone
  are insufficient. Structure pure conversion logic so it can be tested without
  displaying native windows where practical.
- Run macOS formatting, Clippy, and the full existing test suite after changes.
- Repeat the Windows GNU cross-check locally; use native Windows CI as the
  authoritative linking and release-build check.
- Do not describe the menus as runtime-verified until the user reviews the build.

## 2. Add the GitHub Actions Windows build

Create `.github/workflows/windows.yml`. The deliverable for this pass is a
portable, downloadable test build, not an installer or public release.

### Workflow structure

- Enable `push` on `main` and the working `codex/**` branches, `pull_request`, and
  `workflow_dispatch`. Use ordinary PR builds, not `pull_request_target`.
- Use `permissions: contents: read`, concurrency cancellation for superseded runs,
  a reasonable timeout, and artifact retention appropriate for manual testing.
- Start with the explicit x64 runner label `windows-2022` and Rust target
  `x86_64-pc-windows-msvc`; verify the runner's current availability/software list.
  The user will test in Parallels, but their guest architecture was not checked.
  Label artifacts clearly as x64; do not claim native ARM64 support. ARM64 can be
  a separate follow-up if needed, with its own build/dependency validation.
- Check out the requested commit and install a recent stable Rust toolchain with
  rustfmt and clippy. Choose supported, reviewed action versions; pin external
  actions to reviewed immutable SHAs where practical. Avoid adding unneeded
  external services. An optional Rust cache must include target/toolchain/lockfile
  inputs and must not hide a clean-build failure.
- Print the commit SHA, Rust version, target, and selected SDK/compiler path in
  logs so an artifact can be traced to its source.

### Important native release requirement: FXC

The pinned `gpui-0.2.2/build.rs` compiles HLSL shaders for native Windows release
builds. Its `compile_shaders()` needs **fxc.exe**, not merely Rust or a linker.
It also embeds its Windows manifest via the enabled `windows-manifest` feature.

In PowerShell, discover a suitable installed Windows SDK x64 `fxc.exe` under
`${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\fxc.exe`; choose an installed
SDK version, fail clearly if absent, and write `GPUI_FXC_PATH=<full path>` into
`$env:GITHUB_ENV` for later build steps. Do not rely on GPUI's hardcoded fallback
SDK path or assume FXC is on PATH. The earlier macOS cross-check does not exercise
this native Windows build-script path. Preserve the manifest feature.

Use these checks in separate steps so failures are identifiable:

```text
cargo fmt --check
cargo clippy --locked --target x86_64-pc-windows-msvc -- -D warnings
cargo test --locked --target x86_64-pc-windows-msvc
cargo build --release --locked --target x86_64-pc-windows-msvc
```

Investigate actual platform-specific test failures rather than ignoring tests
wholesale. The two existing rendering/stroke benchmarks can remain ignored.
CI does not need to launch the GUI in the hosted runner session.

### Artifact and delivery

- Package the actual release binary from
  `target/x86_64-pc-windows-msvc/release/lime.exe`, not a debug binary.
- Upload a clearly named artifact, e.g. `lime-windows-x64-<short-sha>`, with
  `if-no-files-found: error`. Include a short usage/readiness text and commit SHA;
  keep PDB symbols separate if publishing them for diagnostics.
- Check required runtime dependencies and include permitted redistributables or
  document their installation when needed. Do not promise a standalone executable
  based solely on a successful link on a fully provisioned CI runner.
- A portable ZIP/artifact is sufficient initially. Executable `.ico` resources,
  installer/signing, and `.lime`/Open With registration are subsequent tasks unless
  necessary for this manual test build. Keep the original blue artwork if adding
  a Windows icon. If adding app resources, avoid conflicting with GPUI's embedded
  manifest. Do not claim Windows file association support from macOS Info.plist.
- Commit and push the implementation/workflow to the working branch as part of
  the authorized continuation, then inspect its workflow run through completion.
  Fix failures and rerun until a genuine native release artifact is available.
  Do not merge to main or publish a release without a corresponding user request.
- A new dispatch-only workflow may not be available through the manual Actions UI
  before it exists on the default branch; the feature-branch push trigger should
  produce the first build without requiring an early merge.
- Deliver the exact commit, workflow run link, artifact name/download location,
  and any known runtime dependency or unverified limitation. Update README.md and
  AGENTS.md to reflect what was built versus what was manually tested.

Primary references to recheck when implementing:

- https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md
- https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions
- https://docs.github.com/en/actions/using-workflows/storing-workflow-data-as-artifacts
- https://github.com/actions/upload-artifact

## User's manual review in Parallels

Give the user this short checklist with the artifact; do not block delivery on
performing this interactive review yourself:

1. Launch Lime and confirm all menu headings/submenus are visible and usable by
   mouse and Alt/arrow/Enter/Escape. Check viewport sizing after the native menu
   bar is added, plus resizing and display scaling.
2. With no document, confirm meaningful enable/disable states. Open/create an
   image and exercise commands that have no toolbar/keyboard alternative.
3. Verify File Open, Save As `.lime`, legacy `.imageedit` opening, export, native
   dialogs, filenames with spaces/Unicode, close, and unsaved Exit confirmation.
4. Exercise Undo/Redo, clipboard, selection/layer commands, adjustments, filters,
   and nested transform menus; one click/shortcut must execute once.
5. While typing text, editing numeric options, or filtering the font list, invoke
   menu commands and verify focus, drafts, commit/cancel, and clipboard behavior.
6. Check disabled commands during background work/dialogs/previews. Report the
   exact build SHA and reproduction steps for failures.

Completion means: native menus implemented from the shared actions, native
Windows CI checks and release build green, downloadable artifact delivered, and
macOS behavior preserved. The user's runtime review remains a separate pending
verification until they provide feedback.

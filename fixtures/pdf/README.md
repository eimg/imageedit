# PDF regression fixtures

These small fixtures are authored for Lime and contain no external artwork or
font files. `geometry-fonts-transparency-images.pdf` exercises CropBox, 90° page
rotation, Helvetica text, an embedded Type 3 glyph, half-opacity vector artwork,
and a 2×2 RGB image. The malformed/truncated and invalid-password encrypted
fixtures must return errors. Tests run in the shared macOS/Windows Rust suite;
this does not claim native Windows or visual PDF QA on this machine.

The **PDF checks** GitHub Actions workflow runs the PDF Rust tests and the real
headless worker protocol on macOS and Windows. To run the same checks locally:

```sh
cargo test --locked pdf
cargo build --locked
python3 scripts/check-pdf-worker.py --binary target/debug/lime
```

On Windows, use `target/debug/lime.exe`. These checks do not open an app window.

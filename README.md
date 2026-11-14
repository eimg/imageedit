# Lime

<img src="macos/AppIcon.svg" width="96" height="96" alt="Lime half-slice logo">

**Lite Image Editor** for Windows x64, macOS Apple silicon, and Linux x86_64/ARM64.
Layers, painting, selections, text, and effects in a workspace that puts your canvas first.

![Lime editing a night skyline](lime-screenshot.jpg)

## Download

**Lime 1.2.0** · [Release notes](https://github.com/eimg/lime/releases/tag/v1.2.0)

| Platform | Download | Install |
| --- | --- | --- |
| Windows x64 | [EXE](https://github.com/eimg/lime/releases/download/v1.2.0/Lime-1.2.0-windows-x64.exe) | Download and run. No installer. |
| macOS (Apple silicon) | [DMG](https://github.com/eimg/lime/releases/download/v1.2.0/Lime-1.2.0.dmg) | Open and drag Lime to Applications. |
| Linux x86_64 | [tar.gz](https://github.com/eimg/lime/releases/download/v1.2.0/Lime-1.2.0-linux-x86_64.tar.gz) | Extract and run `./lime`. |
| Linux ARM64 | [tar.gz](https://github.com/eimg/lime/releases/download/v1.2.0/Lime-1.2.0-linux-aarch64.tar.gz) | Extract and run `./lime`. |

[SHA-256 checksums](https://github.com/eimg/lime/releases/download/v1.2.0/SHA256SUMS)

Windows requires Windows 10 or later, DirectX, and the Microsoft Visual C++
2015–2022 x64 runtime. Linux requires glibc 2.35 or newer, a working Vulkan driver,
an X11 or Wayland session, and a file chooser portal. See the
[Linux requirements](linux/README.txt) for runtime packages.

Windows downloads are unsigned. The macOS app is ad hoc signed and not notarized.

## Features

- **Layers** — masks, blend modes, opacity, locks, and linked movement.
- **Selections and transforms** — rectangle, oval, freehand, magic wand, crop, scale, rotate, and perspective.
- **Painting** — pencil, brush, eraser, clone stamp, gradients, and bucket fill.
  Per-tool settings and recent colors survive app restarts.
- **Text and paths** — editable text, font controls, wrapping, and editable vector paths.
- **Color and effects** — levels, curves, blur, sharpen, drop shadows, and editable layer effects.
- **PDF and recovery** — import selected PDF pages, export a flattened PDF, and recover interrupted work.

## Getting started

Open an image or PDF, or create a new image. Choose a tool and adjust its options
beside the canvas. Hover the **?** for gestures and shortcuts.

Crop and paint strokes can start outside the canvas; crop stays within the image.
The status bar shows the current app version.

Use the reset arrow in **Tool Options** or **Tools → Reset Current Tool** to restore
that tool's defaults. **Tools → Reset All Tool Settings…** resets every tool after
confirmation. Resets preserve artwork, colors, and saved gradient presets.

Save as **`.lime`** to keep layers and editable content. Use **Export** for a
flattened PNG, JPEG, WebP, or PDF copy.

[Report a problem or suggest a feature](https://github.com/eimg/lime/issues).

Lime - Lite Image Editor
Experimental Linux x86_64 / ARM64 portable build

Extract the archive, then run ./lime. No installation or administrator access
is required. Keep all files together if you want the optional desktop launcher.
The separate raw binary is the same executable; run chmod +x on it after download.

Requirements:
- 64-bit GNU/Linux, glibc 2.35 or newer (Ubuntu 22.04 build baseline).
- A working Vulkan driver and an X11 or Wayland desktop session.
- FreeType, Fontconfig, libxkbcommon and libxkbcommon-x11; X11/Wayland libraries.
- A D-Bus session and xdg-desktop-portal with a desktop-specific file chooser
  backend (GTK, GNOME, KDE, etc.) for Open/Save dialogs.
- Installed fonts. ARM64 hardware needs a compatible Vulkan driver too.

Arch Linux runtime packages (install through your normal package manager):
  vulkan-icd-loader fontconfig freetype2 libxkbcommon libxkbcommon-x11
  libxcb libx11 wayland xdg-desktop-portal
Also install the Vulkan driver and portal backend appropriate for your system.
For example, xdg-desktop-portal-gtk is a common file chooser backend.
In a virtual machine without Vulkan GPU support, vulkan-swrast provides Mesa's
software Vulkan renderer (Lavapipe); use a current Mesa release. Performance
will depend on the CPU. An older software driver can open a blank window.

Ubuntu 22.04 runtime packages:
  libvulkan1 libfontconfig1 libfreetype6 libxkbcommon0 libxkbcommon-x11-0
  libwayland-client0 libwayland-cursor0 libx11-xcb1 libxcb1
  libxcb-xfixes0 libxcb-shape0 xdg-desktop-portal
Also install the appropriate GPU driver and portal backend.

Menus: click File/Edit/etc. or press F10; Alt plus a menu's first letter opens
that menu. Arrows navigate, Enter activates, Escape closes. Submenus show a
Back row; Left returns to the parent. File > Exit and Ctrl+Q quit.

Pass image, PDF, or .lime filenames as arguments: ./lime example.png
Wayland is preferred when WAYLAND_DISPLAY is set; otherwise X11 is used.
To test X11 within a Wayland session: env -u WAYLAND_DISPLAY ./lime

Optional desktop integration:
Place the executable in ~/.local/bin/lime, the desktop file in
~/.local/share/applications/, and the SVG in
~/.local/share/icons/hicolor/scalable/apps/com.eimg.lime.svg.
Create those directories if needed. The launcher expects lime on your desktop
session's PATH; otherwise edit its Exec line to the absolute executable path.
No system integration or file association is installed automatically.

Settings/gradients: $XDG_CONFIG_HOME/lime (default ~/.config/lime).
Recovery copies: the Recovery directory beneath that settings directory.
Update-check cache: $XDG_CACHE_HOME/lime (default ~/.cache/lime).

Builds are experimental pending desktop testing on both architectures.
A binary download still relies on system libraries, portals, and GPU drivers.
See linked-libraries.txt for the libraries linked by this particular build.
For detailed window/GPU logs, run LIME_LOG=debug ./lime from a terminal.
Report the distribution, architecture, session (X11/Wayland), GPU/driver,
terminal output, and steps to reproduce any problem.

Source and releases: https://github.com/eimg/lime

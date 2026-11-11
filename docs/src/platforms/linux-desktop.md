# Linux Desktop Development

Linux is the primary development and testing environment for CraftUI.

---

## 1. Wayland & X11 Windowing

CraftUI integrates seamlessly with modern Linux desktop environments (GNOME, KDE Plasma, Sway, Hyprland, XFCE):
- Native Wayland client support with client-side decorations and fractional scaling.
- Transparent X11 fallback when running on legacy display servers or inside Wine/containers.
- Dual GPU backend support:
  - **Vulkan** (primary): Ultra-low latency rendering via Mesa RADV, ANV, or NVIDIA drivers.
  - **Software Fallback**: Fast CPU rasterization via Vello CPU or TinySkia if running in virtual machines or headless CI runners.

---

## 2. In-Process Desktop Services

Unlike frameworks that spawn external CLI subprocesses (e.g. calling `zenity` or `xclip`), CraftUI uses fast, safe, in-process Rust integrations:
- **Clipboard:** Native Wayland (`wl-clipboard-rs`) and X11 (`x11-clipboard`) protocol integration.
- **File Dialogs:** Native `xdg-desktop-portal` file dialogs (`rfd`), honoring your system's file manager and dark/light desktop theme.
- **Multi-Window:** First-class support for spawning multiple native windows via `WindowLauncher`.

---

## 3. Running with Hot-Reload

During development, use hot-reload to iterate on `.cui` markup without waiting for Rust compilation:

```bash
craftui run --platform desktop
```
Changes saved in VS Code or your text editor reload in under 100 milliseconds while preserving component state!

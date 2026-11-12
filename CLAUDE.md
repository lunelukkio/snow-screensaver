# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A Windows snow-falling screensaver written in Rust with Direct3D 11. Goals:
- Launches full-screen, always on top of every other window, on every selected monitor
- Soft, slowly drifting snow particles in a continuous loop
- User-adjustable from an in-app right-click menu: **count**, **speed**, **size**, **edge blur**, **sway range**, **FPS**, **resolution**, **color** (monitors are chosen in `config.json`)
- One-click launch from the Windows taskbar; **double-click** dismisses the screensaver
- **Minimize CPU, GPU and memory use.** It is a screensaver, so sharpness and smoothness may be traded away for lower resource use (defaults: 15 fps, half-resolution back buffer)

It replaced a Python + tkinter implementation that used 95-98 % of a logical core. `docs/rust-rewrite-plan.md` (Japanese) holds the measurements, the rejected alternatives and the reasons behind the design below; read it before changing the rendering approach.

## Tooling

Rust (a recent stable toolchain for the MSVC target; developed with 1.95) and Cargo. Crates: `windows` (Win32 and Direct3D bindings), `serde_json` (settings), `winresource` (icon, build time only). Adding anything else needs a clear reason: the executable is about 260 KB and uses about 2 MB of resident memory.

```powershell
cargo test                                   # config, flake model, menu ids
cargo clippy --all-targets -- -D warnings
cargo fmt --check
./build.ps1                                  # release build -> dist\snow.exe
```

Python is used only for tooling, and always through **uv** (no `pip`, no manual venv). `tools/measure.py` needs only the standard library:

```powershell
uv run --no-project python tools/measure.py bench dist/snow.exe   # CPU, GPU, memory
uv run --no-project python tools/measure.py smoke dist/snow.exe   # behaviour
```

Each run starts the executable from a temporary folder with its own `config.json` and covers every monitor with snow (about 1 minute for `bench`, 30 seconds for `smoke`). CPU figures are percent of ONE logical core. Run both after any change to rendering, pacing or message handling.

The shaders are compiled with the Windows SDK's `fxc` and the bytecode is checked in (`shaders/snow_vs.cso`, `shaders/snow_ps.cso`), so a build does not need the SDK. After editing `shaders/snow.hlsl`, recompile both from the repository root:

```powershell
fxc /nologo /T vs_4_0 /E vs_main /O3 /Fo shaders\snow_vs.cso shaders\snow.hlsl
fxc /nologo /T ps_4_0 /E ps_main /O3 /Fo shaders\snow_ps.cso shaders\snow.hlsl
```

`build.rs` embeds `installer/snow.ico` as the executable's icon. `installer/make_icon.py` regenerates that icon: `uv run --no-project --with pillow python installer/make_icon.py` (it overwrites the tracked file).

## Architecture

Four modules under `src/`, kept separate so the frame path stays small:

- **`main.rs`**: windows, message loop and frame pacing. One borderless `WS_POPUP` + `WS_EX_TOPMOST` window per selected monitor, per-monitor DPI aware (physical pixels). The window procedure never touches application state: it only queues an `Event` (quit, menu, command, display on/off, session lock, display change), and the main loop handles the queue, so modal calls (popup menu, colour dialog) cannot re-enter. Frames are paced by a high-resolution waitable timer, with plain millisecond waits where it is missing. While the display is off or the session is locked there are no frames and no timer, only `WaitMessage`. A display change or a lost device rebuilds the windows and the renderer (up to 10 attempts, one second apart). The working set is trimmed once, 30 frames after start-up and after each menu action.
- **`flakes.rs`**: the particle model. One `FlakeField` per monitor holds parallel arrays (`base_x`, `y`, `vy`, `phase`) and a small xorshift generator. A flake falls `vy * dt * 60` pixels per step and sways by `sin(phase + t * SWAY_FREQ) * sway_amp`; `dt` is capped so flakes do not jump after a pause.
- **`render.rs`**: Direct3D 11. One device (hardware, with WARP as fallback) and one flip-model swap chain per monitor (`DXGI_SWAP_EFFECT_FLIP_DISCARD`, `DXGI_SCALING_STRETCH`) whose back buffer is `resolution` percent of the monitor; Windows stretches it. All flakes of a monitor are one `DrawInstanced(4, count)`: the instance buffer holds one `float2` centre per flake, and `shaders/snow.hlsl` computes the soft disc in the pixel shader (premultiplied alpha), so no flake image ever exists on the CPU. `Present(1, 0)` is the main CPU cost of a frame.
- **`menu.rs`**: the right-click menu (`TrackPopupMenu`) and the colour dialog. Each setting owns a block of 100 menu command ids (`ID_COUNT` = 1000, `ID_SPEED` = 1100, ...); the offset is the preset index, and `command_from_id` maps an id back to a `Command`. `tools/measure.py` posts these ids, so keep them stable.
- **`config.rs`**: `config.json` next to the executable. Every key is validated and clamped on its own; a missing or malformed file gives the defaults, unknown keys are ignored, and nothing here may crash the app. The render loop never re-reads the file: "Save settings" in the menu writes it.

### Dismissal

**Double-click** anywhere closes all monitor windows together (`WM_LBUTTONDBLCLK`; the window class has `CS_DBLCLKS`). Single clicks, mouse moves and key presses are ignored on purpose: this is a deliberate exit gesture, not Windows-style any-input dismissal. A `WM_CLOSE` (for example from Alt+F4) and the menu's Exit also quit. The pointer is hidden over the snow, and the snow stands still while the menu or the colour dialog is open.

### Multi-monitor

`EnumDisplayMonitors` gives the geometry in physical pixels. `"monitors": "all"` (the default) or a list of indices in that enumeration order, for example `[0, 2]`. Indices that do not exist are skipped, and an empty selection falls back to all monitors. Each monitor has its own window, swap chain and `FlakeField` (`count` is per monitor); the device and the timer are shared.

### Performance constraints

Measured on a two-monitor setup (4K + Full HD) at the defaults: about 0.7 % of one logical core (about 0.4 ms of CPU per frame, mostly `Present`), 1.7 MB private working set, `dwm.exe` unchanged within noise. Alternatives that were measured and rejected: GDI (3.5-5 % CPU), DirectComposition visuals (`dwm.exe` +9-10 points), a blt-model swap chain (more `dwm.exe` load than flip model). Re-measure before swapping any of them in.

- The frame path must not allocate: the instance buffer is mapped and filled in place. Menu, config and rebuild work stay off the hot path.
- Do not raise the system timer rate (the high-resolution waitable timer exists to avoid that) and do not busy-wait.
- Avoid heavy frameworks (`winit`, `wgpu`, GUI toolkits): they add dependencies and resident memory.
- Resolution 100 % costs about 6 times the GPU load of 50 % for little visible gain, so 50 % is the default.
- EcoQoS and `WS_EX_NOREDIRECTIONBITMAP` showed no measurable benefit and are not used.

## Conventions

- Entry point: `src/main.rs`. There is no command-line interface; all configuration lives in the right-click menu and `config.json`.
- Config file: `config.json` next to `snow.exe` (so `dist\config.json`). Keys: `count`, `speed`, `size`, `edge_blur`, `sway_amp`, `fps` (default 15), `color` (`#rrggbb`), `monitors`, `resolution` (percent, default 50). The format is compatible with the former Python version's file, which has neither `fps` nor `resolution`. The root `config.json` is git-ignored personal settings that `build.ps1` copies to `dist\` once.
- `SNOW_STATS_FILE`: when this environment variable names a file, the measured frame rate is written there on exit. It is a test hook for `tools/measure.py` and does nothing otherwise.
- Code comments and identifiers are in English.

## Distribution

`./build.ps1` runs `cargo build --release` (LTO, `panic = "abort"`, stripped) and copies the result to `dist\snow.exe`. `dist/`, `target/` and the root `config.json` are git-ignored. Pin `dist\snow.exe` to the taskbar.

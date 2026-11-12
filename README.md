# Snow Screensaver

A very light Windows snow-falling screensaver written in Rust with Direct3D 11. It launches full-screen, stays on top of every other window, and dismisses on double-click.

## Features

- Soft snow particles drifting slowly across every selected monitor
- Adjustable from a right-click menu while it runs: **count**, **speed**, **size**, **edge blur**, **sway range**, **FPS**, **resolution** and **color**
- Multi-monitor: all monitors by default, or a subset chosen in `config.json`
- One-click launch from the Windows taskbar
- Double-click anywhere to exit (single clicks and key presses are ignored on purpose)
- Minimal CPU, GPU and memory use; nothing is drawn while the display is off or the session is locked

## Resource use

At the default settings (15 fps, back buffer at 50 % of the monitor resolution) it takes about 0.7 % of one logical CPU core and about 2 MB of private working set, in a 260 KB executable. That was measured with two monitors (4K and Full HD). The Python/tkinter version it replaced took 95-98 % of a core.

How: one instanced Direct3D 11 draw per monitor with the soft discs computed in the pixel shader, a flip-model swap chain that Windows stretches from the smaller back buffer, a low frame rate paced by a high-resolution timer, and no drawing at all while nothing is visible. The measurements and the alternatives that were rejected are in [`docs/rust-rewrite-plan.md`](docs/rust-rewrite-plan.md) (in Japanese).

## Build

Requirements: Windows 10 (version 1803 or later) or Windows 11, a GPU with Direct3D 10 or newer support (the software WARP rasterizer is the fallback), and a recent stable Rust toolchain for the MSVC target ([rustup](https://rustup.rs/)).

```powershell
./build.ps1
```

This builds the release executable and puts it at `dist\snow.exe`; pin that file to the Windows taskbar for one-click launch. The first run also copies an existing `config.json` from the repository root to `dist\`.

## Controls

| Action | Result |
|---|---|
| Right-click | Open the settings menu (count / speed / size / edge blur / sway range / FPS / resolution / color / save settings / exit) |
| Double-click | Exit |
| Single click / key / mouse move | Ignored |

Pick a value to apply it live; **Save settings** writes `config.json` (next to `snow.exe`) for the next launch. Snow stands still while the menu or the colour dialog is open.

## Settings

`config.json` sits next to `snow.exe`. A missing or invalid value falls back to its default.

| Key | Meaning | Default |
|---|---|---|
| `count` | Flakes per monitor (0-20000) | 200 |
| `speed` | Fall speed in pixels per 1/60 s | 0.8 |
| `size` | Flake diameter in pixels | 6 |
| `edge_blur` | Edge softness in pixels (0 = hard edge) | 2 |
| `sway_amp` | Horizontal sway in pixels | 18 |
| `fps` | Frames per second (1-240) | 15 |
| `resolution` | Back-buffer size in percent of the monitor (10-100) | 50 |
| `color` | Flake colour as `"#rrggbb"` | `"#ffffff"` |
| `monitors` | `"all"`, or a list of monitor indices such as `[0, 1]` | `"all"` |

## Test and measure

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
uv run --no-project python tools/measure.py bench dist/snow.exe   # CPU, GPU, memory
uv run --no-project python tools/measure.py smoke dist/snow.exe   # behaviour
```

`tools/measure.py` needs only Python's standard library, run here through [uv](https://docs.astral.sh/uv/). Each run covers every monitor with snow for about a minute.

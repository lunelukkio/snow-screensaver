# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A Windows snow-falling screensaver written in Python. Goals:
- Launches full-screen, always on top of every other window
- Soft, slowly drifting snow particles (Pillow gaussian-blurred image items) in a continuous loop
- User-adjustable from an in-app right-click menu: **count**, **speed**, **size**, **edge blur**, **sway range**, **FPS**, **color**, monitor selection
- Multi-monitor: choose all monitors, or a subset
- One-click launch from the Windows taskbar; **double-click** dismisses the screensaver
- Minimize CPU and memory usage

## Tooling

This project uses **uv** for environment and dependency management. Do not use `pip` or `venv` directly.

```bash
uv sync                       # install/refresh deps from pyproject.toml + uv.lock
uv add <package>              # add a runtime dep
uv add --dev <package>        # add a dev dep
uv run python main.py         # run the entry point inside the venv
uv run python -m <module>     # run any module inside the venv
```

Python target: **3.12** (pinned via `.python-version`). The venv lives at `.venv/`.

## Architecture

The app is two layers — keep them separate so the render loop stays tight:

1. **Render layer (`snow/screensaver.py`)** — one borderless top-most `tkinter.Toplevel` per selected monitor, each owning a `Canvas`. A single hidden `Tk()` root drives all monitors via one `after()`-paced tick. The right-click menu (`tk.Menu` cascades) calls live mutators on each `FlakeField`. Tick interval is mutable (`interval_ms[0]`) so the FPS menu takes effect immediately.

2. **Particle model (`snow/particles.py`)** — `FlakeField` holds parallel arrays for `x`, `y`, `vy`, `phase`, `base_x`, `img_id`. Each flake is drawn as a `create_image` item using a single per-field RGBA `PhotoImage` produced by `render_flake_pil()` (Pillow `ImageDraw.ellipse` → 4× supersample → `LANCZOS` downscale → `GaussianBlur`). Setters re-render the image only when `size`, `edge_blur`, or `color` change. Horizontal sway is `base_x + sin(phase + t * SWAY_FREQ) * sway_amp` — no velocity integrator.

`snow/config.py` loads/saves `config.json` next to `main.py` (or `snow.exe` when frozen). The render loop never re-reads it; "Save settings" in the menu writes through. Unknown keys in a stale JSON are silently dropped.

### Dismissal

**Double-click** anywhere closes all monitor windows together. **Single clicks** are bound to a no-op (`return "break"`) so they don't dismiss; mouse moves and key presses are intentionally not bound. This is a deliberate exit gesture, not a Windows-style any-input dismissal.

### Multi-monitor

`screeninfo.get_monitors()` for geometry. One `Toplevel` per monitor with explicit `geometry(f"{w}x{h}+{x}+{y}")`, `overrideredirect(True)`, `attributes("-topmost", True)`. All share one root `Tk()` and one tick.

### Performance constraints

- Avoid pygame / PyQt — they pull large binaries and add process memory. tkinter + screeninfo + Pillow is the chosen stack.
- The render tick must not allocate (no list comprehensions building new lists per frame, no f-strings in the hot path).
- Live menu mutators are off the hot path; image re-rendering only happens on user action.

### Color picker note

`tkinter.colorchooser.askcolor` is hidden by topmost windows, so `pick_color()` toggles `-topmost` off on every monitor while the dialog is open and back on afterward.

## Conventions

- Entry point: `main.py` calls `snow.screensaver.run()`. There is no `--settings` flag — all configuration lives in the in-app right-click menu.
- Config file: `config.json` next to `main.py` (or next to `snow.exe` when frozen). Missing or malformed → fall back to `DEFAULTS`; never crash.
- Runtime deps: `screeninfo`, `pillow`. Dev: `pyinstaller`. Adding anything else needs a clear reason.

## Distribution

`installer/build_exe.py` runs PyInstaller `--onefile --noconsole` and produces a single `dist/snow.exe`. Pin to taskbar.

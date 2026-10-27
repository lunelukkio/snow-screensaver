# Snow Screensaver

A lightweight Windows snow-falling screensaver written in Python. Launches full-screen, stays on top of every other window, and dismisses on double-click.

## Features

- White, soft snow particles drifting slowly across every selected monitor
- Adjustable from a right-click menu: **count**, **speed**, **size**, **edge blur**, **sway range**, **FPS**, and **color**
- Multi-monitor: apply to all monitors, or pick a subset
- One-click launch from the Windows taskbar
- Double-click anywhere to exit (single clicks and key presses are ignored on purpose)
- Minimal CPU/memory footprint — `tkinter` + `screeninfo` + `Pillow`

## Setup

This project uses [uv](https://docs.astral.sh/uv/). Install dependencies with:

```bash
uv sync
```

## Run from source

```bash
uv run python main.py
```

Right-click anywhere to open the settings menu. Pick a value to apply it live; "Save settings" writes to `config.json` for next launch.

## Build a standalone .exe

```bash
uv run python installer/build_exe.py
```

Outputs `dist/snow.exe`. Pin it to the Windows taskbar for one-click launch.

## Controls

| Action | Result |
|---|---|
| Right-click | Open settings menu (count / speed / size / edge blur / sway range / FPS / color / save / exit) |
| Double-click | Exit screensaver |
| Single click / key / mouse move | Ignored |

## Requirements

- Windows 10/11
- Python 3.12 (managed by uv)

from __future__ import annotations

import time
import tkinter as tk
from dataclasses import dataclass
from tkinter import colorchooser

from screeninfo import get_monitors

from . import config
from .particles import FlakeField


COUNT_PRESETS = [10, 25, 50, 100, 200, 400, 800, 1500, 3000, 5000, 10000]
SPEED_PRESETS = [0.1, 0.3, 0.5, 0.8, 1.0, 1.5, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0, 30.0]
SIZE_PRESETS = [1, 2, 3, 5, 8, 12, 18, 25, 35, 50]
EDGE_BLUR_PRESETS = [0, 1, 2, 3, 5, 8, 12, 20, 30]
SWAY_PRESETS = [0, 5, 10, 18, 30, 50, 80, 120, 200]
FPS_PRESETS = [10, 15, 20, 24, 30, 45, 60, 90, 120]


@dataclass
class MonitorWindow:
    top: tk.Toplevel
    canvas: tk.Canvas
    field: FlakeField


def _selected_monitors(cfg: config.Config):
    monitors = get_monitors()
    if cfg.monitors == "all" or not isinstance(cfg.monitors, list):
        return monitors
    chosen = []
    for idx in cfg.monitors:
        if isinstance(idx, int) and 0 <= idx < len(monitors):
            chosen.append(monitors[idx])
    return chosen or monitors


def _make_window(root: tk.Tk, monitor, cfg: config.Config) -> MonitorWindow:
    top = tk.Toplevel(root)
    top.overrideredirect(True)
    top.geometry(f"{monitor.width}x{monitor.height}+{monitor.x}+{monitor.y}")
    top.attributes("-topmost", True)
    top.configure(bg="black")
    canvas = tk.Canvas(
        top,
        width=monitor.width,
        height=monitor.height,
        bg="black",
        highlightthickness=0,
        cursor="none",
    )
    canvas.pack(fill="both", expand=True)
    field = FlakeField(
        width=monitor.width,
        height=monitor.height,
        count=cfg.count,
        size=cfg.size,
        speed=cfg.speed,
        edge_blur=cfg.edge_blur,
        sway_amp=cfg.sway_amp,
        color=cfg.color,
    )
    field.create_items(canvas)
    return MonitorWindow(top=top, canvas=canvas, field=field)


def _build_menu(root: tk.Tk, windows: list[MonitorWindow], cfg: config.Config,
                interval_ms: list[int], quit_all) -> tk.Menu:
    """Right-click context menu. Mutations apply live to every monitor."""
    count_var = tk.IntVar(value=cfg.count)
    speed_var = tk.DoubleVar(value=cfg.speed)
    size_var = tk.IntVar(value=cfg.size)
    edge_var = tk.IntVar(value=cfg.edge_blur)
    sway_var = tk.IntVar(value=cfg.sway_amp)
    fps_var = tk.IntVar(value=cfg.fps)

    def apply_count(v: int):
        cfg.count = v
        count_var.set(v)
        for w in windows:
            w.field.set_count(w.canvas, v)

    def apply_speed(v: float):
        cfg.speed = v
        speed_var.set(v)
        for w in windows:
            w.field.set_speed(v)

    def apply_size(v: int):
        cfg.size = v
        size_var.set(v)
        for w in windows:
            w.field.set_size(w.canvas, v)

    def apply_edge_blur(v: int):
        cfg.edge_blur = v
        edge_var.set(v)
        for w in windows:
            w.field.set_edge_blur(w.canvas, v)

    def apply_sway(v: int):
        cfg.sway_amp = v
        sway_var.set(v)
        for w in windows:
            w.field.set_sway_amp(v)

    def apply_fps(v: int):
        cfg.fps = v
        fps_var.set(v)
        interval_ms[0] = max(1, int(round(1000 / v)))

    def pick_color():
        # The colorchooser modal would otherwise be hidden by the topmost windows.
        for w in windows:
            w.top.attributes("-topmost", False)
        try:
            _, hex_ = colorchooser.askcolor(initialcolor=cfg.color, parent=root)
        finally:
            for w in windows:
                w.top.attributes("-topmost", True)
        if hex_:
            cfg.color = hex_
            for w in windows:
                w.field.set_color(w.canvas, hex_)

    menu = tk.Menu(root, tearoff=0)

    count_menu = tk.Menu(menu, tearoff=0)
    for v in COUNT_PRESETS:
        count_menu.add_radiobutton(label=str(v), value=v, variable=count_var,
                                    command=lambda v=v: apply_count(v))
    menu.add_cascade(label="Count", menu=count_menu)

    speed_menu = tk.Menu(menu, tearoff=0)
    for v in SPEED_PRESETS:
        speed_menu.add_radiobutton(label=f"{v:g}", value=v, variable=speed_var,
                                    command=lambda v=v: apply_speed(v))
    menu.add_cascade(label="Speed", menu=speed_menu)

    size_menu = tk.Menu(menu, tearoff=0)
    for v in SIZE_PRESETS:
        size_menu.add_radiobutton(label=str(v), value=v, variable=size_var,
                                   command=lambda v=v: apply_size(v))
    menu.add_cascade(label="Size", menu=size_menu)

    edge_menu = tk.Menu(menu, tearoff=0)
    for v in EDGE_BLUR_PRESETS:
        label = "Off" if v == 0 else str(v)
        edge_menu.add_radiobutton(label=label, value=v, variable=edge_var,
                                   command=lambda v=v: apply_edge_blur(v))
    menu.add_cascade(label="Edge blur", menu=edge_menu)

    sway_menu = tk.Menu(menu, tearoff=0)
    for v in SWAY_PRESETS:
        label = "Off" if v == 0 else str(v)
        sway_menu.add_radiobutton(label=label, value=v, variable=sway_var,
                                   command=lambda v=v: apply_sway(v))
    menu.add_cascade(label="Sway range", menu=sway_menu)

    fps_menu = tk.Menu(menu, tearoff=0)
    for v in FPS_PRESETS:
        fps_menu.add_radiobutton(label=str(v), value=v, variable=fps_var,
                                  command=lambda v=v: apply_fps(v))
    menu.add_cascade(label="FPS", menu=fps_menu)

    menu.add_command(label="Color…", command=pick_color)
    menu.add_separator()
    menu.add_command(label="Save settings", command=lambda: config.save(cfg))
    menu.add_command(label="Exit", command=quit_all)
    return menu


def run() -> None:
    cfg = config.load()
    monitors = _selected_monitors(cfg)
    if not monitors:
        return

    root = tk.Tk()
    root.withdraw()  # hidden root

    windows = [_make_window(root, m, cfg) for m in monitors]
    interval_ms = [max(1, int(round(1000 / cfg.fps)))]

    def quit_all(_event=None):
        try:
            root.destroy()
        except tk.TclError:
            pass

    def swallow(_event):
        return "break"

    menu = _build_menu(root, windows, cfg, interval_ms, quit_all)

    def popup(event):
        try:
            menu.tk_popup(event.x_root, event.y_root)
        finally:
            menu.grab_release()
        return "break"

    for w in windows:
        w.top.bind("<Double-Button-1>", quit_all)
        w.top.bind("<Button-1>", swallow)
        w.top.bind("<Button-3>", popup)
        w.canvas.bind("<Double-Button-1>", quit_all)
        w.canvas.bind("<Button-1>", swallow)
        w.canvas.bind("<Button-3>", popup)
        w.top.focus_force()

    start = time.perf_counter()
    last = [start]

    def tick():
        now = time.perf_counter()
        dt = now - last[0]
        last[0] = now
        t = now - start
        for w in windows:
            w.field.step(w.canvas, dt, t)
        root.after(interval_ms[0], tick)

    root.after(interval_ms[0], tick)
    root.mainloop()

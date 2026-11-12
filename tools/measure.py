"""Acceptance checks for the snow screensaver (standard library only, Windows).

  bench <exe>  resource use: CPU of the app and of dwm.exe, GPU load and
               memory, working set and frame-rate accuracy for several settings
  smoke <exe>  behaviour: picture on every monitor, menu commands, saving,
               pause on session lock, rebuild after a display change, exit

Every run copies the executable into a temporary folder next to a generated
config.json, so the real settings are never touched. The snow windows cover all
monitors while a run lasts (about one minute for bench, 30 s for smoke).

Usage:  uv run --no-project python tools/measure.py bench dist/snow.exe
        uv run --no-project python tools/measure.py smoke dist/snow.exe

CPU figures are percent of ONE logical core, taken from cycle counters.
"""
from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
ntdll = ctypes.WinDLL("ntdll")
NO_WINDOW = 0x08000000
CLASS_NAME = "SnowScreensaver"
WM_DISPLAYCHANGE, WM_CLOSE, WM_COMMAND = 0x007E, 0x0010, 0x0111
WM_LBUTTONDBLCLK, WM_WTSSESSION_CHANGE = 0x0203, 0x02B1
# Menu command ids, see src/menu.rs.
ID_COUNT, ID_SPEED, ID_SIZE, ID_EDGE_BLUR, ID_SWAY = 1000, 1100, 1200, 1300, 1400
ID_FPS, ID_RESOLUTION, ID_SAVE = 1500, 1600, 1701
pc = time.perf_counter

for fn, res, args in [
    (user32.GetDC, wt.HANDLE, [wt.HANDLE]),
    (user32.ReleaseDC, ctypes.c_int, [wt.HANDLE, wt.HANDLE]),
    (user32.PostMessageW, wt.BOOL, [wt.HANDLE, wt.UINT, ctypes.c_size_t, ctypes.c_ssize_t]),
    (user32.GetClassNameW, ctypes.c_int, [wt.HANDLE, wt.LPWSTR, ctypes.c_int]),
    (user32.GetWindowThreadProcessId, wt.DWORD, [wt.HANDLE, ctypes.POINTER(wt.DWORD)]),
    (user32.SetProcessDpiAwarenessContext, wt.BOOL, [ctypes.c_ssize_t]),
    (gdi32.CreateCompatibleDC, wt.HANDLE, [wt.HANDLE]),
    (gdi32.CreateCompatibleBitmap, wt.HANDLE, [wt.HANDLE, ctypes.c_int, ctypes.c_int]),
    (gdi32.SelectObject, wt.HANDLE, [wt.HANDLE, wt.HANDLE]),
    (gdi32.DeleteObject, wt.BOOL, [wt.HANDLE]),
    (gdi32.DeleteDC, wt.BOOL, [wt.HANDLE]),
    (gdi32.BitBlt, wt.BOOL, [wt.HANDLE] + [ctypes.c_int] * 4 + [wt.HANDLE, ctypes.c_int, ctypes.c_int, wt.DWORD]),
    (gdi32.GetDIBits, ctypes.c_int, [wt.HANDLE, wt.HANDLE, wt.UINT, wt.UINT, ctypes.c_void_p, ctypes.c_void_p, wt.UINT]),
    (k32.GetCurrentThread, wt.HANDLE, []),
    (k32.QueryThreadCycleTime, wt.BOOL, [wt.HANDLE, ctypes.POINTER(ctypes.c_uint64)]),
    (ntdll.NtQuerySystemInformation, ctypes.c_long, [ctypes.c_ulong, ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong)]),
]:
    fn.restype, fn.argtypes = res, args

ENUM_WINDOWS = ctypes.WINFUNCTYPE(wt.BOOL, wt.HANDLE, wt.LPARAM)
ENUM_MONITORS = ctypes.WINFUNCTYPE(wt.BOOL, wt.HANDLE, wt.HANDLE, ctypes.POINTER(wt.RECT), wt.LPARAM)


class UNICODE_STRING(ctypes.Structure):
    _fields_ = [("Length", ctypes.c_ushort), ("MaximumLength", ctypes.c_ushort), ("Buffer", ctypes.c_void_p)]


class SPI(ctypes.Structure):
    """SYSTEM_PROCESS_INFORMATION (x64) up to the VM counters."""
    _fields_ = [
        ("NextEntryOffset", ctypes.c_ulong), ("NumberOfThreads", ctypes.c_ulong),
        ("WorkingSetPrivateSize", ctypes.c_longlong), ("HardFaultCount", ctypes.c_ulong),
        ("NumberOfThreadsHighWatermark", ctypes.c_ulong), ("CycleTime", ctypes.c_ulonglong),
        ("CreateTime", ctypes.c_longlong), ("UserTime", ctypes.c_longlong),
        ("KernelTime", ctypes.c_longlong), ("ImageName", UNICODE_STRING),
        ("BasePriority", ctypes.c_long), ("UniqueProcessId", ctypes.c_void_p),
        ("InheritedFromUniqueProcessId", ctypes.c_void_p), ("HandleCount", ctypes.c_ulong),
        ("SessionId", ctypes.c_ulong), ("UniqueProcessKey", ctypes.c_void_p),
        ("PeakVirtualSize", ctypes.c_size_t), ("VirtualSize", ctypes.c_size_t),
        ("PageFaultCount", ctypes.c_ulong), ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t), ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t), ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t), ("PagefileUsage", ctypes.c_size_t),
    ]


def snapshot():
    """{pid: dict(name, cyc, ws, pws, commit)} for every process."""
    size = 1 << 20
    while True:
        buf = ctypes.create_string_buffer(size)
        need = ctypes.c_ulong(0)
        status = ntdll.NtQuerySystemInformation(5, buf, size, ctypes.byref(need))
        if status == 0:
            break
        if (status & 0xFFFFFFFF) != 0xC0000004:  # STATUS_INFO_LENGTH_MISMATCH
            raise OSError(f"NtQuerySystemInformation failed: {status & 0xFFFFFFFF:#x}")
        size = max(size * 2, need.value + 65536)
    out, off = {}, 0
    while True:
        e = SPI.from_buffer(buf, off)
        name = ctypes.wstring_at(e.ImageName.Buffer, e.ImageName.Length // 2) if e.ImageName.Buffer else ""
        out[e.UniqueProcessId or 0] = dict(name=name.lower(), cyc=e.CycleTime, ws=e.WorkingSetSize,
                                           pws=e.WorkingSetPrivateSize, commit=e.PagefileUsage)
        if e.NextEntryOffset == 0:
            return out
        off += e.NextEntryOffset


def calibrate_cycles_per_second() -> float:
    """Cycle-counter ticks per second of on-CPU time (best of a few spins)."""
    def cycles():
        v = ctypes.c_uint64()
        k32.QueryThreadCycleTime(k32.GetCurrentThread(), ctypes.byref(v))
        return v.value
    best = 0.0
    for _ in range(5):
        c0, t0 = cycles(), pc()
        while pc() - t0 < 0.1:
            pass
        best = max(best, (cycles() - c0) / (pc() - t0))
    return best


CYCLES_PER_SECOND = 0.0


def cpu_over(pid, seconds):
    """(app %, dwm %, final snapshot) over `seconds`, in percent of one core."""
    a, t0 = snapshot(), pc()
    time.sleep(seconds)
    b, wall = snapshot(), pc() - t0
    def used(match):
        total = sum(v["cyc"] - a[p]["cyc"] for p, v in b.items() if p in a and match(p, v))
        return total / CYCLES_PER_SECOND / wall * 100
    return used(lambda p, v: p == pid), used(lambda p, v: v["name"] == "dwm.exe"), b


def gpu_counters(pid, dwm_pids, samples):
    """Starts a PowerShell sampler for per-process GPU engine load and GPU memory."""
    paths = [f"\\GPU Engine(pid_{p}_*)\\Utilization Percentage" for p in [pid, *dwm_pids]]
    paths.append(f"\\GPU Process Memory(pid_{pid}_*)\\Dedicated Usage")
    quoted = ",".join("'" + p + "'" for p in paths)
    script = (f"Get-Counter -Counter {quoted} -SampleInterval 1 -MaxSamples {samples} "
              "-ErrorAction SilentlyContinue | ForEach-Object { foreach ($s in $_.CounterSamples) "
              "{ '{0}|{1}' -f $s.Path, $s.CookedValue }; '---' }")
    return subprocess.Popen(["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
                            creationflags=NO_WINDOW)


def read_gpu_counters(proc, pid):
    """Average (app engine %, dwm engine %, app dedicated MB), or None if unavailable."""
    try:
        text, _ = proc.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        proc.kill()
        return None
    blocks = [b for b in text.split("---") if b.strip()]
    if not blocks:
        return None
    acc = [0.0, 0.0, 0.0]
    for block in blocks:
        for line in block.splitlines():
            path, _, value = line.strip().rpartition("|")
            if "(pid_" not in path:
                continue
            try:
                v = float(value)
            except ValueError:
                continue
            owner = int(path.lower().split("(pid_")[1].split("_")[0])
            if "gpu engine" in path.lower():
                acc[0 if owner == pid else 1] += v
            else:
                acc[2] += v / 2**20
    return [x / len(blocks) for x in acc]


def monitors():
    """Monitor rectangles (x, y, width, height) in enumeration order."""
    rects = []

    @ENUM_MONITORS
    def collect(_mon, _dc, rect, _data):
        r = rect.contents
        rects.append((r.left, r.top, r.right - r.left, r.bottom - r.top))
        return True

    user32.EnumDisplayMonitors(None, None, collect, 0)
    return rects


def app_windows(pid):
    found = []

    @ENUM_WINDOWS
    def collect(hwnd, _data):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        name = ctypes.create_unicode_buffer(64)
        user32.GetClassNameW(hwnd, name, 64)
        if owner.value == pid and name.value == CLASS_NAME:
            found.append(hwnd)
        return True

    user32.EnumWindows(collect, 0)
    return found


def post(pid, message, wparam=0):
    windows = app_windows(pid)
    if windows:
        user32.PostMessageW(windows[0], message, wparam, 0)
    return bool(windows)


def capture(rect):
    """Screen pixels of `rect` as top-down BGRA bytes."""
    x, y, w, h = rect
    screen = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(screen)
    bitmap = gdi32.CreateCompatibleBitmap(screen, w, h)
    old = gdi32.SelectObject(mem, bitmap)
    gdi32.BitBlt(mem, 0, 0, w, h, screen, x, y, 0x00CC0020)
    header = struct.pack("<IiiHHIIiiII", 40, w, -h, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = ctypes.create_string_buffer(w * h * 4)
    gdi32.GetDIBits(mem, bitmap, 0, h, pixels, header, 0)
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, screen)
    return pixels.raw


def picture_stats(bgra):
    """(fraction of lit pixels, brightest (r, g, b)) of a captured monitor."""
    blue, green, red = bgra[0::4], bgra[1::4], bgra[2::4]
    brightest = max((red, green, blue), key=max)
    lit = len(brightest) - brightest.count(0)
    return lit / len(brightest), (max(red), max(green), max(blue))


def save_png(path, bgra, width, height, crop=(1280, 720)):
    """Writes the centre `crop` of a capture as an RGB PNG."""
    cw, ch = min(crop[0], width), min(crop[1], height)
    x0, y0 = (width - cw) // 2, (height - ch) // 2
    rows = []
    for y in range(y0, y0 + ch):
        line = bgra[(y * width + x0) * 4:(y * width + x0 + cw) * 4]
        rgb = bytearray(cw * 3)
        rgb[0::3], rgb[1::3], rgb[2::3] = line[2::4], line[1::4], line[0::4]
        rows.append(b"\x00" + bytes(rgb))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", cw, ch, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(b"".join(rows), 6)) + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


class App:
    """The executable running from a private folder with its own config.json."""

    def __init__(self, exe, config):
        self.dir = tempfile.mkdtemp(prefix="snow-measure-")
        self.exe = shutil.copy(exe, os.path.join(self.dir, "snow.exe"))
        self.config_path = os.path.join(self.dir, "config.json")
        self.stats_path = os.path.join(self.dir, "stats.txt")
        with open(self.config_path, "w", encoding="utf-8") as f:
            json.dump(config, f)
        env = dict(os.environ, SNOW_STATS_FILE=self.stats_path)
        self.proc = subprocess.Popen([self.exe], env=env)
        self.pid = self.proc.pid

    def stop(self, message=WM_CLOSE):
        """Asks the app to exit; returns (exit code, frame statistics)."""
        post(self.pid, message)
        try:
            code = self.proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            code = None
        stats = {}
        if os.path.exists(self.stats_path):
            with open(self.stats_path) as f:
                stats = dict(item.split("=") for item in f.read().split())
        shutil.rmtree(self.dir, ignore_errors=True)
        return code, stats

    def saved_config(self):
        with open(self.config_path, encoding="utf-8") as f:
            return json.load(f)


def base_config(exe):
    """Settings next to the executable, or the built-in defaults."""
    path = os.path.join(os.path.dirname(os.path.abspath(exe)), "config.json")
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def bench(exe):
    base = base_config(exe)
    fps = base.get("fps", 15)
    variants = [("static", {"fps": 1}), ("configured", {}), ("fps10", {"fps": 10}),
                ("fps30", {"fps": 30}), ("res100", {"resolution": 100})]
    warm, meas = 4.5, 6.0
    print(f"config: {base or 'defaults'}; warm {warm}s, measure {meas}s per variant")
    rows = []
    for label, overrides in variants:
        app = App(exe, {**base, **overrides})
        try:
            time.sleep(warm)
            if app.proc.poll() is not None:
                raise SystemExit(f"{label}: the app exited early (code {app.proc.returncode})")
            dwm_pids = [p for p, v in snapshot().items() if v["name"] == "dwm.exe"]
            sampler = gpu_counters(app.pid, dwm_pids, int(meas) - 2)
            cpu, dwm, snap = cpu_over(app.pid, meas)
            gpu = read_gpu_counters(sampler, app.pid)
            mem = snap[app.pid]
        finally:
            _code, stats = app.stop()
        rows.append(dict(label=label, target=overrides.get("fps", fps), cpu=cpu, dwm=dwm, gpu=gpu,
                         ws=mem["ws"] / 2**20, pws=mem["pws"] / 2**20, commit=mem["commit"] / 2**20,
                         fps=float(stats.get("fps", "nan"))))
        time.sleep(0.5)

    static = rows[0]["dwm"]
    print(f"\n{'variant':11s} {'fps':>7s} {'app CPU%':>9s} {'dwm +pt':>8s} {'app GPU%':>9s} "
          f"{'dwm GPU%':>9s} {'ws MB':>6s} {'priv ws':>8s} {'commit':>7s} {'vram MB':>8s}")
    for r in rows:
        g = r["gpu"] or [float("nan")] * 3
        print(f"{r['label']:11s} {r['fps']:7.2f} {r['cpu']:9.3f} {r['dwm'] - static:+8.2f} {g[0]:9.2f} "
              f"{g[1]:9.2f} {r['ws']:6.1f} {r['pws']:8.1f} {r['commit']:7.1f} {g[2]:8.1f}")
    print(f"(dwm.exe with the static control: {static:.2f}% of one core)")

    cfg = rows[1]
    size_kb = os.path.getsize(exe) / 1024
    checks = [
        ("app CPU <= 1.0% of one core", cfg["cpu"] <= 1.0, f"{cfg['cpu']:.3f}%"),
        ("dwm.exe increase <= +1.5 points", cfg["dwm"] - static <= 1.5, f"{cfg['dwm'] - static:+.2f}"),
        ("private working set <= 5 MB", cfg["pws"] <= 5.0, f"{cfg['pws']:.1f} MB"),
        ("working set <= 10 MB", cfg["ws"] <= 10.0, f"{cfg['ws']:.1f} MB"),
        ("executable <= 1 MB", size_kb <= 1024, f"{size_kb:.0f} KB"),
    ]
    for r in rows[1:4]:
        error = abs(r["fps"] - r["target"]) / r["target"] * 100
        checks.append((f"frame rate within 3% of {r['target']}", error <= 3.0, f"{r['fps']:.2f} fps"))
    return report(checks)


def smoke(exe):
    # A file as the former Python version wrote it: no fps or resolution keys.
    config = {"count": 200, "speed": 0.3, "size": 18, "edge_blur": 2, "sway_amp": 10,
              "color": "#c4c0fa", "monitors": "all"}
    shots = tempfile.mkdtemp(prefix="snow-shots-")
    rects = monitors()
    checks = []
    app = App(exe, config)
    try:
        time.sleep(3.0)
        windows = app_windows(app.pid)
        checks.append(("one window per monitor", len(windows) == len(rects), f"{len(windows)} of {len(rects)}"))
        first = []
        for i, rect in enumerate(rects):
            pixels = capture(rect)
            lit, colour = picture_stats(pixels)
            first.append(lit)
            save_png(os.path.join(shots, f"monitor{i}_start.png"), pixels, rect[2], rect[3])
            near = all(abs(a - b) <= 6 for a, b in zip(colour, (0xC4, 0xC0, 0xFA)))
            checks.append((f"monitor {i}: snow is drawn", lit > 1e-4, f"{lit * 100:.3f}% lit"))
            checks.append((f"monitor {i}: configured colour", near, "#%02x%02x%02x" % colour))

        for command in (ID_COUNT + 5, ID_SPEED + 4, ID_SIZE + 7, ID_EDGE_BLUR + 4, ID_SWAY + 4,
                        ID_FPS + 5, ID_RESOLUTION + 0, ID_SAVE):
            post(app.pid, WM_COMMAND, command)
        time.sleep(4.0)
        saved = app.saved_config()
        want = {"count": 400, "speed": 1, "size": 25, "edge_blur": 5, "sway_amp": 30, "fps": 30,
                "resolution": 100, "color": "#c4c0fa", "monitors": "all"}
        wrong = {k: saved.get(k) for k, v in want.items() if saved.get(k) != v}
        checks.append(("menu commands apply and are saved", not wrong, str(wrong or "all 9 keys")))
        for i, rect in enumerate(rects):
            pixels = capture(rect)
            lit, _ = picture_stats(pixels)
            save_png(os.path.join(shots, f"monitor{i}_changed.png"), pixels, rect[2], rect[3])
            checks.append((f"monitor {i}: more and larger flakes", lit > first[i] * 1.5,
                           f"{first[i] * 100:.3f}% -> {lit * 100:.3f}% lit"))

        post(app.pid, WM_WTSSESSION_CHANGE, 7)  # WTS_SESSION_LOCK
        time.sleep(0.5)
        locked, _, _ = cpu_over(app.pid, 3.0)
        post(app.pid, WM_WTSSESSION_CHANGE, 8)  # WTS_SESSION_UNLOCK
        time.sleep(0.5)
        unlocked, _, _ = cpu_over(app.pid, 3.0)
        # Paused, only stray window messages and driver threads cost anything.
        paused = locked < 0.1 and locked < unlocked * 0.2
        checks.append(("locked session: drawing stops", paused, f"{locked:.3f}%"))
        checks.append(("unlocked session: drawing again", unlocked > 0.2, f"{unlocked:.3f}%"))

        post(app.pid, WM_DISPLAYCHANGE)
        time.sleep(3.0)
        rebuilt = app_windows(app.pid)
        alive = app.proc.poll() is None
        fresh = alive and len(rebuilt) == len(rects) and not set(rebuilt) & set(windows)
        checks.append(("display change: windows rebuilt", fresh, f"{len(rebuilt)} new windows"))
        lit, _ = picture_stats(capture(rects[0]))
        checks.append(("display change: snow is drawn again", lit > 1e-4, f"{lit * 100:.3f}% lit"))
    finally:
        code, stats = app.stop(WM_LBUTTONDBLCLK)
    checks.append(("double-click exits cleanly", code == 0, f"exit code {code}"))
    checks.append(("frame statistics written", "fps" in stats, str(stats)))
    print(f"screenshots: {shots}")
    return report(checks)


def report(checks):
    print()
    for name, ok, detail in checks:
        print(f"  {'PASS' if ok else 'FAIL'}  {name}: {detail}")
    failed = sum(not ok for _, ok, _ in checks)
    print(f"\n{len(checks) - failed} of {len(checks)} checks passed")
    return 1 if failed else 0


def main():
    global CYCLES_PER_SECOND
    if len(sys.argv) != 3 or sys.argv[1] not in ("bench", "smoke"):
        raise SystemExit(__doc__)
    exe = os.path.abspath(sys.argv[2])
    user32.SetProcessDpiAwarenessContext(-4)  # physical pixels on every monitor
    CYCLES_PER_SECOND = calibrate_cycles_per_second()
    sys.exit(bench(exe) if sys.argv[1] == "bench" else smoke(exe))


if __name__ == "__main__":
    main()

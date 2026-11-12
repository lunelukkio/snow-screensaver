//! Snow screensaver: one topmost full-screen window per selected monitor,
//! drawn with Direct3D 11 at a low, timer-paced frame rate.
//!
//! Layers: `flakes` (particle model), `render` (Direct3D), `menu` (right-click
//! settings), `config` (persisted settings). This file owns the windows, the
//! message loop and the frame pacing.
//!
//! The window procedure never touches application state: it only queues an
//! `Event`, and the main loop handles the queue. That keeps modal calls (the
//! popup menu, the colour dialog) free of re-entrancy problems.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod flakes;
mod menu;
mod render;

use std::cell::RefCell;
use std::f64::consts::TAU;
use std::mem::size_of;
use std::time::Duration;

use windows::core::{w, Error, Result, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    E_FAIL, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Power::{
    RegisterPowerSettingNotification, UnregisterPowerSettingNotification, HPOWERNOTIFY,
    POWERBROADCAST_SETTING,
};
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, GetCurrentProcess, SetProcessWorkingSetSize, SetWaitableTimer,
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use config::{Config, MAX_COUNT};
use flakes::{FlakeField, Rng, SWAY_FREQ};
use menu::Command;
use render::{Look, Renderer};

const CLASS_NAME: PCWSTR = w!("SnowScreensaver");
const APP_TITLE: PCWSTR = w!("Snow Screensaver");

/// Longest time step fed to the model, so flakes do not jump after a pause
/// (open menu, display off, locked session).
const MAX_DT: f32 = 0.5;

/// Frames to wait after start-up or a menu action before handing memory that
/// is no longer touched back to the system.
const TRIM_DELAY_FRAMES: u32 = 30;

/// How often a failed device or window rebuild is retried, one second apart.
const REBUILD_ATTEMPTS: u32 = 10;

/// What the window procedure saw; handled by the main loop.
enum Event {
    Quit,
    Menu(HWND, i32, i32),
    Command(u32),
    DisplayOn(bool),
    Locked(bool),
    DisplayChanged,
}

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

fn push(event: Event) {
    EVENTS.with(|queue| queue.borrow_mut().push(event));
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Double-click is the deliberate exit gesture; single clicks and keys do nothing.
        WM_LBUTTONDBLCLK | WM_CLOSE => push(Event::Quit),
        WM_RBUTTONUP => {
            let mut at = POINT::default();
            let _ = GetCursorPos(&mut at);
            push(Event::Menu(hwnd, at.x, at.y));
        }
        WM_COMMAND => push(Event::Command((wparam.0 & 0xffff) as u32)),
        WM_POWERBROADCAST => {
            if wparam.0 as u32 == PBT_POWERSETTINGCHANGE && lparam.0 != 0 {
                let setting = &*(lparam.0 as *const POWERBROADCAST_SETTING);
                if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                    // 0 = off, 1 = on, 2 = dimmed
                    push(Event::DisplayOn(setting.Data[0] != 0));
                }
            }
            return LRESULT(1);
        }
        WM_WTSSESSION_CHANGE => match wparam.0 as u32 {
            WTS_SESSION_LOCK => push(Event::Locked(true)),
            WTS_SESSION_UNLOCK => push(Event::Locked(false)),
            _ => {}
        },
        WM_DISPLAYCHANGE => push(Event::DisplayChanged),
        WM_SETCURSOR => {
            SetCursor(None); // hide the pointer over the snow
            return LRESULT(1);
        }
        WM_ERASEBKGND => return LRESULT(1),
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

unsafe extern "system" fn monitor_proc(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let rects = &mut *(data.0 as *mut Vec<RECT>);
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        rects.push(info.rcMonitor);
    }
    true.into()
}

/// Monitor rectangles in enumeration order (the order `config.json` indexes).
unsafe fn monitor_rects() -> Vec<RECT> {
    let mut rects: Vec<RECT> = Vec::new();
    let data = LPARAM(&mut rects as *mut Vec<RECT> as isize);
    let _ = EnumDisplayMonitors(None, None, Some(monitor_proc), data);
    rects
}

fn ticks() -> i64 {
    let mut value = 0;
    // Cannot fail on any supported Windows version.
    let _ = unsafe { QueryPerformanceCounter(&mut value) };
    value
}

fn look(cfg: &Config) -> Look {
    Look {
        size: cfg.size as f32,
        edge_blur: cfg.edge_blur as f32,
        color: cfg.color,
        resolution: cfg.resolution,
    }
}

struct App {
    instance: HINSTANCE,
    cfg: Config,
    rng: Rng,
    windows: Vec<HWND>,
    fields: Vec<FlakeField>,
    renderer: Option<Renderer>,
    power_notify: Option<HPOWERNOTIFY>,
    /// Window the menu was last opened on; owns the colour dialog.
    menu_owner: Option<HWND>,
    display_on: bool,
    locked: bool,
    quit: bool,
    rebuild: bool,
    /// Set when a modal menu or dialog has just held up the main loop.
    blocked: bool,
    trim_in: u32,
}

impl App {
    fn new(instance: HINSTANCE, cfg: Config) -> App {
        App {
            instance,
            cfg,
            rng: Rng::new(ticks() as u64),
            windows: Vec::new(),
            fields: Vec::new(),
            renderer: None,
            power_notify: None,
            menu_owner: None,
            display_on: true,
            locked: false,
            quit: false,
            rebuild: false,
            blocked: false,
            trim_in: 0,
        }
    }

    /// Creates the windows, the renderer and the flake fields for the
    /// currently attached monitors.
    unsafe fn build(&mut self) -> Result<()> {
        let rects = self.cfg.monitors.select(&monitor_rects());
        if rects.is_empty() {
            return Err(Error::new(E_FAIL, "no monitor found"));
        }
        let mut targets = Vec::with_capacity(rects.len());
        for rect in &rects {
            let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                CLASS_NAME,
                APP_TITLE,
                WS_POPUP,
                rect.left,
                rect.top,
                width,
                height,
                None,
                None,
                Some(self.instance),
                None,
            )?;
            self.windows.push(hwnd);
            targets.push((hwnd, width as u32, height as u32));
            self.fields.push(FlakeField::new(
                width as f32,
                height as f32,
                self.cfg.count as usize,
                self.cfg.speed,
                &mut self.rng,
            ));
        }
        self.renderer = Some(Renderer::new(
            &targets,
            &look(&self.cfg),
            MAX_COUNT as usize,
        )?);

        // One window is enough to hear about display power and session lock.
        let first = self.windows[0];
        self.power_notify = RegisterPowerSettingNotification(
            HANDLE(first.0),
            &GUID_CONSOLE_DISPLAY_STATE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        )
        .ok();
        let _ = WTSRegisterSessionNotification(first, NOTIFY_FOR_THIS_SESSION);

        for &hwnd in &self.windows {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(first);
        self.trim_in = TRIM_DELAY_FRAMES;
        Ok(())
    }

    unsafe fn teardown(&mut self) {
        self.renderer = None; // swap chains must go before their windows
        if let Some(handle) = self.power_notify.take() {
            let _ = UnregisterPowerSettingNotification(handle);
        }
        if let Some(&first) = self.windows.first() {
            let _ = WTSUnRegisterSessionNotification(first);
        }
        for hwnd in self.windows.drain(..) {
            let _ = DestroyWindow(hwnd);
        }
        self.fields.clear();
        self.menu_owner = None;
    }

    /// Tears everything down and builds it again; used after a lost device or
    /// a changed monitor layout. Retries because a restarting driver needs time.
    unsafe fn rebuild(&mut self) -> Result<()> {
        let mut attempt = 0;
        loop {
            self.teardown();
            match self.build() {
                Ok(()) => return Ok(()),
                Err(error) if attempt + 1 >= REBUILD_ATTEMPTS => {
                    self.teardown();
                    return Err(error);
                }
                Err(_) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }

    unsafe fn handle(&mut self, event: Event) {
        match event {
            Event::Quit => self.quit = true,
            Event::Menu(hwnd, x, y) => {
                self.menu_owner = Some(hwnd);
                let _ = menu::show(hwnd, x, y, &self.cfg);
                self.blocked = true;
                self.trim_in = TRIM_DELAY_FRAMES;
            }
            Event::Command(id) => {
                if let Some(command) = menu::command_from_id(id) {
                    self.apply(command);
                    self.trim_in = TRIM_DELAY_FRAMES;
                }
            }
            Event::DisplayOn(on) => self.display_on = on,
            Event::Locked(locked) => self.locked = locked,
            Event::DisplayChanged => self.rebuild = true,
        }
    }

    unsafe fn apply(&mut self, command: Command) {
        match command {
            Command::Count(count) => {
                self.cfg.count = count;
                for field in &mut self.fields {
                    field.set_count(count as usize, self.cfg.speed, &mut self.rng);
                }
            }
            Command::Speed(speed) => {
                self.cfg.speed = speed;
                for field in &mut self.fields {
                    field.set_speed(speed, &mut self.rng);
                }
            }
            Command::Size(size) => {
                self.cfg.size = size;
                self.update_look();
            }
            Command::EdgeBlur(edge_blur) => {
                self.cfg.edge_blur = edge_blur;
                self.update_look();
            }
            Command::SwayAmp(sway_amp) => self.cfg.sway_amp = sway_amp,
            Command::Fps(fps) => self.cfg.fps = fps,
            Command::Resolution(resolution) => {
                self.cfg.resolution = resolution;
                self.update_look();
            }
            Command::PickColor => {
                self.blocked = true;
                let owner = self.menu_owner.or(self.windows.first().copied());
                if let Some(color) = owner.and_then(|owner| menu::pick_color(owner, self.cfg.color))
                {
                    self.cfg.color = color;
                    self.update_look();
                }
            }
            Command::Save => {
                if let Err(error) = self.cfg.save() {
                    self.blocked = true;
                    let text = HSTRING::from(format!("Could not save config.json.\n\n{error}"));
                    MessageBoxW(
                        self.windows.first().copied(),
                        &text,
                        APP_TITLE,
                        MB_ICONWARNING | MB_OK,
                    );
                }
            }
            Command::Exit => self.quit = true,
        }
    }

    unsafe fn update_look(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            if renderer.set_look(&look(&self.cfg)).is_err() {
                self.rebuild = true;
            }
        }
    }

    /// Advances and draws every monitor. An error means the device was lost.
    unsafe fn frame(&mut self, dt: f32, sway_phase: f32) -> Result<()> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Err(E_FAIL.into());
        };
        let (size, sway_amp) = (self.cfg.size as f32, self.cfg.sway_amp as f32);
        let rng = &mut self.rng;
        for (index, field) in self.fields.iter_mut().enumerate() {
            let count = field.count();
            renderer.draw(index, count, |centres, scale| {
                field.step(dt, sway_phase, size, sway_amp, scale, rng, centres)
            })?;
        }
        Ok(())
    }

    /// Counts down to the one-off working-set trim.
    unsafe fn after_frame(&mut self) {
        if self.trim_in > 0 {
            self.trim_in -= 1;
            if self.trim_in == 0 {
                // Start-up code and menu resources are not needed again; this
                // lets the system reclaim their pages.
                let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
            }
        }
    }
}

/// Frame counter for `tools/measure.py`: when the `SNOW_STATS_FILE` environment
/// variable names a file, the measured frame rate is written there on exit.
#[derive(Default)]
struct FrameStats {
    frames: u64,
    first: i64,
    last: i64,
}

impl FrameStats {
    fn record(&mut self, now: i64) {
        if self.frames == 0 {
            self.first = now;
        }
        self.frames += 1;
        self.last = now;
    }

    fn write(&self, frequency: f64) {
        let Ok(path) = std::env::var("SNOW_STATS_FILE") else {
            return;
        };
        let seconds = (self.last - self.first) as f64 / frequency;
        let fps = if seconds > 0.0 {
            (self.frames - 1) as f64 / seconds
        } else {
            0.0
        };
        let text = format!("frames={} seconds={seconds:.3} fps={fps:.3}\n", self.frames);
        let _ = std::fs::write(path, text);
    }
}

/// Sleeps until `ticks` performance-counter ticks have passed or a message arrives.
unsafe fn wait(timer: Option<HANDLE>, ticks: i64, frequency: f64) {
    let seconds = ticks as f64 / frequency;
    if let Some(timer) = timer {
        let due = -((seconds * 1e7) as i64).max(1); // relative, in 100 ns units
        if SetWaitableTimer(timer, &due, 0, None, None, false).is_ok() {
            MsgWaitForMultipleObjectsEx(Some(&[timer]), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            return;
        }
    }
    let milliseconds = (seconds * 1e3).ceil().max(1.0) as u32;
    MsgWaitForMultipleObjectsEx(None, milliseconds, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
}

unsafe fn run() -> Result<()> {
    // Work in physical pixels on every monitor.
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let instance: HINSTANCE = GetModuleHandleW(None)?.into();
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_DBLCLKS,
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        // Resource id 1 (MAKEINTRESOURCE) is the icon embedded by build.rs.
        hIcon: LoadIconW(Some(instance), PCWSTR(std::ptr::without_provenance(1)))
            .unwrap_or_default(),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if RegisterClassExW(&class) == 0 {
        return Err(Error::from_thread());
    }

    let mut app = App::new(instance, Config::load());
    if let Err(error) = app.build() {
        app.teardown();
        return Err(error);
    }

    let mut frequency = 0i64;
    QueryPerformanceFrequency(&mut frequency)?;
    let frequency = frequency as f64;
    // A high-resolution timer keeps the frame rate exact without raising the
    // system timer rate; without it (before Windows 10 1803) waits are coarse.
    let timer = CreateWaitableTimerExW(
        None,
        PCWSTR::null(),
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
        TIMER_ALL_ACCESS.0,
    )
    .ok();

    let start = ticks();
    let mut last = start;
    let mut next = start;
    let mut stats = FrameStats::default();
    let mut msg = MSG::default();
    loop {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let idle = EVENTS.with(|queue| queue.borrow().is_empty());
        if !idle {
            let events = EVENTS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
            for event in events {
                app.handle(event);
            }
            if std::mem::take(&mut app.blocked) {
                // A menu or dialog held the loop: do not count that time as motion.
                last = ticks();
            }
        }
        if app.quit {
            break;
        }
        if app.rebuild {
            app.rebuild = false;
            app.rebuild()?;
            last = ticks();
            next = last;
            continue;
        }
        if !app.display_on || app.locked {
            // Nothing can be seen: no frames, no timer, just wait for a message.
            let _ = WaitMessage();
            last = ticks();
            next = last;
            continue;
        }

        let now = ticks();
        if now >= next {
            let frame_seconds = 1.0 / app.cfg.fps.max(1) as f64;
            // Never clamp below two frame intervals, or a low FPS would slow the snow.
            let max_dt = MAX_DT.max(2.0 * frame_seconds as f32);
            let dt = (((now - last) as f64 / frequency) as f32).min(max_dt);
            let sway_phase = (((now - start) as f64 / frequency * SWAY_FREQ) % TAU) as f32;
            last = now;
            if app.frame(dt, sway_phase).is_err() {
                app.rebuild = true;
                continue;
            }
            app.after_frame();
            stats.record(now);
            let interval = (frequency * frame_seconds) as i64;
            next += interval;
            if next <= now {
                next = now + interval; // fell behind: drop the backlog
            }
            continue;
        }
        wait(timer, next - now, frequency);
    }
    app.teardown();
    stats.write(frequency);
    Ok(())
}

fn main() {
    if let Err(error) = unsafe { run() } {
        let text = HSTRING::from(format!("Snow Screensaver could not run.\n\n{error}"));
        unsafe { MessageBoxW(None, &text, APP_TITLE, MB_ICONERROR | MB_OK) };
        std::process::exit(1);
    }
}

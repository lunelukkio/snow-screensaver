//! Right-click settings menu and the colour dialog.
//!
//! Every menu item carries a stable command id: one block of 100 ids per
//! setting, the offset being the preset index. Choosing an item makes Windows
//! post `WM_COMMAND` with that id to the owner window, and `command_from_id`
//! turns the id into a `Command`. Nothing here is on the per-frame path.

use std::mem::size_of;

use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, DestroyMenu, PostMessageW,
    SetForegroundWindow, TrackPopupMenu, HMENU, MF_BYCOMMAND, MF_POPUP, MF_SEPARATOR, MF_STRING,
    TPM_RIGHTBUTTON, WM_NULL,
};

use crate::config::Config;

pub const COUNT_PRESETS: [u32; 11] = [10, 25, 50, 100, 200, 400, 800, 1500, 3000, 5000, 10000];
pub const SPEED_PRESETS: [f32; 13] = [
    0.1, 0.3, 0.5, 0.8, 1.0, 1.5, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0, 30.0,
];
pub const SIZE_PRESETS: [u32; 10] = [1, 2, 3, 5, 8, 12, 18, 25, 35, 50];
pub const EDGE_BLUR_PRESETS: [u32; 9] = [0, 1, 2, 3, 5, 8, 12, 20, 30];
pub const SWAY_PRESETS: [u32; 9] = [0, 5, 10, 18, 30, 50, 80, 120, 200];
pub const FPS_PRESETS: [u32; 10] = [5, 10, 15, 20, 24, 30, 45, 60, 90, 120];
pub const RESOLUTION_PRESETS: [u32; 4] = [100, 50, 33, 25];

pub const ID_COUNT: u32 = 1000;
pub const ID_SPEED: u32 = 1100;
pub const ID_SIZE: u32 = 1200;
pub const ID_EDGE_BLUR: u32 = 1300;
pub const ID_SWAY: u32 = 1400;
pub const ID_FPS: u32 = 1500;
pub const ID_RESOLUTION: u32 = 1600;
pub const ID_COLOR: u32 = 1700;
pub const ID_SAVE: u32 = 1701;
pub const ID_EXIT: u32 = 1702;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Count(u32),
    Speed(f32),
    Size(u32),
    EdgeBlur(u32),
    SwayAmp(u32),
    Fps(u32),
    Resolution(u32),
    PickColor,
    Save,
    Exit,
}

/// Maps a `WM_COMMAND` id back to the setting it stands for.
pub fn command_from_id(id: u32) -> Option<Command> {
    fn preset<T: Copy>(presets: &[T], id: u32, base: u32) -> Option<T> {
        presets.get((id - base) as usize).copied()
    }
    match id {
        ID_COUNT..=1099 => preset(&COUNT_PRESETS, id, ID_COUNT).map(Command::Count),
        ID_SPEED..=1199 => preset(&SPEED_PRESETS, id, ID_SPEED).map(Command::Speed),
        ID_SIZE..=1299 => preset(&SIZE_PRESETS, id, ID_SIZE).map(Command::Size),
        ID_EDGE_BLUR..=1399 => preset(&EDGE_BLUR_PRESETS, id, ID_EDGE_BLUR).map(Command::EdgeBlur),
        ID_SWAY..=1499 => preset(&SWAY_PRESETS, id, ID_SWAY).map(Command::SwayAmp),
        ID_FPS..=1599 => preset(&FPS_PRESETS, id, ID_FPS).map(Command::Fps),
        ID_RESOLUTION..=1699 => {
            preset(&RESOLUTION_PRESETS, id, ID_RESOLUTION).map(Command::Resolution)
        }
        ID_COLOR => Some(Command::PickColor),
        ID_SAVE => Some(Command::Save),
        ID_EXIT => Some(Command::Exit),
        _ => None,
    }
}

fn off_or_number(v: &u32) -> String {
    if *v == 0 {
        "Off".to_string()
    } else {
        v.to_string()
    }
}

/// Adds one cascading submenu of radio items; `current` marks the active one.
unsafe fn add_submenu<T: PartialEq>(
    root: HMENU,
    title: PCWSTR,
    base: u32,
    presets: &[T],
    current: T,
    label: impl Fn(&T) -> String,
) -> Result<()> {
    let sub = CreatePopupMenu()?;
    for (i, value) in presets.iter().enumerate() {
        let text = HSTRING::from(label(value));
        AppendMenuW(sub, MF_STRING, (base + i as u32) as usize, &text)?;
    }
    if let Some(i) = presets.iter().position(|v| *v == current) {
        let last = base + presets.len() as u32 - 1;
        CheckMenuRadioItem(sub, base, last, base + i as u32, MF_BYCOMMAND.0)?;
    }
    AppendMenuW(root, MF_POPUP, sub.0 as usize, title)
}

unsafe fn build(root: HMENU, cfg: &Config) -> Result<()> {
    let number = |v: &u32| v.to_string();
    add_submenu(
        root,
        w!("Count"),
        ID_COUNT,
        &COUNT_PRESETS,
        cfg.count,
        number,
    )?;
    add_submenu(
        root,
        w!("Speed"),
        ID_SPEED,
        &SPEED_PRESETS,
        cfg.speed,
        |v| v.to_string(),
    )?;
    add_submenu(root, w!("Size"), ID_SIZE, &SIZE_PRESETS, cfg.size, number)?;
    add_submenu(
        root,
        w!("Edge blur"),
        ID_EDGE_BLUR,
        &EDGE_BLUR_PRESETS,
        cfg.edge_blur,
        off_or_number,
    )?;
    add_submenu(
        root,
        w!("Sway range"),
        ID_SWAY,
        &SWAY_PRESETS,
        cfg.sway_amp,
        off_or_number,
    )?;
    add_submenu(root, w!("FPS"), ID_FPS, &FPS_PRESETS, cfg.fps, number)?;
    add_submenu(
        root,
        w!("Resolution"),
        ID_RESOLUTION,
        &RESOLUTION_PRESETS,
        cfg.resolution,
        |v| format!("{v}%"),
    )?;
    AppendMenuW(root, MF_STRING, ID_COLOR as usize, w!("Color…"))?;
    AppendMenuW(root, MF_SEPARATOR, 0, PCWSTR::null())?;
    AppendMenuW(root, MF_STRING, ID_SAVE as usize, w!("Save settings"))?;
    AppendMenuW(root, MF_STRING, ID_EXIT as usize, w!("Exit"))
}

/// Shows the settings menu at screen position `(x, y)` and blocks until it
/// closes. A chosen item arrives later as `WM_COMMAND` on `owner`.
pub unsafe fn show(owner: HWND, x: i32, y: i32, cfg: &Config) -> Result<()> {
    let root = CreatePopupMenu()?;
    let built = build(root, cfg);
    if built.is_ok() {
        // Without foreground status the menu would not close on an outside click.
        let _ = SetForegroundWindow(owner);
        let _ = TrackPopupMenu(root, TPM_RIGHTBUTTON, x, y, None, owner, None);
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
    }
    let _ = DestroyMenu(root); // also destroys the submenus
    built
}

/// Opens the system colour dialog owned by `owner` (so it stays above the
/// topmost snow windows). Returns `None` when cancelled.
pub unsafe fn pick_color(owner: HWND, current: [u8; 3]) -> Option<[u8; 3]> {
    let mut custom = [COLORREF(0x00FF_FFFF); 16];
    let [r, g, b] = current.map(u32::from);
    let mut dialog = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: COLORREF(r | (g << 8) | (b << 16)),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN,
        ..Default::default()
    };
    if !ChooseColorW(&mut dialog).as_bool() {
        return None;
    }
    let v = dialog.rgbResult.0;
    Some([v as u8, (v >> 8) as u8, (v >> 16) as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_their_presets() {
        assert_eq!(command_from_id(ID_COUNT), Some(Command::Count(10)));
        assert_eq!(command_from_id(ID_COUNT + 10), Some(Command::Count(10000)));
        assert_eq!(command_from_id(ID_SPEED + 1), Some(Command::Speed(0.3)));
        assert_eq!(command_from_id(ID_SIZE + 6), Some(Command::Size(18)));
        assert_eq!(command_from_id(ID_EDGE_BLUR), Some(Command::EdgeBlur(0)));
        assert_eq!(command_from_id(ID_SWAY + 2), Some(Command::SwayAmp(10)));
        assert_eq!(command_from_id(ID_FPS), Some(Command::Fps(5)));
        assert_eq!(command_from_id(ID_FPS + 2), Some(Command::Fps(15)));
        assert_eq!(
            command_from_id(ID_RESOLUTION + 1),
            Some(Command::Resolution(50))
        );
        assert_eq!(command_from_id(ID_COLOR), Some(Command::PickColor));
        assert_eq!(command_from_id(ID_SAVE), Some(Command::Save));
        assert_eq!(command_from_id(ID_EXIT), Some(Command::Exit));
    }

    #[test]
    fn unknown_ids_are_ignored() {
        assert_eq!(command_from_id(0), None);
        assert_eq!(command_from_id(999), None);
        assert_eq!(command_from_id(ID_COUNT + 11), None);
        assert_eq!(command_from_id(ID_RESOLUTION + 4), None);
        assert_eq!(command_from_id(1703), None);
        assert_eq!(command_from_id(u32::MAX), None);
    }

    #[test]
    fn every_block_fits_in_its_id_range() {
        for len in [
            COUNT_PRESETS.len(),
            SPEED_PRESETS.len(),
            SIZE_PRESETS.len(),
            EDGE_BLUR_PRESETS.len(),
            SWAY_PRESETS.len(),
            FPS_PRESETS.len(),
            RESOLUTION_PRESETS.len(),
        ] {
            assert!(len <= 100);
        }
    }
}

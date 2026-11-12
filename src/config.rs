//! Settings stored in `config.json` next to the executable.
//!
//! The file is a flat JSON object, compatible with the one written by the
//! former Python implementation. Every key is validated on its own, so one bad
//! value never discards the others; a missing or unreadable file yields the
//! defaults. Unknown keys are ignored.

use std::path::PathBuf;

use serde_json::{Map, Value};

/// Upper bound for `count`; the renderer sizes its instance buffer for it.
pub const MAX_COUNT: u32 = 20_000;

/// Which monitors get a snow window: all of them, or a list of indices in
/// `EnumDisplayMonitors` order.
#[derive(Clone, Debug, PartialEq)]
pub enum Monitors {
    All,
    List(Vec<usize>),
}

impl Monitors {
    /// Picks the configured entries of `all`. Indices that do not exist are
    /// skipped, and an empty result falls back to every monitor.
    pub fn select<T: Copy>(&self, all: &[T]) -> Vec<T> {
        match self {
            Monitors::All => all.to_vec(),
            Monitors::List(indices) => {
                let chosen: Vec<T> = indices
                    .iter()
                    .filter_map(|&i| all.get(i).copied())
                    .collect();
                if chosen.is_empty() {
                    all.to_vec()
                } else {
                    chosen
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Flakes per monitor.
    pub count: u32,
    /// Fall speed in pixels per 1/60 s (each flake varies by +/-30 %).
    pub speed: f32,
    /// Flake diameter in monitor pixels.
    pub size: u32,
    /// Softness of the flake edge in monitor pixels; 0 = hard edge.
    pub edge_blur: u32,
    /// Horizontal sway amplitude in monitor pixels.
    pub sway_amp: u32,
    /// Frames per second.
    pub fps: u32,
    /// Flake colour as RGB.
    pub color: [u8; 3],
    pub monitors: Monitors,
    /// Render resolution in percent of the monitor resolution.
    pub resolution: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            count: 200,
            speed: 0.8,
            size: 6,
            edge_blur: 2,
            sway_amp: 18,
            fps: 15,
            color: [255, 255, 255],
            monitors: Monitors::All,
            resolution: 50,
        }
    }
}

impl Config {
    /// Parses a config file; anything missing or invalid keeps its default.
    pub fn from_json(text: &str) -> Config {
        let mut cfg = Config::default();
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return cfg;
        };
        if let Some(v) = number(&map, "count") {
            cfg.count = clamp_u32(v, 0, MAX_COUNT);
        }
        if let Some(v) = number(&map, "speed") {
            cfg.speed = v.clamp(0.0, 100.0) as f32;
        }
        if let Some(v) = number(&map, "size") {
            cfg.size = clamp_u32(v, 1, 200);
        }
        if let Some(v) = number(&map, "edge_blur") {
            cfg.edge_blur = clamp_u32(v, 0, 100);
        }
        if let Some(v) = number(&map, "sway_amp") {
            cfg.sway_amp = clamp_u32(v, 0, 1000);
        }
        if let Some(v) = number(&map, "fps") {
            cfg.fps = clamp_u32(v, 1, 240);
        }
        if let Some(v) = number(&map, "resolution") {
            cfg.resolution = clamp_u32(v, 10, 100);
        }
        if let Some(c) = map
            .get("color")
            .and_then(Value::as_str)
            .and_then(parse_color)
        {
            cfg.color = c;
        }
        if let Some(Value::Array(items)) = map.get("monitors") {
            let list: Vec<usize> = items
                .iter()
                .filter_map(Value::as_u64)
                .map(|v| v as usize)
                .collect();
            if !list.is_empty() {
                cfg.monitors = Monitors::List(list);
            }
        }
        cfg
    }

    /// Serialises in the key order of the original file format.
    pub fn to_json(&self) -> String {
        let monitors = match &self.monitors {
            Monitors::All => "\"all\"".to_string(),
            Monitors::List(list) => {
                let items: Vec<String> = list.iter().map(|i| i.to_string()).collect();
                format!("[{}]", items.join(", "))
            }
        };
        format!(
            "{{\n  \"count\": {},\n  \"speed\": {},\n  \"size\": {},\n  \"edge_blur\": {},\n  \
             \"sway_amp\": {},\n  \"fps\": {},\n  \"color\": \"{}\",\n  \"monitors\": {},\n  \
             \"resolution\": {}\n}}\n",
            self.count,
            self.speed,
            self.size,
            self.edge_blur,
            self.sway_amp,
            self.fps,
            format_color(self.color),
            monitors,
            self.resolution
        )
    }

    /// Location of the config file: next to the executable.
    pub fn path() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        Some(exe.parent()?.join("config.json"))
    }

    pub fn load() -> Config {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Config::from_json(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or(std::io::ErrorKind::NotFound)?;
        std::fs::write(path, self.to_json())
    }
}

fn number(map: &Map<String, Value>, key: &str) -> Option<f64> {
    map.get(key)?.as_f64().filter(|v| v.is_finite())
}

fn clamp_u32(v: f64, lo: u32, hi: u32) -> u32 {
    v.round().clamp(lo as f64, hi as f64) as u32
}

/// Parses `#RRGGBB` (the leading `#` is optional).
pub fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

pub fn format_color(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_malformed_file_gives_defaults() {
        assert_eq!(Config::from_json(""), Config::default());
        assert_eq!(Config::from_json("{ not json"), Config::default());
        assert_eq!(Config::from_json("[1, 2, 3]"), Config::default());
    }

    #[test]
    fn reads_a_file_written_by_the_python_version() {
        let text = r##"{
          "count": 200,
          "speed": 0.3,
          "size": 18,
          "edge_blur": 2,
          "sway_amp": 10,
          "color": "#c4c0fa",
          "monitors": "all"
        }"##;
        let cfg = Config::from_json(text);
        assert_eq!(cfg.count, 200);
        assert_eq!(cfg.speed, 0.3);
        assert_eq!(cfg.size, 18);
        assert_eq!(cfg.sway_amp, 10);
        assert_eq!(cfg.color, [0xc4, 0xc0, 0xfa]);
        assert_eq!(cfg.monitors, Monitors::All);
        // Keys the old file does not have fall back to the new defaults.
        assert_eq!(cfg.fps, 15);
        assert_eq!(cfg.resolution, 50);
    }

    #[test]
    fn one_bad_value_does_not_discard_the_others() {
        let cfg = Config::from_json(r#"{"count": "many", "size": 12, "color": "red", "junk": 1}"#);
        assert_eq!(cfg.count, Config::default().count);
        assert_eq!(cfg.size, 12);
        assert_eq!(cfg.color, Config::default().color);
    }

    #[test]
    fn values_are_clamped_and_rounded() {
        let cfg = Config::from_json(
            r#"{"count": 9999999, "size": 0, "fps": 1000, "resolution": 1, "edge_blur": 2.6, "speed": -5}"#,
        );
        assert_eq!(cfg.count, MAX_COUNT);
        assert_eq!(cfg.size, 1);
        assert_eq!(cfg.fps, 240);
        assert_eq!(cfg.resolution, 10);
        assert_eq!(cfg.edge_blur, 3);
        assert_eq!(cfg.speed, 0.0);
    }

    #[test]
    fn monitor_list_is_kept_and_empty_list_means_all() {
        let cfg = Config::from_json(r#"{"monitors": [1, 0]}"#);
        assert_eq!(cfg.monitors, Monitors::List(vec![1, 0]));
        assert_eq!(
            Config::from_json(r#"{"monitors": []}"#).monitors,
            Monitors::All
        );
        assert_eq!(
            Config::from_json(r#"{"monitors": ["x"]}"#).monitors,
            Monitors::All
        );
    }

    #[test]
    fn selection_skips_missing_monitors_and_falls_back_to_all() {
        let all = ['a', 'b', 'c'];
        assert_eq!(Monitors::All.select(&all), vec!['a', 'b', 'c']);
        assert_eq!(Monitors::List(vec![2, 0]).select(&all), vec!['c', 'a']);
        assert_eq!(Monitors::List(vec![1, 9]).select(&all), vec!['b']);
        assert_eq!(Monitors::List(vec![7, 9]).select(&all), vec!['a', 'b', 'c']);
    }

    #[test]
    fn saved_text_reads_back_identically() {
        let cfg = Config {
            count: 400,
            speed: 1.5,
            size: 25,
            edge_blur: 0,
            sway_amp: 30,
            fps: 10,
            color: [1, 0xab, 0xff],
            monitors: Monitors::List(vec![0, 2]),
            resolution: 33,
        };
        let text = cfg.to_json();
        assert_eq!(Config::from_json(&text), cfg);
        assert!(text.contains("\"color\": \"#01abff\""));
        assert_eq!(
            Config::from_json(&Config::default().to_json()),
            Config::default()
        );
    }

    #[test]
    fn colors_parse_with_or_without_hash() {
        assert_eq!(parse_color("#FFFFFF"), Some([255, 255, 255]));
        assert_eq!(parse_color("c4c0fa"), Some([0xc4, 0xc0, 0xfa]));
        assert_eq!(parse_color("#fff"), None);
        assert_eq!(parse_color("#gggggg"), None);
    }
}

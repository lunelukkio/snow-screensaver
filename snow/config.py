from __future__ import annotations

import json
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path


DEFAULTS = {
    "count": 200,
    "speed": 0.8,
    "size": 6,
    "edge_blur": 2,
    "sway_amp": 18,
    "fps": 30,
    "color": "#FFFFFF",
    "monitors": "all",
}


@dataclass
class Config:
    count: int = 200
    speed: float = 0.8
    size: int = 6
    edge_blur: int = 2
    sway_amp: int = 18
    fps: int = 30
    color: str = "#FFFFFF"
    monitors: object = "all"  # "all" or list[int]


def config_path() -> Path:
    # When frozen by PyInstaller, place config next to the exe; otherwise next to main.py.
    if getattr(sys, "frozen", False):
        base = Path(sys.executable).parent
    else:
        base = Path(__file__).resolve().parent.parent
    return base / "config.json"


def load() -> Config:
    path = config_path()
    if not path.exists():
        return Config()
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return Config()
    merged = {**DEFAULTS, **{k: v for k, v in raw.items() if k in DEFAULTS}}
    return Config(**merged)


def save(cfg: Config) -> None:
    path = config_path()
    path.write_text(json.dumps(asdict(cfg), indent=2), encoding="utf-8")

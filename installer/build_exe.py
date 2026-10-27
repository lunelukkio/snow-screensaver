"""Build the standalone screensaver executable with PyInstaller.

Run with:  uv run python installer/build_exe.py

Outputs in dist/:
  snow.exe -- screensaver (right-click for settings)
"""
from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "installer" / "snow.ico"


def build_one(name: str, entry: Path) -> None:
    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--noconfirm",
        "--clean",
        "--onefile",
        "--noconsole",
        "--name", name,
        "--paths", str(ROOT),
        "--icon", str(ICON),
        str(entry),
    ]
    print(">>>", " ".join(cmd))
    subprocess.check_call(cmd, cwd=ROOT)


def main() -> None:
    if not ICON.exists():
        print(f"Icon missing at {ICON}; running make_icon.py first.")
        subprocess.check_call(
            [sys.executable, str(ROOT / "installer" / "make_icon.py")], cwd=ROOT
        )

    for stale in ("build", "dist"):
        p = ROOT / stale
        if p.exists():
            shutil.rmtree(p)

    build_one("snow", ROOT / "main.py")

    for spec in ROOT.glob("*.spec"):
        spec.unlink()

    print("\nDone. Executable is in:", ROOT / "dist")


if __name__ == "__main__":
    main()

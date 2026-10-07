"""Rebuild sandboxed browser previews from the checked-in design fragments."""

from html import escape
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "design" / "runtime"


def main() -> None:
    theme = json.loads((ROOT / "design" / "themes" / "wiffletree-tidal.json").read_text())
    palettes = {
        f"tidal-{appearance}": {
            "name": f"Tidal {appearance.title()}",
            "appearance": appearance,
            "palette": values,
        }
        for appearance, values in theme["variants"].items()
    }
    frame_template = (RUNTIME / "frame.html").read_text()
    shell_template = (RUNTIME / "shell.html").read_text()
    for source in sorted((ROOT / "design" / "mockups").rglob("source.html")):
        fragment = source.read_text()
        if source.parent.name == "current":
            fragment, count = re.subn(
                r"/\* WIFFLETREE_THEMES_START \*/.*?/\* WIFFLETREE_THEMES_END \*/",
                "/* WIFFLETREE_THEMES_START */"
                + json.dumps(palettes, separators=(",", ":"))
                + "/* WIFFLETREE_THEMES_END */",
                fragment,
                flags=re.DOTALL,
            )
            if count != 1:
                raise ValueError("Current study must contain exactly one marked theme block")
            source.write_text(fragment)
        frame = frame_template.replace("<!-- WIFFLETREE_MOCKUP_SOURCE -->", fragment)
        document = shell_template.replace("__WIFFLETREE_FRAME_DOCUMENT__", escape(frame))
        if source.parent.name == "current":
            document = document.replace("Celadon Chat", "Wiffletree · Tidal")
        destination = source.with_name("index.html")
        destination.write_text(document)
        print(destination.relative_to(ROOT))


if __name__ == "__main__":
    main()

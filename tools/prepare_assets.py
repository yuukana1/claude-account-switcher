"""Extract Anthropic Sans from the locally installed Claude desktop app and create
static weights for the switcher. The result stays in the user's local switcher folder;
nothing is bundled into the portable executable.

Run from the project root:
    PYTHONPATH=tools/vendor python tools/prepare_assets.py
"""

import json
import os
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(ROOT / "tools" / "vendor"))

from fontTools.ttLib import TTFont  # noqa: E402
from fontTools.varLib.instancer import instantiateVariableFont  # noqa: E402


def local_appdata() -> Path:
    return Path(os.environ.get("LOCALAPPDATA", str(Path.home() / "AppData" / "Local")))


def app_asar() -> Path | None:
    settings = local_appdata() / "ClaudeProfiles" / "settings.json"
    if settings.is_file():
        try:
            data = json.loads(settings.read_text(encoding="utf-8-sig"))
            exe = data.get("InstallCache", {}).get("Exe", "")
            if exe:
                candidate = Path(exe).parent / "resources" / "app.asar"
                if candidate.is_file():
                    return candidate
        except Exception:
            pass
    return None


def read_asar_entry(asar: Path, wanted_suffix: str) -> bytes | None:
    with asar.open("rb") as handle:
        head = handle.read(16)
        if len(head) < 16:
            return None
        _pickle, header_size, _padded, text_len = struct.unpack("<4I", head)
        handle.seek(16)
        header = json.loads(handle.read(text_len).decode("utf-8"))
        base = 8 + header_size

        def walk(node, path=""):
            for name, item in node.get("files", {}).items():
                current = path + "/" + name
                if "files" in item:
                    yield from walk(item, current)
                elif current.endswith(wanted_suffix):
                    yield current, item

        for current, item in walk(header):
            offset = int(item.get("offset", 0))
            size = int(item.get("size", 0))
            if size <= 0:
                continue
            handle.seek(base + offset)
            data = handle.read(size)
            if len(data) == size:
                print(f"  using {current} ({size} bytes)")
                return data
    return None


def convert(font_bytes: bytes, out_dir: Path, family: str, weights) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    woff2 = out_dir / f"{family}-Roman-Variable.woff2"
    woff2.write_bytes(font_bytes)

    base_path = out_dir / f"_{family}-base.ttf"
    font = TTFont(woff2)
    font.flavor = None
    font.save(base_path)

    base = TTFont(base_path)
    axes = {a.axisTag: a.defaultValue for a in base["fvar"].axes} if "fvar" in base else {}
    print(f"  {family} axes:", axes)
    for weight, name in weights:
        target = dict(axes)
        target["wght"] = weight
        instance = instantiateVariableFont(
            base,
            target,
            inplace=False,
            updateFontNames=False,
        )
        instance.flavor = None
        path = out_dir / f"{family}-{name}.ttf"
        instance.save(path)
        print(f"  wrote {path.name}")
    try:
        base_path.unlink()
    except OSError:
        pass


def main() -> int:
    asar = app_asar()
    if not asar:
        print("Claude app.asar not found; the UI will fall back to Segoe UI Variable.")
        return 1
    print("Claude app.asar:", asar)
    out = local_appdata() / "ClaudeProfiles" / ".switcher" / "fonts"

    sans = read_asar_entry(asar, "AnthropicSans-Roman-Variable.woff2")
    if sans:
        convert(sans, out, "AnthropicSans", [(500, "Medium"), (620, "SemiBold"), (700, "Bold")])
    else:
        print("AnthropicSans-Roman-Variable.woff2 not found inside app.asar.")

    serif = read_asar_entry(asar, "AnthropicSerif-Roman-Variable.woff2")
    if serif:
        convert(serif, out, "AnthropicSerif", [(500, "Medium")])
    else:
        print("AnthropicSerif-Roman-Variable.woff2 not found inside app.asar.")

    if not sans and not serif:
        return 1
    print("fonts ready in", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

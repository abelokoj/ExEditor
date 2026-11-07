"""GitHub packaging gate: optional font payloads must never be compiled into ExEditor."""
from pathlib import Path
import hashlib
import json
import sys

project = Path(__file__).resolve().parent.parent
binary = Path(sys.argv[1]).read_bytes()
manifest = json.loads((project / "assets/accessories/fonts.json").read_text(encoding="utf-8"))
for pack in manifest["packs"]:
    for asset in pack["fonts"]:
        data = (project / "assets/fonts" / pack["id"] / asset["file"]).read_bytes()
        if len(data) != asset["bytes"] or hashlib.sha256(data).hexdigest() != asset["sha256"]:
            raise SystemExit("Font accessory source does not match its download manifest: " + asset["file"])
        if data in binary:
            raise SystemExit("Optional font was embedded in the application: " + asset["file"])
print("Verified: Pretendard Std, Maple Mono NF and Noto Color Emoji are optional downloads, absent from the executable.")

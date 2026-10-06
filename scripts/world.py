"""Refreshes assets/nodes.json and assets/territories.json from a bdo-viewer extraction.

Usage: python scripts/world.py [path/to/world.json]
"""

import json
import os
import sys
from pathlib import Path

default = Path(os.environ["LOCALAPPDATA"]) / "bdo-viewer" / "data" / "world.json"
source = Path(sys.argv[1]) if len(sys.argv) > 1 else default
assets = Path(__file__).resolve().parent.parent / "assets"

world = json.loads(source.read_text(encoding="utf-8"))
for key in ("nodes", "territories"):
    target = assets / f"{key}.json"
    target.write_text(
        json.dumps(world[key], ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
        newline="\n",
    )
    print(f"{len(world[key])} {key} -> {target}")

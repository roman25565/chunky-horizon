#!/usr/bin/env python3
"""Packs the chunky mod folder into F:/MinecraftTools/Chunky/dist/chunky-<version>.hnmod
(docs/api/mod-files.md), the same layout as AuthMe's tools/pack-hnmod.py. The package goes
outside BuildTestMods so update.bat's mod folder holds only mod sources.
Run: python tools/pack-hnmod.py"""
import json, os, re, zipfile

bundle = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
manifest = open(os.path.join(bundle, "horizon-mixin.toml"), encoding="utf-8").read()
mod_id = re.search(r'^id = "([^"]+)"', manifest, re.M).group(1)
version = re.search(r'^version = "([^"]+)"', manifest, re.M).group(1)
horizon = re.search(r'^horizon_revision = "([0-9a-f]{40})"', manifest, re.M).group(1)
# The horizon-mixins commit that Horizon fdc87ae pins (native/built.json).
MIXINS = "5a64af938ec86b00d53d16a408aabc0038fb3e57"
dist = os.path.join(os.path.dirname(os.path.dirname(bundle)), "Chunky", "dist")
os.makedirs(dist, exist_ok=True)
out = os.path.join(dist, f"{mod_id}-{version}.hnmod")
native = {"schema": 1, "id": mod_id, "version": version, "game": "26.3",
          "horizon": horizon, "mixins": MIXINS}
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("horizon-native.json", json.dumps(native, indent=2) + "\n")
    for dirpath, dirs, files in os.walk(bundle):
        dirs[:] = sorted(d for d in dirs if d not in ("target", "dist", "tools"))
        for f in sorted(files):
            if not (f.endswith((".rs", ".toml")) or f.startswith(("README", "LICENSE"))):
                continue
            full = os.path.join(dirpath, f)
            rel = os.path.relpath(full, bundle).replace(os.sep, "/")
            z.write(full, f"mixins/{mod_id}/{rel}")
print(out)

#!/usr/bin/env python3
"""Export the public Ubuntu source without build outputs or private state."""
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parent.parent
output = root / "outputs"
output.mkdir(exist_ok=True)
files = [root / name for name in ["Cargo.toml", "Cargo.lock", "README.md", "README.zh-CN.md", "VALIDATION.md", ".gitignore"]]
for name in ["src", "assets", "tests", "docs", "build"]:
    files += [p for p in (root / name).rglob("*") if p.is_file() and "__pycache__" not in p.parts]
with zipfile.ZipFile(output / "Encrypt-assets-Ubuntu-0.1.0-source.zip", "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(files):
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise RuntimeError("Source path escaped project")
        if path.suffix.lower() in {".lic", ".issuerkey", ".olmeta", ".key", ".bin", ".pyc"}:
            raise RuntimeError("Private or built file in source")
        archive.write(path, "Encrypt-assets-Ubuntu-0.1.0-source/" + path.relative_to(root).as_posix())
print("Public source exported")

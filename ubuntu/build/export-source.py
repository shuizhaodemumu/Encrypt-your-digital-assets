#!/usr/bin/env python3
"""Export reviewed source/docs only, excluding local transports and private state."""
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parent.parent
output = root / "outputs"
output.mkdir(exist_ok=True)
files = [root / p for p in ["Cargo.toml", "Cargo.lock", "README.md", "VALIDATION.md", "AGENTS.md", ".gitignore", "build/bundle.py", "build/build-ubuntu.sh", "build/export-source.py", "build/verify-installed.py"]]
for directory in ["src", "assets", "tests", "docs"]:
    files += [p for p in (root / directory).rglob("*") if p.is_file() and "__pycache__" not in p.parts]
archive = output / "Encrypt-assets-Ubuntu-0.1.2-source.zip"
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
    for path in sorted(set(files)):
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise RuntimeError("Source export must stay in project")
        if path.suffix.lower() in {".issuerkey", ".lic", ".olmeta", ".key", ".bin", ".pyc"}:
            raise RuntimeError("Private/build file in export")
        z.write(path, "Encrypt-assets-Ubuntu-0.1.2-source/" + path.relative_to(root).as_posix())
print("SOURCE_EXPORT_PASS", len(files), archive.stat().st_size)

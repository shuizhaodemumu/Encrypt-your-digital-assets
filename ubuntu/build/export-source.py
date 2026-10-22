#!/usr/bin/env python3
"""Export reviewed source/docs only, excluding local transports and private state."""
import hashlib
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parent.parent
output = root / "outputs"
output.mkdir(exist_ok=True)
files = [root / p for p in ["Cargo.toml", "Cargo.lock", "README.md", "VALIDATION.md", "AGENTS.md", ".gitignore", "build/bundle.py", "build/build-ubuntu.sh", "build/export-source.py", "build/verify-installed.py"]]
for directory in ["src", "assets", "tests", "docs"]:
    files += [p for p in (root / directory).rglob("*") if p.is_file() and "__pycache__" not in p.parts]
archive = output / "Encrypt-assets-Ubuntu-0.1.3-source.zip"
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
    for path in sorted(set(files)):
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise RuntimeError("Source export must stay in project")
        if path.suffix.lower() in {".issuerkey", ".lic", ".olmeta", ".key", ".bin", ".pyc"}:
            raise RuntimeError("Private/build file in export")
        z.write(path, "Encrypt-assets-Ubuntu-0.1.3-source/" + path.relative_to(root).as_posix())
print("SOURCE_EXPORT_PASS", len(files), archive.stat().st_size)
manifest = output / "SHA256SUMS.txt"
with archive.open("rb") as source:
    source_hash = hashlib.file_digest(source, "sha256").hexdigest()
lines = manifest.read_text(encoding="utf-8").splitlines() if manifest.exists() else []
lines = [line for line in lines if line.rsplit("  ", 1)[-1] != archive.name]
lines.append(f"{source_hash}  {archive.name}")
manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")

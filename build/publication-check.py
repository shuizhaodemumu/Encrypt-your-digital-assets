"""Read-only checks on the Git publication index, docs and packaged source."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import tomllib

root = Path(__file__).resolve().parents[1]
paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
paths = [p for p in paths if p]
blocked = {".issuerkey", ".lic", ".olmeta", ".josn", ".key", ".pem", ".pfx"}
for name in paths:
    p = Path(name)
    assert p.suffix.lower() not in blocked, name
    assert p.parts[0] not in {"work", "target", "toolchain", "程序"}, name
    assert not p.name.startswith(".env"), name
    if p.suffix in {".rs", ".py", ".ps1", ".md", ".json", ".toml", ".yml"}:
        text = (root / p).read_text(encoding="utf-8-sig")
        assert not re.search(r"ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}", text), name
        assert ("-----BEGIN " + "PRIVATE KEY-----") not in text, name
assert tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"] == "2.4.3"
for name in ["README.md", "README.zh-CN.md"]:
    text = (root / name).read_text(encoding="utf-8")
    assert text.startswith("# Encrypt your digital assets\n"), name
    for target in re.findall(r"\]\(([^)]+)\)", text):
        if not target.startswith(("http", "#", "mailto:")):
            assert (root / target).exists(), (name, target)
    assert "2.4.3" in text and "watchdog-test-mode" in text
    assert "60" in text and "PowerPoint" in text and "LibVLC" in text
provenance = json.loads((root / "docs/artwork-provenance.json").read_text())
digest = hashlib.sha256((root / provenance["asset"]).read_bytes()).hexdigest()
assert digest.lower() == provenance["source"]["sha256"].lower()
print(f"PUBLIC_INDEX_NO_PRIVATE_ARTIFACTS_PASS: {len(paths)} files")
print("BILINGUAL_README_LINKS_VERSION_BEHAVIOR_PASS")
print("ARTWORK_ORIGINAL_PROVENANCE_HASH_PASS")

#!/usr/bin/python3
"""Read-only installation checks. Never opens licensed content or reboots."""
import hashlib
import json
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent.parent
installed = Path("/opt/encrypt-assets-ubuntu/EncryptAssetsUbuntu.run")
release = root / "outputs/Encrypt-assets-Ubuntu-0.1.2-release-x86_64.run"
deb = root / "outputs/encrypt-assets-ubuntu_0.1.2_amd64.deb"

def digest(path):
    with path.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()

status = subprocess.check_output(["dpkg-query", "-W", "-f=${Status} ${Version}", "encrypt-assets-ubuntu"], text=True)
assert status == "install ok installed 0.1.2", status
assert digest(installed) == digest(release)
subprocess.run(["desktop-file-validate", "/usr/share/applications/io.github.shuizhaodemumu.EncryptAssetsUbuntu.desktop"], check=True)
listing = subprocess.check_output(["dpkg-deb", "-c", str(deb)], text=True)
assert "/autostart/" not in listing and "/systemd/" not in listing
version = subprocess.check_output([str(installed), "--version"], text=True).strip()
assert version == "Encrypt your digital assets Ubuntu 0.1.2"
probe = subprocess.run([str(installed), "--self-check"], capture_output=True, text=True)
assert probe.returncode == 1 and "requires a watchdog-test-mode build" in probe.stderr, "Production package unexpectedly enables test feature"
results = {"status": status, "version": version, "installed_matches_release": True, "production_test_feature_disabled": True, "no_startup_entries_in_package": True, "release_sha256": digest(release), "deb_sha256": digest(deb)}
(root / "work/installed-validation.json").write_text(json.dumps(results, indent=2) + "\n")
print("DEB_INSTALLED_LAUNCHER_HASH_VERSION_PRODUCTION_FLAGS_PASS")

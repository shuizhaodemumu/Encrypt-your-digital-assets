#!/usr/bin/python3
"""Execute the reviewed CLI and native decoder paths under Xvfb.
Never uses a production watchdog, never invokes reboot, never prints hardware.
"""
import argparse
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("--group", choices=["media", "documents", "all"], default="all")
    args = parser.parse_args()
    binary = args.binary.resolve()
    fixture = ROOT / "work/fixtures"
    if not (fixture / "sample.pptx").is_file():
        subprocess.run(["/usr/bin/python3", str(ROOT / "tests/make-fixtures.py"), str(fixture)], check=True)
    with tempfile.TemporaryDirectory(prefix="integration-", dir=ROOT / "work") as temp:
        work = Path(temp)
        env = dict(os.environ, ENCRYPT_ASSETS_DATA_DIR=str(work / "state"), GDK_BACKEND="x11", GSK_RENDERER="cairo")
        secret = "synthetic-test-password\n"
        def run(*params, password=False, timeout=90):
            p = subprocess.run([str(binary), *map(str, params)], input=secret if password else "", text=True, capture_output=True, env=env, timeout=timeout)
            if p.returncode != 0:
                raise AssertionError("Command failed: " + str(params[0]) + "\n" + p.stderr[-4000:])
            return p.stdout
        run("--create-issuer", work / "test.issuerkey", password=True)
        exts = []
        if args.group in ["media", "all"]:
            exts += ["mp4", "mkv", "avi", "mov", "webm", "wav", "mp3", "m4a", "flac", "ogg"]
        if args.group in ["documents", "all"]:
            exts += ["txt", "png", "pdf", "docx", "pptx", "ppt", "odp"]
        for ext in exts:
            package = work / (ext + ".bin")
            request = work / (ext + ".json")
            license = work / (ext + ".lic")
            run("--pack", fixture / ("sample." + ext), work / "test.issuerkey", package, password=True)
            assert os.access(package, os.X_OK), "package is not executable"
            run("--enroll", package, request)
            envelope = request.read_text()
            assert '"Ciphertext"' in envelope and '"MACAddress"' not in envelope
            run("--issue", work / "test.issuerkey", package, request, license, "1", "600", password=True)
            run("--activate", package, license)
            result = run("--verify-content", package, "8")
            print(ext.upper() + "_LICENSED_NATIVE_VIEW_PASS " + result.strip(), flush=True)
            if ext == "mp4":
                invalid = json.loads(license.read_text())
                invalid["SignatureBase64"] = "AAAA"
                bad = work / "invalid.lic"
                bad.write_text(json.dumps(invalid))
                p = subprocess.run([str(binary), "--activate", str(package), str(bad)], capture_output=True, text=True, env=env)
                assert p.returncode != 0, "forged license accepted"
                child_env = dict(env, ENCRYPT_ASSETS_TEST_CLOSE_SECONDS="7")
                p = subprocess.run([str(package)], capture_output=True, text=True, env=child_env, timeout=30)
                assert p.returncode == 0, p.stderr
                assert "Playback fault restart" not in p.stderr and "PLAYBACK_FAULT" not in p.stderr
                print("NORMAL_CLOSE_AND_VIDEO_EOF_NO_PROCESS_RESTART_PASS", flush=True)
    print("INTEGRATION_GROUP_PASS " + args.group)

if __name__ == "__main__":
    main()

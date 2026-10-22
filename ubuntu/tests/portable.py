#!/usr/bin/python3
"""Verify bundled runtime with host Office/Python/VLC components hidden."""
import argparse
import hashlib
import os
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def autostart_snapshot():
    base = Path.home() / ".config/autostart"
    return {str(p.relative_to(base)): hashlib.sha256(p.read_bytes()).hexdigest() for p in base.rglob("*") if p.is_file()} if base.exists() else {}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("installer", type=Path)
    args = parser.parse_args()
    installer = args.installer.resolve()
    if not (ROOT / "work/fixtures/sample.pptx").is_file():
        subprocess.run(["/usr/bin/python3", str(ROOT / "tests/make-fixtures.py"), str(ROOT / "work/fixtures")], check=True)
    original = autostart_snapshot()
    with tempfile.TemporaryDirectory(prefix="portable 中文路径 ", dir=ROOT / "work") as t:
        work = Path(t)
        (work / "runtime").mkdir(mode=0o700)
        env = dict(os.environ, XDG_DATA_HOME=str(work / "data"), XDG_CACHE_HOME=str(work / "cache"), XDG_RUNTIME_DIR=str(work / "runtime"), ENCRYPT_ASSETS_DATA_DIR=str(work / "state"), GDK_BACKEND="x11", ENCRYPT_ASSETS_TEST_CLOSE_SECONDS="6", GIO_USE_VFS="local", GTK_A11Y="none", GTK_USE_PORTAL="0")
        sandbox = ["bwrap", "--ro-bind", "/", "/", "--bind", str(work), str(work), "--bind", "/tmp", "/tmp", "--dev-bind", "/dev", "/dev", "--proc", "/proc", "--unshare-net"]
        for directory in ["/usr/lib/libreoffice", "/usr/share/libreoffice", "/usr/lib/python3.14", "/usr/lib/x86_64-linux-gnu/vlc", "/usr/share/fonts"]:
            if Path(directory).is_dir():
                sandbox += ["--tmpfs", directory]
        for soname in ["libvlc.so.5", "libvlccore.so.9", "libpoppler-glib.so.8", "libgtk-4.so.1"]:
            real = (Path("/usr/lib/x86_64-linux-gnu") / soname).resolve()
            if real.exists():
                sandbox += ["--ro-bind", "/dev/null", str(real)]
        password = "portable-synthetic-password\n"
        def run(program, *params, secret=False, timeout=120):
            p = subprocess.run(sandbox + [str(program), *map(str, params)], env=env, input=password if secret else "", capture_output=True, text=True, timeout=timeout)
            if p.returncode:
                raise AssertionError(str(params[:1]) + "\n" + p.stderr[-6000:])
            return p
        with ThreadPoolExecutor(max_workers=2) as pool:
            for result in pool.map(lambda _: run(installer, "--version"), range(2)):
                assert "Ubuntu 0.1.3" in result.stdout
        assert not list((work / "data/encrypt-assets-runtime").glob(".install-*"))
        print("CONCURRENT_FIRST_INSTALL_ATOMIC_RUNTIME_PASS", flush=True)
        runtime = next((work / "data/encrypt-assets-runtime").glob("*/launch")).parent
        fontenv = dict(env, FONTCONFIG_FILE=str(runtime / "share/fontconfig/fonts.conf"))
        font = subprocess.check_output(sandbox + ["/usr/bin/fc-match", ":lang=zh-cn", "-f", "%{file}"], env=fontenv, text=True)
        assert font.startswith(str(runtime / "share/fonts")), "Bundled Chinese font was not selected"
        print("BUNDLED_CHINESE_FONT_WITH_HOST_FONTS_HIDDEN_PASS", flush=True)
        run(installer, "--self-check")
        print("BUNDLED_LOADER_AND_CRYPTO_WITH_HOST_COMPONENTS_HIDDEN_PASS", flush=True)
        key = work / "test.issuerkey"
        run(installer, "--create-issuer", key, secret=True)
        for ext in ["mp4", "m4a", "pdf", "docx", "pptx"]:
            app = work / (ext + ".run")
            request = work / (ext + ".json")
            license = work / (ext + ".lic")
            run(installer, "--pack", ROOT / "work/fixtures" / ("sample." + ext), key, app, secret=True)
            run(app, "--enroll", app, request)
            run(installer, "--issue", key, app, request, license, "1", "600", secret=True)
            run(app, "--activate", app, license)
            result = run(app, "--verify-content", app, "8")
            print("BUNDLED_" + ext.upper() + "_OFFLINE_PASS " + result.stdout.strip(), flush=True)
            if ext == "mp4":
                result = run(app)
                assert "PLAYBACK_FAULT" not in result.stderr and "Playback fault restart" not in result.stderr
                print("BUNDLED_CONTENT_SELF_LAUNCH_NORMAL_CLOSE_PASS", flush=True)
            app.unlink()
        # Capture this while Xvfb is still alive. Session D-Bus portal services
        # may log an XIO error when the test display is subsequently torn down;
        # it must not be mistaken for a surviving player/Office subprocess.
        remaining = []
        for process in Path("/proc").glob("[0-9]*"):
            try:
                args = (process / "cmdline").read_bytes().replace(b"\0", b" ")
                if str(work).encode() in args and int(process.name) != os.getpid():
                    remaining.append(process.name)
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                pass
        assert not remaining, "Application children remain after close: " + str(remaining)
        print("NO_APPLICATION_OR_OFFICE_CHILD_PROCESSES_LEFT_PASS", flush=True)
        assert autostart_snapshot() == original
        assert not (work / "data/autostart").exists()
        print("NO_AUTOSTART_MUTATION_PASS", flush=True)
    print("OFFLINE_BUNDLE_INTEGRATION_PASS")

if __name__ == "__main__":
    main()

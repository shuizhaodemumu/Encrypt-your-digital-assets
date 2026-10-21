#!/usr/bin/python3
"""Exercise the real GTK video control dock with synthetic media only."""
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def wait_for(fn, seconds=15):
    stop = time.monotonic() + seconds
    last = None
    while time.monotonic() < stop:
        try:
            value = fn()
            if value:
                return value
        except Exception as exc:
            last = exc
        time.sleep(0.1)
    raise AssertionError("Timed out waiting for video controls: " + str(last))


def main():
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="control-bar-") as temp:
        work = Path(temp)
        env = dict(
            os.environ,
            ENCRYPT_ASSETS_DATA_DIR=str(work / "state"),
            ENCRYPT_ASSETS_TEST_CONTROL_STATE_FILE=str(work / "control-state.txt"),
            GTK_USE_PORTAL="0",
            GDK_BACKEND="x11",
            GSK_RENDERER="cairo",
        )
        video = work / "sample.mp4"
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
             "testsrc2=size=320x180:rate=12", "-t", "12", "-threads", "1", str(video)],
            check=True,
        )
        issuer = work / "test.issuerkey"
        package = work / "sample.exe"
        request = work / "request.json"
        license_file = work / "license.lic"

        def run(*args, password=False):
            result = subprocess.run(
                [str(binary), *map(str, args)],
                input="synthetic-test-password\n" if password else None,
                text=True,
                capture_output=True,
                env=env,
                timeout=90,
            )
            if result.returncode:
                raise AssertionError(result.stderr[-3000:])

        run("--create-issuer", issuer, password=True)
        run("--pack", video, issuer, package, password=True)
        run("--enroll", package, request)
        run("--issue", issuer, package, request, license_file, "1", "600", password=True)
        run("--activate", package, license_file)

        player = subprocess.Popen(
            [str(package)],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        try:
            time.sleep(1)
            subprocess.run(["xdotool", "mousemove", "640", "450"], check=True)

            def state():
                try:
                    return (work / "control-state.txt").read_text().strip()
                except OSError:
                    return ""

            wait_for(lambda: state() == "visible")
            wait_for(lambda: state() == "hidden", seconds=8)
            print("CONTROL_BAR_IDLE_3S_SLIDES_TO_BOTTOM_PASS", flush=True)

            subprocess.run(["xdotool", "mousemove_relative", "15", "15"], check=True)
            wait_for(lambda: state() == "visible", seconds=3)
            print("CONTROL_BAR_MOUSE_MOTION_SLIDES_OUT_PASS", flush=True)

        finally:
            if player.poll() is None:
                os.killpg(player.pid, signal.SIGTERM)
                player.wait(timeout=5)


if __name__ == "__main__":
    main()

#!/usr/bin/python3
"""Private UNO slideshow host. No editable document window is shown."""
import argparse
import os
import subprocess
import sys
import time
import uuid
import signal
from pathlib import Path
import uno
from com.sun.star.beans import PropertyValue
from com.sun.star.document.MacroExecMode import NEVER_EXECUTE

def prop(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p

def main():
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(SystemExit(0)))
    parser = argparse.ArgumentParser()
    parser.add_argument("directory")
    parser.add_argument("source")
    parser.add_argument("--probe", action="store_true")
    args = parser.parse_args()
    directory = Path(args.directory).resolve()
    source = Path(args.source).resolve()
    if not source.is_relative_to(directory):
        raise RuntimeError("Source must be in the private session directory")
    pipe = "encrypt_assets_" + uuid.uuid4().hex
    profile = uno.systemPathToFileUrl(str(directory / "office-profile"))
    error_file = open(directory / "office-errors.log", "wb")
    process = subprocess.Popen([
        "libreoffice", "-env:UserInstallation=" + profile,
        "--accept=pipe,name=" + pipe + ";urp;StarOffice.ServiceManager",
        "--nologo", "--norestore", "--nodefault", "--nofirststartwizard",
    ], stdout=subprocess.DEVNULL, stderr=error_file,
       env=dict(os.environ, SAL_USE_VCLPLUGIN="gen", SAL_DISABLESKIA="1", SAL_DISABLE_OPENCL="1"))
    doc = desktop = None
    try:
        ctx = uno.getComponentContext()
        resolver = ctx.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", ctx)
        remote = None
        for _ in range(300):
            if process.poll() is not None:
                raise RuntimeError("LibreOffice exited before UNO became ready")
            try:
                remote = resolver.resolve("uno:pipe,name=" + pipe + ";urp;StarOffice.ComponentContext")
                break
            except Exception:
                time.sleep(0.1)
        if remote is None:
            raise RuntimeError("LibreOffice UNO connection timed out")
        desktop = remote.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", remote)
        doc = desktop.loadComponentFromURL(uno.systemPathToFileUrl(str(source)), "_blank", 0, (
            prop("Hidden", False), prop("Minimized", True), prop("ReadOnly", True), prop("MacroExecutionMode", NEVER_EXECUTE),
            prop("UpdateDocMode", 0), prop("Silent", True),
        ))
        if doc is None or not doc.supportsService("com.sun.star.presentation.PresentationDocument"):
            raise RuntimeError("This file is not a supported presentation")
        # Impress needs a real view controller. Loading Hidden=True crashes
        # LibreOffice 26.2 when starting a full-screen presentation. Create a
        # minimized read-only frame, hide it, then start the separate slideshow.
        editor = doc.getCurrentController().getFrame().getContainerWindow()
        editor.setVisible(False)
        if editor.isVisible():
            raise RuntimeError("The editor window could not be hidden")
        # Use the named UNO constant, not a guessed numeric macro policy.
        show = doc.getPresentation()
        show.IsFullScreen = True
        show.IsEndless = True
        show.Pause = 0
        show.AllowAnimations = True
        show.IsAlwaysOnTop = True
        if not doc.isReadonly():
            raise RuntimeError("Presentation must be read-only")
        try:
            if not show.isRunning():
                show.start()
        except Exception:
            time.sleep(0.3)
            error_file.flush()
            errors = (directory / "office-errors.log").read_text(errors="replace")[-8192:]
            print("LibreOffice host exit: " + str(process.poll()) + " " + errors, file=sys.stderr)
            raise
        for _ in range(100):
            if show.isRunning():
                break
            time.sleep(0.1)
        if not show.isRunning():
            raise RuntimeError("The slideshow did not start")
        (directory / "ready").write_text("slideshow-only\n", encoding="utf-8")
        start = time.monotonic()
        while show.isRunning():
            if editor.isVisible():
                editor.setVisible(False)
            if editor.isVisible():
                raise RuntimeError("Only the slideshow may be visible")
            (directory / "heartbeat").write_text(str(time.monotonic()), encoding="ascii")
            if args.probe and time.monotonic() - start > 4:
                controller = show.getController()
                if controller is None:
                    raise RuntimeError("No slideshow controller")
                controller.gotoNextSlide()
                print("IMPRESS_SLIDESHOW_READY_READONLY_PASS", flush=True)
                break
            time.sleep(0.5)
        show.end()
    finally:
        if doc is not None:
            try:
                doc.close(True)
            except Exception:
                pass
        if desktop is not None:
            try:
                desktop.terminate()
            except Exception:
                pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        error_file.close()

if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        import traceback
        traceback.print_exc()
        print("Impress: " + type(e).__name__ + ": " + str(e), file=sys.stderr)
        sys.exit(1)

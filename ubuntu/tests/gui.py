#!/usr/bin/python3
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path
import pyatspi

ROOT = Path(__file__).resolve().parent.parent

def nodes(node):
    yield node
    for child in node:
        if child:
            yield from nodes(child)

def wait_for(fn, seconds=15):
    stop = time.monotonic() + seconds
    last = None
    while time.monotonic() < stop:
        try:
            result = fn()
            if result:
                return result
        except Exception as e:
            last = e
        time.sleep(0.15)
    raise AssertionError("UI wait timed out: " + str(last))

def main():
    binary = Path(sys.argv[1]).resolve()
    (ROOT / "work").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="gui-", dir=ROOT / "work") as temp:
        env = dict(os.environ, ENCRYPT_ASSETS_DATA_DIR=str(Path(temp) / "state"), GDK_BACKEND="x11", GSK_RENDERER="cairo", GTK_A11Y="atspi", GTK_USE_PORTAL="0")
        log = open(ROOT / "work/gui-runtime.log", "w")
        child = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        try:
            def application():
                return next(a for a in pyatspi.Registry.getDesktop(0) if a.name in ["encrypt-assets-ubuntu", "Encrypt your digital assets", binary.name])
            app = wait_for(application)
            def find(name, role=None):
                return next(n for n in nodes(app) if n.name == name and (role is None or n.getRoleName() == role or (role == "push button" and n.getRoleName() == "button")))
            def click(name):
                n = wait_for(lambda: find(name, "push button"))
                assert n.queryAction().doAction(0)
                time.sleep(0.3)
            def entry(name):
                for parent in (n for n in nodes(app) if n.name == name):
                    for n in nodes(parent):
                        try:
                            n.queryEditableText()
                            return n
                        except Exception:
                            pass
                raise AssertionError("No editable field " + name)
            wait_for(lambda: find("生成加密内容程序"))
            click("生成加密内容程序")
            wait_for(lambda: find("请先加载密钥并选择原文件"))
            # GTK standard captions follow the desktop locale (for example "好").
            subprocess.run(["xdotool", "key", "Return"], check=True)
            time.sleep(0.3)
            print("GUI_PREREQUISITE_ERROR_RECOVERY_PASS", flush=True)
            field = entry("首次导入窗口")
            field.queryEditableText().setTextContents("0")
            time.sleep(0.2)
            issue = find("确认并签发 LIC")
            assert not issue.getState().contains(pyatspi.STATE_SENSITIVE)
            field.queryEditableText().setTextContents("120")
            time.sleep(0.2)
            assert issue.getState().contains(pyatspi.STATE_SENSITIVE)
            click("中文 / English")
            wait_for(lambda: find("Build encrypted content app"))
            assert entry("First import window").queryText().getText(0, -1) == "120"
            click("中文 / English")
            wait_for(lambda: find("生成加密内容程序"))
            assert entry("首次导入窗口").queryText().getText(0, -1) == "120"
            print("GUI_WINDOW_VALIDATION_LANGUAGE_DRAFT_PRESERVED_PASS", flush=True)
            click("选择视频 / 音频 / 文档 / 图片 / PPT")
            # The native chooser may be in the application or the portal process.
            def cancel_node():
                for application in pyatspi.Registry.getDesktop(0):
                    for n in nodes(application):
                        if n.name.lstrip("_").startswith(("取消", "Cancel")) and n.getRoleName() in ["push button", "button"]:
                            return n
            cancel = wait_for(cancel_node)
            cancel.queryAction().doAction(0)
            time.sleep(0.4)
            print("GUI_NATIVE_CHOOSER_CANCEL_PASS", flush=True)
            subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "x11grab", "-video_size", "1280x900", "-i", os.environ["DISPLAY"], "-frames:v", "1", str(ROOT / "work/ubuntu-workspace.png")], check=True)
            pixels = subprocess.check_output(["ffmpeg", "-v", "error", "-i", str(ROOT / "work/ubuntu-workspace.png"), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            assert sum(pixels) / len(pixels) > 20, "Screenshot is blank or the app used another display"
            print("GUI_SCREENSHOT_CAPTURED", flush=True)
        finally:
            if child.poll() is None:
                subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "x11grab", "-video_size", "1280x900", "-i", os.environ["DISPLAY"], "-frames:v", "1", str(ROOT / "work/ubuntu-last-ui.png")], timeout=10)
            child.terminate()
            child.wait(timeout=10)
            log.close()

if __name__ == "__main__":
    main()

#!/usr/bin/python3
"""Synthetic samples only; no user media or issuer data."""
import os
import subprocess
import sys
import time
import uuid
from pathlib import Path
import uno
from com.sun.star.beans import PropertyValue

def prop(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p

def main():
    root = Path(sys.argv[1]).resolve()
    root.mkdir(parents=True, exist_ok=True)
    (root / "sample.txt").write_text("离线授权测试\nUbuntu encrypted document sample\n", encoding="utf-8")
    for ext, codec in [("mp4", "mpeg4"), ("mkv", "mpeg4"), ("avi", "mpeg4"), ("mov", "mpeg4"), ("webm", "libvpx")]:
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=12", "-t", "2", "-c:v", codec, "-threads", "1", str(root / ("sample." + ext))], check=True)
    for ext in ["wav", "mp3", "m4a", "flac", "ogg"]:
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", str(root / ("sample." + ext))], check=True)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=c=0xa98bc5:s=480x320", "-frames:v", "1", "-threads", "1", str(root / "sample.png")], check=True)
    pipe = "ea_fixture_" + uuid.uuid4().hex
    profile = root / "fixture-profile"
    process = subprocess.Popen(["libreoffice", "-env:UserInstallation=" + profile.as_uri(), "--headless", "--nologo", "--norestore", "--nodefault", "--accept=pipe,name=" + pipe + ";urp;StarOffice.ServiceManager"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    desktop = None
    try:
        local = uno.getComponentContext()
        resolver = local.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local)
        for _ in range(300):
            try:
                ctx = resolver.resolve("uno:pipe,name=" + pipe + ";urp;StarOffice.ComponentContext")
                break
            except Exception:
                time.sleep(0.1)
        else:
            raise RuntimeError("fixture UNO timeout")
        desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
        text = desktop.loadComponentFromURL("private:factory/swriter", "_blank", 0, (prop("Hidden", True),))
        text.Text.String = "Ubuntu encrypted document test\nSynthetic content only."
        text.storeToURL((root / "sample.docx").as_uri(), (prop("FilterName", "Office Open XML Text"), prop("Overwrite", True)))
        text.storeToURL((root / "sample.pdf").as_uri(), (prop("FilterName", "writer_pdf_Export"), prop("Overwrite", True)))
        text.close(True)
        show = desktop.loadComponentFromURL("private:factory/simpress", "_blank", 0, (prop("Hidden", True),))
        pages = show.getDrawPages()
        pages.insertNewByIndex(1)
        for i in range(2):
            page = pages.getByIndex(i)
            shape = show.createInstance("com.sun.star.drawing.TextShape")
            point = uno.createUnoStruct("com.sun.star.awt.Point")
            point.X, point.Y = 1800, 1600
            size = uno.createUnoStruct("com.sun.star.awt.Size")
            size.Width, size.Height = 22000, 7000
            shape.setPosition(point)
            shape.setSize(size)
            page.add(shape)
            shape.String = "Encrypted presentation slide " + str(i + 1)
            shape.CharHeight = 32.0
            page.Change = 1
            page.Duration = 2
        for ext, filtername in [("pptx", "Impress MS PowerPoint 2007 XML"), ("ppt", "MS PowerPoint 97"), ("odp", "impress8")]:
            show.storeToURL((root / ("sample." + ext)).as_uri(), (prop("FilterName", filtername), prop("Overwrite", True)))
        show.close(True)
    finally:
        if desktop is not None:
            try:
                desktop.terminate()
            except Exception:
                pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    print("SYNTHETIC_VIDEO_AUDIO_IMAGE_DOCUMENT_PRESENTATION_FIXTURES_READY")

if __name__ == "__main__":
    main()

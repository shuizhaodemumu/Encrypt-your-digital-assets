#!/usr/bin/python3
"""Check actual DEB/RUN icons and an isolated production administrator launch."""
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parent.parent
APP="io.github.shuizhaodemumu.EncryptAssetsUbuntu"
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def validate_entry(path):
    result=subprocess.run(["desktop-file-validate",str(path)],capture_output=True,text=True)
    assert result.returncode==0 and not (result.stdout+result.stderr).strip(),result.stdout+result.stderr
    assert "Categories=Utility;\n" in path.read_text()

with tempfile.TemporaryDirectory(prefix="icon-check-", dir=ROOT/"work") as tmp:
    temp=Path(tmp)
    deb=ROOT/"outputs/encrypt-assets-ubuntu_0.1.1_amd64.deb"
    run=ROOT/"outputs/Encrypt-assets-Ubuntu-0.1.1-release-x86_64.run"
    subprocess.run(["dpkg-deb","-x",str(deb),str(temp/"deb")],check=True)
    entry=temp/f"deb/usr/share/applications/{APP}.desktop"
    validate_entry(entry)
    assert f"Icon={APP}\n" in entry.read_text()
    assert sha(temp/"deb/opt/encrypt-assets-ubuntu/EncryptAssetsUbuntu.run")==sha(run)
    assert not any("autostart" in p.parts or "systemd" in p.parts for p in (temp/"deb").rglob("*"))
    for size in [16,24,32,48,64,128,256,512]:
        rel=f"hicolor/{size}x{size}/apps/{APP}.png"
        assert sha(temp/"deb/usr/share/icons"/rel)==sha(ROOT/"assets/icons"/rel)
    print("DEB_RUN_BYTES_DESKTOP_ENTRY_EIGHT_ICON_SIZES_NO_STARTUP_PASS",flush=True)
    env=dict(os.environ,XDG_DATA_HOME=str(temp/"data"),XDG_CONFIG_HOME=str(temp/"config"),
        ENCRYPT_ASSETS_DATA_DIR=str(temp/"state"),GDK_BACKEND="x11",GSK_RENDERER="cairo")
    version=subprocess.check_output(["sh",str(run),"--version"],env=env,text=True).strip()
    assert version=="Encrypt your digital assets Ubuntu 0.1.1",version
    with (temp/"app.log").open("w") as log:
        child=subprocess.Popen(["sh",str(run)],env=env,stdout=log,stderr=log,start_new_session=True)
        try:
            win=None
            for _ in range(150):
                assert child.poll() is None, (temp/"app.log").read_text()
                # GTK also creates an unmapped application-leader window. Its
                # name can match, but it intentionally has no window icon.
                found=subprocess.run(["xdotool","search","--onlyvisible","--pid",str(child.pid),"--name","Encrypt your digital assets"],capture_output=True,text=True)
                if found.returncode==0 and found.stdout.strip():
                    win=found.stdout.splitlines()[0];break
                time.sleep(.2)
            assert win,"Production admin window missing"
            entry=temp/f"data/applications/{APP}.desktop"
            validate_entry(entry)
            assert f"Icon={APP}\n" in entry.read_text()
            cache=temp/f"state/icons/hicolor/512x512/apps/{APP}.png"
            assert sha(cache)==sha(ROOT/"assets/app-icon.png")
            for size in [16,24,32,48,64,128,256,512]:
                rel=f"hicolor/{size}x{size}/apps/{APP}.png"
                assert sha(temp/"data/icons"/rel)==sha(ROOT/"assets/icons"/rel)
            for _ in range(30):
                prop=subprocess.check_output(["xprop","-id",win,"_NET_WM_ICON"],text=True)
                if "CARDINAL" in prop: break
                time.sleep(.1)
            assert "CARDINAL" in prop,prop[:200]
            assert not (temp/"config/autostart").exists()
            assert not (temp/"config/systemd").exists()
            print("PRODUCTION_RUN_WINDOW_ICON_LOCAL_MENU_CACHE_NO_STARTUP_PASS",flush=True)
        finally:
            if child.poll() is None:
                os.killpg(child.pid,signal.SIGTERM)
                child.wait(timeout=10)

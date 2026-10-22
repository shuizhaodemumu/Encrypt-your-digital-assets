#!/usr/bin/env python3
"""Build a self-contained Ubuntu x86_64 offline installer and a .deb wrapper.

The runtime contains GTK, LibVLC plugins, Poppler, LibreOffice, Python/UNO,
their shared-library closure, and the matching Linux ELF interpreter.
It installs per user without sudo, Docker, FUSE or downloading dependencies.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def copytree(source, destination):
    shutil.copytree(source, destination, symlinks=False, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "test", "tests", "config-*"))

def write(path, text, executable=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    path.chmod(0o755 if executable else 0o644)

def elf(path):
    try:
        with path.open("rb") as f:
            return f.read(4) == b"\x7fELF"
    except (OSError, IsADirectoryError):
        return False

def relocate_uno_module(path):
    # Ubuntu's python3-uno prepends absolute distribution paths to uno.py.
    # Preserve upstream code/notices, replacing only that packaging bootstrap.
    text = path.read_text()
    old = "sys.path.append('/usr/lib/libreoffice/program')\nos.putenv('URE_BOOTSTRAP', 'vnd.sun.star.pathname:/usr/lib/libreoffice/program/fundamentalrc')"
    if old not in text:
        raise RuntimeError("Unknown python3-uno bootstrap; review relocation before packaging")
    new = "# Encrypt-assets Ubuntu: relocate the Ubuntu UNO bootstrap to the private runtime.\n_uno_program = os.path.join(os.environ['ENCRYPT_ASSETS_RUNTIME'], 'libreoffice', 'program')\nsys.path.append(_uno_program)\nos.putenv('URE_BOOTSTRAP', 'vnd.sun.star.pathname:' + os.path.join(_uno_program, 'fundamentalrc'))"
    path.write_text(text.replace(old, new))

def build(binary, suffix):
    out = ROOT / "outputs"
    out.mkdir(exist_ok=True)
    work = ROOT / "work" / ("bundle-" + suffix)
    if work.exists():
        if work.resolve().parent != (ROOT / "work").resolve():
            raise RuntimeError("Unsafe staging path")
        shutil.rmtree(work)
    stage = work / "runtime"
    (stage / "bin").mkdir(parents=True)
    (stage / "lib").mkdir()
    shutil.copy2(binary, stage / "bin" / "encrypt-assets-ubuntu")
    shutil.copy2("/usr/bin/patchelf", stage / "bin/patchelf")
    copytree("/usr/lib/libreoffice", stage / "libreoffice")
    fundamental = stage / "libreoffice/program/fundamentalrc"
    fundamental.write_text(fundamental.read_text().replace("BRAND_BASE_DIR=file:///usr/lib/libreoffice", "BRAND_BASE_DIR=${ORIGIN}/.."))
    pyver = f"python{sys.version_info.major}.{sys.version_info.minor}"
    copytree("/usr/lib/" + pyver, stage / "python/lib" / pyver)
    shutil.copy2(Path(sys.executable).resolve(), stage / "bin" / "python-real")
    for name in ["uno.py", "unohelper.py"]:
        shutil.copy2(Path("/usr/lib/python3/dist-packages") / name, stage / "python/lib" / pyver / name)
    relocate_uno_module(stage / "python/lib" / pyver / "uno.py")
    for name in ["NotoSansCJK-Regular.ttc", "NotoSansCJK-Bold.ttc"]:
        source = Path("/usr/share/fonts/opentype/noto") / name
        if not source.is_file():
            raise RuntimeError("Install fonts-noto-cjk on the build machine")
        (stage / "share/fonts").mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, stage / "share/fonts" / name)
    write(stage / "share/fontconfig/fonts.conf", '<?xml version="1.0"?>\n<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">\n<fontconfig><include ignore_missing="yes">/etc/fonts/fonts.conf</include><dir prefix="relative">../fonts</dir></fontconfig>\n')
    copytree("/usr/lib/x86_64-linux-gnu/vlc/plugins", stage / "vlc/plugins")
    copytree("/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0", stage / "gdk-pixbuf")
    copytree("/usr/share/glib-2.0/schemas", stage / "share/glib-2.0/schemas")
    for source, relative in [("/usr/share/libreoffice", "share/libreoffice"), ("/usr/share/poppler", "share/poppler"), ("/usr/share/icons/Adwaita", "share/icons/Adwaita"), ("/usr/share/icons/hicolor", "share/icons/hicolor")]:
        if Path(source).is_dir():
            copytree(source, stage / relative)
    # Copy all dependencies of every plugin as well as the main programs.
    pending = [p for p in stage.rglob("*") if p.is_file() and elf(p)]
    seen = set()
    origin_libraries = {}
    missing = set()
    search_env = dict(os.environ, LD_LIBRARY_PATH="/usr/lib/libreoffice/program:/usr/lib/x86_64-linux-gnu")
    while pending:
        path = pending.pop()
        key = str(path.resolve())
        if key in seen:
            continue
        seen.add(key)
        proc = subprocess.run(["ldd", str(path)], capture_output=True, text=True, env=search_env)
        for line in proc.stdout.splitlines():
            if "not found" in line:
                missing.add((path.name, line.strip()))
            match = re.search(r"(?:=>\s+)?(/[^\s]+)", line)
            if not match:
                continue
            src = Path(match.group(1))
            if not src.is_file():
                continue
            name = src.name
            dest = stage / "lib" / name
            if dest.exists():
                continue
            shutil.copy2(src.resolve(), dest)
            origin_libraries[name] = str(src)
            pending.append(dest)
    # Some optional LO and VLC plugins have deliberately absent optional deps;
    # list them explicitly instead of silently calling the package complete.
    (work / "missing-libraries.json").write_text(json.dumps(sorted(missing), indent=2), encoding="utf-8")
    if missing:
        raise RuntimeError("Unresolved runtime dependencies; inspect " + str(work / "missing-libraries.json"))
    if not (stage / "lib/ld-linux-x86-64.so.2").exists():
        raise RuntimeError("Missing ELF interpreter")
    write(stage / "bin/libreoffice", '''#!/bin/sh
set -eu
r=${ENCRYPT_ASSETS_RUNTIME:?}
attempt=0
while :; do
    if "$r/libreoffice/program/soffice.bin" "$@"; then exit 0; else code=$?; fi
    case "$code" in
        79|81) attempt=$((attempt+1)); [ "$attempt" -lt 3 ] || exit "$code" ;;
        *) exit "$code" ;;
    esac
done
''', True)
    write(stage / "bin/python3", '''#!/bin/sh
set -eu
r=${ENCRYPT_ASSETS_RUNTIME:?}
export PYTHONHOME="$r/python"
export PYTHONPATH="$r/libreoffice/program:$r/python/lib/''' + pyver + '''"
exec "$r/lib/ld-linux-x86-64.so.2" --library-path "$r/libreoffice/program:$r/lib" "$r/bin/python-real" "$@"
''', True)
    write(stage / "launch", '''#!/bin/sh
set -eu
r=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export ENCRYPT_ASSETS_RUNTIME="$r"
export ENCRYPT_ASSETS_EXECUTABLE="$r/bin/encrypt-assets-ubuntu"
export ENCRYPT_ASSETS_PYTHON="$r/bin/python3"
export PATH="$r/bin:$PATH"
export VLC_PLUGIN_PATH="$r/vlc/plugins"
export GSETTINGS_SCHEMA_DIR="$r/share/glib-2.0/schemas"
export FONTCONFIG_FILE="$r/share/fontconfig/fonts.conf"
export XDG_DATA_DIRS="$r/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export GSK_RENDERER=cairo
exec "$r/bin/encrypt-assets-ubuntu" "$@"
''', True)
    write(stage / "relocate", '''#!/bin/sh
set -eu
r=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
target=${1:-$r}
"$r/lib/ld-linux-x86-64.so.2" --library-path "$r/lib" "$r/bin/patchelf" --set-interpreter "$target/lib/ld-linux-x86-64.so.2" --force-rpath --set-rpath "$target/lib:$target/libreoffice/program" "$r/bin/encrypt-assets-ubuntu"
"$r/lib/ld-linux-x86-64.so.2" --library-path "$r/lib" "$r/bin/patchelf" --set-interpreter "$target/lib/ld-linux-x86-64.so.2" --force-rpath --set-rpath "$target/libreoffice/program:$target/lib" "$r/libreoffice/program/soffice.bin"
''', True)
    docs = stage / "docs"
    docs.mkdir()
    for file in ["README.md", "VALIDATION.md"]:
        if (ROOT / file).exists():
            shutil.copy2(ROOT / file, stage / file)
    for file in ["安装与使用.md", "artwork-provenance.json"]:
        if (ROOT / "docs" / file).exists():
            shutil.copy2(ROOT / "docs" / file, docs / file)
    # Ship our app identity in the private icon theme, independent of host themes.
    copytree(ROOT / "assets/icons/hicolor", stage / "share/icons/hicolor")
    subprocess.run(["gtk-update-icon-cache", "-f", "-t", str(stage / "share/icons/hicolor")], check=True)
    shutil.copy2(ROOT / "assets/app-icon.png", stage / "app-icon.png")
    shutil.copy2(ROOT / "docs/app-icon-provenance.json", docs / "app-icon-provenance.json")
    # Include redistribution notices from the installed Ubuntu packages.
    notices = docs / "third-party"
    notices.mkdir()
    packages = set()
    for lib in origin_libraries.values():
        result = subprocess.run(["dpkg-query", "-S", lib], capture_output=True, text=True)
        for line in result.stdout.splitlines():
            if ": /" in line:
                packages.add(line.split(": /", 1)[0].split(":", 1)[0])
    packages.update(["libreoffice-core", "libreoffice-impress", "libreoffice-writer", "python3-uno", "vlc-plugin-base", "vlc-plugin-video-output", pyver, "libgtk-4-1", "libpoppler-glib8", "fonts-noto-cjk"])
    for package in sorted(packages):
        copyright = Path("/usr/share/doc") / package / "copyright"
        if copyright.exists():
            shutil.copy2(copyright, notices / (package + ".copyright"))
    write(docs / "runtime-manifest.json", json.dumps({"builder": "Ubuntu 26.04 x86_64", "private_loader": True, "libraries": origin_libraries, "packages": sorted(packages)}, ensure_ascii=False, indent=2))
    archive = work / "runtime.tar.gz"
    with tarfile.open(archive, "w:gz", compresslevel=3) as tar:
        tar.add(stage, arcname=".")
    digest = hashlib.file_digest(archive.open("rb"), "sha256").hexdigest()
    size = archive.stat().st_size
    header = r'''#!/bin/sh
set -eu
self=$(readlink -f -- "$0")
base=${XDG_DATA_HOME:-$HOME/.local/share}/encrypt-assets-runtime
dest=$base/@HASH@
umask 077
if [ ! -f "$dest/.complete" ]; then
    mkdir -p -- "$base"
    tmp=$(mktemp -d "$base/.install-XXXXXX")
    trap 'rm -rf -- "$tmp"' EXIT HUP INT TERM
    tail -c +@OFFSET@ -- "$self" | head -c @SIZE@ > "$tmp/archive.tar.gz"
    actual=$(sha256sum "$tmp/archive.tar.gz")
    [ "${actual%% *}" = '@HASH@' ] || { echo 'Installer integrity check failed' >&2; exit 1; }
    tar -xzf "$tmp/archive.tar.gz" -C "$tmp"
    rm -- "$tmp/archive.tar.gz"
    "$tmp/relocate" "$dest"
    touch "$tmp/.complete"
    if mv -T -- "$tmp" "$dest" 2>/dev/null; then
        :
    elif [ -f "$dest/.complete" ]; then
        rm -rf -- "$tmp"
    else
        echo 'Runtime installation could not be completed' >&2
        exit 1
    fi
    trap - EXIT HUP INT TERM
fi
export ENCRYPT_ASSETS_BUNDLE="$self"
if [ "$#" -eq 0 ] && [ "$(tail -c 32 -- "$self" | head -c 8 | od -An -tx1 | tr -d ' \n')" != '4f4c455845303031' ]; then
    stable=$base/EncryptAssetsUbuntu.run
    if [ "$self" != "$stable" ]; then cp -- "$self" "$stable.new"; chmod 700 "$stable.new"; mv -- "$stable.new" "$stable"; fi
    export ENCRYPT_ASSETS_BUNDLE="$stable"
    apps=${XDG_DATA_HOME:-$HOME/.local/share}/applications
    mkdir -p -- "$apps"
    icons=${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor
    for size in 16 24 32 48 64 128 256 512; do
        mkdir -p -- "$icons/${size}x${size}/apps"
        cp -- "$dest/share/icons/hicolor/${size}x${size}/apps/io.github.shuizhaodemumu.EncryptAssetsUbuntu.png" "$icons/${size}x${size}/apps/"
    done
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -f -t "$icons" >/dev/null 2>&1 || :; fi
    escaped=$(printf '%s' "$stable" | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g; s/%/%%/g')
    # Match the GTK application ID so Wayland shells associate the window icon.
    # Remove only the old entry generated by this same per-user launcher.
    if [ -f "$apps/encrypt-assets-ubuntu.desktop" ] && grep -Fqx "Exec=\"$escaped\"" "$apps/encrypt-assets-ubuntu.desktop"; then
        rm -- "$apps/encrypt-assets-ubuntu.desktop"
    fi
    cat > "$apps/io.github.shuizhaodemumu.EncryptAssetsUbuntu.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Encrypt your digital assets
Name[zh_CN]=离线内容加密与授权
Exec="$escaped"
Icon=io.github.shuizhaodemumu.EncryptAssetsUbuntu
Terminal=false
Categories=Utility;
StartupWMClass=io.github.shuizhaodemumu.EncryptAssetsUbuntu
DESKTOP
fi
exec "$dest/launch" "$@"
exit 0
'''
    header = header.replace("@HASH@", digest).replace("@SIZE@", str(size))
    offset = 0
    while True:
        text = header.replace("@OFFSET@", str(offset))
        calculated = len(text.encode("utf-8")) + 1
        if calculated == offset:
            break
        offset = calculated
    runfile = out / f"Encrypt-assets-Ubuntu-0.1.3-{suffix}-x86_64.run"
    with runfile.open("wb") as f:
        f.write(text.encode("utf-8"))
        with archive.open("rb") as a:
            shutil.copyfileobj(a, f)
    runfile.chmod(0o755)
    print("OFFLINE_INSTALLER_BUILT", runfile.name, runfile.stat().st_size, flush=True)
    if suffix == "release":
        deb = work / "deb"
        target = deb / "opt/encrypt-assets-ubuntu"
        target.mkdir(parents=True)
        shutil.copy2(runfile, target / "EncryptAssetsUbuntu.run")
        write(deb / "DEBIAN/control", "Package: encrypt-assets-ubuntu\nVersion: 0.1.3\nArchitecture: amd64\nMaintainer: shuizhaodemumu\nSection: utils\nPriority: optional\nDepends: bash, coreutils, tar, gzip\nDescription: Offline encrypted media licensing desktop\n Includes private GTK, VLC, Poppler, LibreOffice and Python runtimes.\n Does not install login or boot autostart entries.\n")
        write(deb / "usr/share/applications/io.github.shuizhaodemumu.EncryptAssetsUbuntu.desktop", "[Desktop Entry]\nType=Application\nName=Encrypt your digital assets\nName[zh_CN]=离线内容加密与授权\nExec=/opt/encrypt-assets-ubuntu/EncryptAssetsUbuntu.run\nIcon=io.github.shuizhaodemumu.EncryptAssetsUbuntu\nTerminal=false\nCategories=Utility;\nStartupWMClass=io.github.shuizhaodemumu.EncryptAssetsUbuntu\n")
        copytree(ROOT / "assets/icons/hicolor", deb / "usr/share/icons/hicolor")
        subprocess.run(["dpkg-deb", "--build", "--root-owner-group", "-Zgzip", "-z1", str(deb), str(out / "encrypt-assets-ubuntu_0.1.3_amd64.deb")], check=True)
    return runfile

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("--suffix", choices=["test", "release"], default="test")
    args = parser.parse_args()
    build(args.binary.resolve(), args.suffix)

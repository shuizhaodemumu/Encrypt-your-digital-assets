import os,sys,subprocess,tempfile,pathlib
def rewrite(text):
    for root,drive in [(r'D:\codex create\加密软件\VLC视频重构版\安卓视频版','Y:'),(r'D:\codex create\驱动总裁Rust重构','V:')]:
        for prefix in [root.replace('\\','\\\\').replace(' ','\\ '),root.replace('\\','\\\\'),root.replace('\\','/'),root]:text=text.replace(prefix,drive)
    return text
args=[];temporary=[]
try:
    for arg in sys.argv[1:]:
        if arg.startswith('@') and pathlib.Path(arg[1:]).is_file():
            contents=pathlib.Path(arg[1:]).read_text(encoding='utf8');fd,name=tempfile.mkstemp(suffix='.rsp',dir=r'Y:\work');os.close(fd)
            pathlib.Path(name).write_text(rewrite(contents),encoding='utf8');temporary.append(name);args.append('@'+name)
        else:args.append(rewrite(arg))
    sys.exit(subprocess.run([r'V:\work\rust-tools\mingw\mingw64\bin\gcc.exe',*args],shell=False).returncode)
finally:
    for p in temporary:os.unlink(p)

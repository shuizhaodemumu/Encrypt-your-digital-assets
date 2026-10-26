"""Build a reproducible local APK with the already-installed Android/JDK toolchain."""
import pathlib,subprocess,os,zipfile,xml.etree.ElementTree as ET,shutil,sys,secrets,atexit
actual_root=pathlib.Path(__file__).resolve().parent.parent
root=pathlib.Path('W:/')
if not root.exists():
 subprocess.run(['subst','W:',str(actual_root)],check=True)
 atexit.register(lambda:subprocess.run(['subst','W:','/D'],check=False))
if not os.path.samefile(root,actual_root):raise RuntimeError('W: is already used by another directory')
tools=pathlib.Path(r'D:\codex create\2026-10-14\zui\work\android-tools')
out=root/'work/apk';out.mkdir(parents=True,exist_ok=True)
jdk=tools/'jdk/jdk-17.0.20.1+1';bt=tools/'build/android-15';platform=tools/'platform/android-35/android.jar'
deps=tools/'deps';vlc=root/'work/deps/libvlc';debug='--debug' in sys.argv
def run(a):
 print('RUN',pathlib.Path(str(a[0])).name,flush=True);subprocess.run([str(x) for x in a],check=True,cwd=root)
aar_dirs=[vlc]+[p for p in deps.iterdir() if p.is_dir() and (p/'classes.jar').exists() and not p.name.startswith('dfu')]
jars=[p/'classes.jar' for p in aar_dirs]+[p for p in deps.glob('*.jar') if not p.name.startswith('gson')]
compiled=[];packages=[]
for idx,d in enumerate(aar_dirs+[root/'app']):
 if d!=root/'app' and (d/'AndroidManifest.xml').exists():
  package=ET.parse(d/'AndroidManifest.xml').getroot().attrib.get('package');
  if package:packages.append(package)
 res=d/'res'
 if res.exists() and any(res.rglob('*.*')):
  archive=out/f'res-{idx}.zip';run([bt/'aapt2.exe','compile','--dir',res,'-o',archive]);compiled.append(archive)
generated=out/'generated';generated.mkdir(exist_ok=True)
manifest=root/'app/AndroidManifest.xml'
if debug:
 tree=ET.parse(manifest);tree.getroot().find('application').set('{http://schemas.android.com/apk/res/android}debuggable','true');manifest=out/'debug-manifest.xml';tree.write(manifest,encoding='utf-8')
args=[bt/'aapt2.exe','link','-o',out/'resources.apk','--manifest',manifest,'-I',platform,'--java',generated,'--auto-add-overlay','--extra-packages',':'.join(packages),'-A',root/'app/assets','--min-sdk-version','26','--target-sdk-version','35']
for p in compiled:args+=['-R',p]
run(args)
classes=out/'classes';classes.mkdir(exist_ok=True)
sources=list((root/'app/src').rglob('*.java'))+list(generated.rglob('*.java'))
argfile=out/'javac.args';argfile.write_text('\n'.join('"'+p.as_posix()+'"' for p in sources),encoding='utf8')
run([jdk/'bin/javac.exe','-J-Dfile.encoding=UTF-8','-encoding','UTF-8','-source','8','-target','8','-classpath',os.pathsep.join(str(p) for p in [platform,*jars]),'-d',classes,'@'+str(argfile)])
own=out/'classes.jar'
with zipfile.ZipFile(own,'w',zipfile.ZIP_DEFLATED) as z:
 for p in classes.rglob('*.class'):z.write(p,p.relative_to(classes).as_posix())
dex=out/'dex';dex.mkdir(exist_ok=True)
run([jdk/'bin/java.exe','-Xmx2g','-cp',tools/'r8-8.13.19.jar','com.android.tools.r8.D8','--release','--min-api','26','--lib',platform,'--output',dex,own,*jars])
unsigned=out/'unsigned.apk';shutil.copyfile(out/'resources.apk',unsigned)
with zipfile.ZipFile(unsigned,'a',zipfile.ZIP_DEFLATED) as z:
 for p in dex.glob('*.dex'):z.write(p,p.name)
 z.write(root/'core/target/aarch64-linux-android/release/libencrypt_video.so','lib/arm64-v8a/libencrypt_video.so')
 for p in (vlc/'jni/arm64-v8a').glob('*.so'):z.write(p,'lib/arm64-v8a/'+p.name)
aligned=out/'aligned.apk';run([bt/'zipalign.exe','-P','16','-f','4',unsigned,aligned])
sign=root/'work/signing';sign.mkdir(exist_ok=True);key=sign/'android-preview.p12';secret=sign/'password.txt'
if not key.exists():
 secret.write_text(secrets.token_urlsafe(36),encoding='utf8')
 run([jdk/'bin/keytool.exe','-genkeypair','-keystore',key,'-storetype','PKCS12','-alias','video','-keyalg','RSA','-keysize','3072','-validity','7300','-storepass:file',secret,'-dname','CN=Encrypt Assets Android Preview'])
output=root/'outputs'/('EncryptVideo-0.1.0-arm64-debug.apk' if debug else 'EncryptVideo-0.1.0-arm64.apk')
run([jdk/'bin/java.exe','-jar',bt/'lib/apksigner.jar','sign','--ks',key,'--ks-key-alias','video','--ks-pass','file:'+str(secret),'--out',output,aligned])
run([jdk/'bin/java.exe','-jar',bt/'lib/apksigner.jar','verify','--verbose',output])
print('APK',output,output.stat().st_size,flush=True)


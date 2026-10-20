"""Read actual PE resources, not file names, to verify the delivered icon."""
from pathlib import Path
import struct
import sys
ROOT = Path(__file__).resolve().parent.parent

def resources(path):
    data = path.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    assert data[pe:pe+4] == b"PE\0\0"
    count = struct.unpack_from("<H", data, pe+6)[0]
    optional_size = struct.unpack_from("<H", data, pe+20)[0]
    optional = pe+24
    assert struct.unpack_from("<H", data, optional)[0] == 0x20b
    resource_rva = struct.unpack_from("<I", data, optional+112+16)[0]
    sections = []
    for i in range(count):
        entry = optional+optional_size+i*40
        size, rva, rawsize, raw = struct.unpack_from("<IIII", data, entry+8)
        sections.append((rva, max(size,rawsize),raw))
    def offset(rva):
        for start,size,raw in sections:
            if start <= rva < start+size:
                return raw+rva-start
        raise AssertionError("Resource RVA not mapped")
    base = offset(resource_rva)
    result = {}
    def walk(relative, key=()):
        at = base+relative
        named, ids = struct.unpack_from("<HH",data,at+12)
        for i in range(named+ids):
            name,target = struct.unpack_from("<II",data,at+16+i*8)
            if name & 0x80000000:
                n=base+(name&0x7fffffff)
                length=struct.unpack_from("<H",data,n)[0]
                name=data[n+2:n+2+length*2].decode("utf-16le")
            if target & 0x80000000:
                walk(target&0x7fffffff,key+(name,))
            else:
                rva,size=struct.unpack_from("<II",data,base+target)
                start=offset(rva)
                result[key+(name,)]=data[start:start+size]
    walk(0)
    return result

ico=(ROOT/"assets/app-icon.ico").read_bytes()
count=struct.unpack_from("<H",ico,4)[0]
expected=[]
for i in range(count):
    size,offset=struct.unpack_from("<II",ico,6+i*16+8)
    expected.append(ico[offset:offset+size])
paths=[Path(s) for s in sys.argv[1:]] or [ROOT/"target/debug/offline-license-rust.exe"]
for p in paths:
    r=resources(p)
    actual=[v for key,v in r.items() if key[0]==3]
    assert len(actual)==count and sorted(actual)==sorted(expected),p
    assert any(k[0]==14 for k in r),"Missing group icon"
    assert any(k[0]==24 for k in r),"Missing visual styles manifest"
    print("PE_EMBEDDED_ICON_7_SIZES_AND_MANIFEST_PASS",p.name)

import struct, sys
with open(sys.argv[1],"rb") as f: d=f.read()
e=struct.unpack_from("<Q",d,24)[0]
print("Entry:",hex(e))
n=struct.unpack_from("<H",d,56)[0]
off=struct.unpack_from("<Q",d,32)[0]
sz=struct.unpack_from("<H",d,54)[0]
for i in range(n):
    t=struct.unpack_from("<I",d,off+i*sz)[0]
    va=struct.unpack_from("<Q",d,off+i*sz+16)[0]
    pa=struct.unpack_from("<Q",d,off+i*sz+24)[0]
    fs=struct.unpack_from("<Q",d,off+i*sz+32)[0]
    ms=struct.unpack_from("<Q",d,off+i*sz+40)[0]
    typ="LOAD" if t==1 else ("RELRO" if t==4 else ("STACK" if t==6 else hex(t)))
    print(f"  {typ}: vaddr={hex(va)} paddr={hex(pa)} filesz={hex(fs)} memsz={hex(ms)}")

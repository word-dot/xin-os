import struct
import sys

img_path = "/home/yuxing/rCore-Tutorial-v3/sdcard-rv.img"
with open(img_path, "rb") as f:
    data = f.read(50*1024*1024)

offsets = []
pos = 0
while pos < len(data) - 4:
    if data[pos:pos+4] == b'\x7fELF':
        offsets.append(pos)
        if len(offsets) >= 10:
            break
    pos += 1

print(f"Found {len(offsets)} ELF headers")
for off in offsets:
    print(f"  Offset: {off} (0x{off:x})")
    if off + 64 <= len(data):
        entry = struct.unpack("<Q", data[off+24:off+32])[0]
        phoff = struct.unpack("<Q", data[off+32:off+40])[0]
        phnum = struct.unpack("<H", data[off+56:off+58])[0]
        phentsize = struct.unpack("<H", data[off+54:off+56])[0]
        print(f"  Entry: 0x{entry:x}")
        print(f"  Phoff: {phoff}, Phnum: {phnum}, Phentsize: {phentsize}")
        for i in range(min(phnum, 5)):
            phdr_off = off + phoff + i * phentsize
            if phdr_off + 56 <= len(data):
                p_type = struct.unpack("<I", data[phdr_off:phdr_off+4])[0]
                p_vaddr = struct.unpack("<Q", data[phdr_off+16:phdr_off+24])[0]
                p_memsz = struct.unpack("<Q", data[phdr_off+40:phdr_off+48])[0]
                p_flags = struct.unpack("<I", data[phdr_off+4:phdr_off+8])[0]
                print(f"  Segment {i}: type={p_type}, vaddr=0x{p_vaddr:x}, memsz=0x{p_memsz:x}, flags=0x{p_flags:x}")

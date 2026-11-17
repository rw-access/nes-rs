#!/usr/bin/env python3
"""Generate original, redistributable mapper probes; no game ROMs required.

MMC4 renders tiles selected by a read of the upper selected PRG window.
VRC2/4 render a CHR page selected through both register nibbles.
VRC6 changes the background palette from its CPU-mode IRQ handler.
Each should reach a stable nonblack screen in both nes-rs and a reference core.
The native ignored-contract tests preserve exact expected register semantics.
"""
import argparse
from pathlib import Path


def fixture(mapper):
    prg = bytearray(0x20000)
    for page in range(16):
        prg[page * 0x2000:(page + 1) * 0x2000] = bytes([page]) * 0x2000
    chr_rom = bytearray(0x8000)
    for page in range(32):
        for tile in range(64):
            color = (tile & 3) if mapper == 10 else (2 if page == 16 else 1)
            offset = page * 0x400 + tile * 16
            chr_rom[offset:offset + 8] = bytes([255 if color & 1 else 0]) * 8
            chr_rom[offset + 8:offset + 16] = bytes([255 if color & 2 else 0]) * 8
    origin = 0xe000
    code = bytearray()

    def emit(*values):
        code.extend(values)

    def store(address, value):
        emit(0xa9, value, 0x8d, address & 255, address >> 8)

    def branch(opcode, target):
        emit(opcode, (target - (len(code) + 2)) & 255)

    emit(0x78, 0xd8, 0xa2, 0xff, 0x9a)  # SEI, CLD, LDX #FF, TXS
    store(0x2000, 0)
    store(0x2001, 0)
    for _ in range(2):
        wait = len(code)
        emit(0x2c, 0x02, 0x20)  # BIT PPUSTATUS
        branch(0x10, wait)  # BPL
    if mapper == 10:
        store(0xa000, 2)
        for address in [0xb000, 0xc000, 0xd000, 0xe000]:
            store(address, 0)
        emit(0xad, 0x00, 0xa0, 0x85, 0x00)  # page tag 5 at A000 -> tile 5
    else:
        if mapper in [21, 22, 23, 25]:
            high_address = 0xb001 if mapper == 23 else 0xb002
            store(0xb000, 0)
            store(high_address, 2 if mapper == 22 else 1)  # physical CHR page 16
        store(0x0000, 1)  # tile 1
    store(0x2006, 0x3f)
    store(0x2006, 0)
    for _ in range(4):
        for color in [0x0f, 0x16, 0x2a, 0x12]:
            store(0x2007, color)
    store(0x2006, 0x20)
    store(0x2006, 0)
    emit(0xa2, 0, 0xa0, 4)  # 1024 identical tiles/attributes; all palettes equal
    loop = len(code)
    emit(0xa5, 0, 0x8d, 7, 0x20, 0xe8)
    branch(0xd0, loop)
    emit(0x88)
    branch(0xd0, loop)
    store(0x2006, 0x20)
    store(0x2006, 0)
    store(0x2005, 0)
    store(0x2005, 0)
    if mapper in [24, 26]:
        store(0xf000, 0xff)
        store(0xf001 if mapper == 24 else 0xf002, 6)
        emit(0x58)  # CLI: handler changes palette if IRQ layout/count is correct.
    store(0x2001, 0x0a)
    here = origin + len(code)
    emit(0x4c, here & 255, here >> 8)
    assert len(code) < 0x300
    prg[-0x2000:-0x2000 + len(code)] = code
    handler = bytearray()
    if mapper in [24, 26]:
        handler.append(0x48)  # PHA
        for address, value in [(0xf002 if mapper == 24 else 0xf001, 0),
                               (0x2006, 0x3f), (0x2006, 1), (0x2007, 0x2a),
                               (0x2006, 0x20), (0x2006, 0),
                               (0x2005, 0), (0x2005, 0)]:
            handler.extend([0xa9, value, 0x8d, address & 255, address >> 8])
        handler.append(0x68)  # PLA
    handler.append(0x40)  # RTI
    prg[-0x1d00:-0x1d00 + len(handler)] = handler
    prg[-6:] = bytes([0, 0xe3, 0, 0xe0, 0, 0xe3])
    header = bytearray(b'NES\x1a' + bytes(12))
    header[4:8] = bytes([8, 4, ((mapper & 15) << 4) | 1, mapper & 0xf0])
    return header + prg + chr_rom


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    rows = []
    for mapper in [10, 21, 22, 23, 24, 25, 26]:
        name = f'audit-{mapper:03}.nes'
        (args.output / name).write_bytes(fixture(mapper))
        rows.append(f'{mapper}\t{name}\t60\t61\t120\tOriginal synthetic mapper probe\n')
    (args.output / 'cases.tsv').write_text(''.join(rows))


if __name__ == '__main__':
    main()

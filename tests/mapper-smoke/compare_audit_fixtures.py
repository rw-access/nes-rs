#!/usr/bin/env python3
"""Compare the original audit probes by pixel color class, allowing RGB palettes.

Both cores must produce the same red/green/black spatial pattern at both
capture frames. This is intentionally limited to these simple generated ROMs;
it is not a general game screenshot oracle.
"""
import argparse
import json
import struct
import zlib
from pathlib import Path


def rgb_pixels(path):
    data = path.read_bytes()
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError(f'{path}: not PNG')
    offset, compressed = 8, bytearray()
    while offset < len(data):
        size = int.from_bytes(data[offset:offset + 4], 'big')
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        if kind == b'IHDR':
            width, height, depth, color, compression, filter_method, interlace = struct.unpack('>IIBBBBB', payload)
            if depth != 8 or color not in (2, 6) or compression or filter_method or interlace:
                raise ValueError(f'{path}: unsupported PNG format')
        elif kind == b'IDAT':
            compressed.extend(payload)
        offset += size + 12
    channels = 3 if color == 2 else 4
    stride = width * channels
    raw = zlib.decompress(compressed)
    previous = bytearray(stride)
    pixels = []
    for y in range(height):
        start = y * (stride + 1)
        mode = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for x in range(stride):
            left = row[x - channels] if x >= channels else 0
            up = previous[x]
            upper_left = previous[x - channels] if x >= channels else 0
            if mode == 0:
                predictor = 0
            elif mode == 1:
                predictor = left
            elif mode == 2:
                predictor = up
            elif mode == 3:
                predictor = (left + up) // 2
            elif mode == 4:
                p = left + up - upper_left
                predictor = min((left, up, upper_left), key=lambda v: abs(p - v))
            else:
                raise ValueError(f'{path}: bad PNG filter {mode}')
            row[x] = (row[x] + predictor) & 255
        pixels.extend(tuple(row[x:x + 3]) for x in range(0, stride, channels))
        previous = row
    return (width, height), pixels


def classes(pixels):
    return bytes(0 if max(pixel) < 50 else 1 + max(range(3), key=lambda channel: pixel[channel])
                 for pixel in pixels)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('actual', type=Path)
    parser.add_argument('reference', type=Path)
    args = parser.parse_args()
    results = []
    for mapper in [10, 21, 22, 23, 24, 25, 26]:
        for stage in ['boot', 'post-start']:
            actual_size, actual = rgb_pixels(args.actual / f'{mapper:03}' / f'{stage}.png')
            ref_size, reference = rgb_pixels(args.reference / f'{mapper:03}' / f'{stage}.png')
            a, b = classes(actual), classes(reference)
            assert actual_size == ref_size == (256, 240)
            mismatches = sum(x != y for x, y in zip(a, b))
            expected = {1} if mapper == 10 else ({1, 2} if mapper in [24, 26] else {2})
            assert set(a) == expected, (mapper, stage, set(a), expected)
            assert mismatches == 0, (mapper, stage, mismatches)
            results.append(dict(mapper=mapper, stage=stage, differing_color_classes=mismatches))
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()

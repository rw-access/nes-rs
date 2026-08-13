"""CFFI declarations and shared-library discovery for nes-rs.

The declarations intentionally contain only opaque handles.  The current
nes-rs library uses status-returning functions with output handle pointers:
``nes_create(rom_ptr, rom_len, handle_out)`` and
``nes_snapshot(handle, snapshot_out)``.  Zero status means success.
"""

from __future__ import annotations

import os
import platform
from pathlib import Path
from typing import Any

from cffi import FFI

_CDEF = r"""
typedef unsigned char uint8_t;
typedef unsigned int uint32_t;
typedef unsigned long long size_t;
typedef unsigned int NesStatus;
typedef _Bool bool;
typedef struct NesHandle NesHandle;
typedef struct NesSnapshot NesSnapshot;

NesStatus nes_create(const uint8_t *rom_ptr, size_t rom_len,
                     NesHandle **out_handle);
void nes_destroy(NesHandle *handle);
NesStatus nes_advance_frames(NesHandle *handle, uint8_t controller_bits,
                             uint32_t frames);
NesStatus nes_rewind(NesHandle *handle, bool *out_rewound);
NesStatus nes_snapshot(const NesHandle *handle, NesSnapshot **out_snapshot);
NesStatus nes_restore(NesHandle *handle, const NesSnapshot *snapshot);
void nes_snapshot_destroy(NesSnapshot *snapshot);
const uint8_t *nes_ram_view(const NesHandle *handle);
size_t nes_ram_view_len(void);
const char *nes_last_error(void);
NesStatus nes_set_video_output(NesHandle *handle, bool enabled);
const uint8_t *nes_framebuffer_view(const NesHandle *handle);
size_t nes_framebuffer_view_len(void);
const float *nes_audio_view(const NesHandle *handle);
size_t nes_audio_view_len(const NesHandle *handle);
uint32_t nes_audio_sample_rate(void);
"""


def make_ffi() -> FFI:
    """Create a fresh CFFI ABI-mode object."""

    ffi = FFI()
    ffi.cdef(_CDEF)
    return ffi


def _candidate_library_names() -> tuple[str, ...]:
    system = platform.system()
    if system == "Windows":
        return ("nes_ffi.dll", "libnes_ffi.dll")
    if system == "Darwin":
        return ("libnes_ffi.dylib", "nes_ffi.dylib")
    return ("libnes_ffi.so", "nes_ffi.so")


def find_library(path: str | os.PathLike[str] | None = None) -> Path:
    """Find the native library, honoring ``NES_FFI_LIBRARY`` first."""

    explicit = path or os.environ.get("NES_FFI_LIBRARY")
    if explicit:
        candidate = Path(explicit).expanduser().resolve()
        if not candidate.is_file():
            raise FileNotFoundError(f"nes-rs FFI library does not exist: {candidate}")
        return candidate

    # ffi.py lives below <repo>/python/src/nes_gym.
    repo = Path(__file__).resolve().parents[3]
    names = _candidate_library_names()
    candidates = [
        repo / "target" / profile / name
        for profile in ("release", "debug")
        for name in names
    ]
    candidates.extend(Path.cwd() / name for name in names)
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    searched = ", ".join(str(candidate) for candidate in candidates)
    raise FileNotFoundError(f"could not find nes-rs FFI library; searched: {searched}")


def load_library(
    path: str | os.PathLike[str] | None = None,
    *,
    ffi: FFI | None = None,
) -> tuple[FFI, Any, Path]:
    """Load the shared library and return ``(ffi, lib, resolved_path)``."""

    ffi = ffi or make_ffi()
    resolved = find_library(path)
    return ffi, ffi.dlopen(str(resolved)), resolved


def last_error(ffi: FFI, lib: Any) -> str:
    """Read the optional process-local native error string."""

    try:
        value = lib.nes_last_error()
    except AttributeError:
        return "native nes-rs operation failed"
    if value == ffi.NULL:
        return "native nes-rs operation failed"
    return ffi.string(value).decode("utf-8", errors="replace")

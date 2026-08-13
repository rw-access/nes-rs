"""Small Python wrapper around the opaque nes-rs C ABI."""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any

import numpy as np
from cffi import FFI

from .ffi import last_error, load_library, make_ffi


class NesCoreError(RuntimeError):
    """Raised when the native emulator reports an error."""


class NesSnapshot:
    """An owning handle to a cheap in-memory native emulator snapshot."""

    def __init__(self, core: "NesCore", handle: Any):
        self._core = core
        self._handle = handle

    @property
    def handle(self) -> Any:
        if self._handle == self._core.ffi.NULL:
            raise NesCoreError("snapshot has already been destroyed")
        return self._handle

    def close(self) -> None:
        if self._handle != self._core.ffi.NULL:
            self._core.lib.nes_snapshot_destroy(self._handle)
            self._handle = self._core.ffi.NULL

    destroy = close

    def __enter__(self) -> "NesSnapshot":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass


class NesCore:
    """Own one emulator and expose its stable 2 KiB RAM observation view."""

    def __init__(
        self,
        rom: str | Path | bytes | bytearray | memoryview,
        *,
        library: str | Path | None = None,
        ffi: FFI | None = None,
        lib: Any | None = None,
    ):
        self.ffi = ffi or make_ffi()
        if lib is None:
            self.ffi, lib, library_path = load_library(library, ffi=self.ffi)
            self.library_path: Path | None = library_path
        else:
            self.library_path = Path(library).resolve() if library else None
        self.lib = lib
        self._closed = False
        self._rom = self._read_rom(rom)
        rom_buffer = self.ffi.new("uint8_t[]", self._rom)
        # Retain the input buffer for ABIs that keep a borrowed ROM pointer.
        self._rom_buffer = rom_buffer
        out_handle = self.ffi.new("NesHandle **")
        status = self.lib.nes_create(rom_buffer, len(self._rom), out_handle)
        self._handle = out_handle[0]
        if self._handle == self.ffi.NULL or int(status) != 0:
            message = last_error(self.ffi, self.lib)
            if self._handle != self.ffi.NULL:
                self.lib.nes_destroy(self._handle)
                self._handle = self.ffi.NULL
            raise NesCoreError(f"nes_create failed ({int(status)}): {message}")

        self._ram_buffer: Any | None = None
        self._ram: np.ndarray | None = None
        try:
            self._bind_ram_view()
        except Exception:
            self.close()
            raise

    @staticmethod
    def _read_rom(rom: str | Path | bytes | bytearray | memoryview) -> bytes:
        if isinstance(rom, (str, Path)):
            return Path(rom).read_bytes()
        return bytes(rom)

    @property
    def ffi_handle(self) -> Any:
        self._require_open()
        return self._handle

    @property
    def ram(self) -> np.ndarray:
        self._require_open()
        assert self._ram is not None
        return self._ram

    @property
    def ram_view(self) -> np.ndarray:
        return self.ram

    @property
    def ram_len(self) -> int:
        return int(self.ram.size)

    @property
    def framebuffer_len(self) -> int:
        self._require_open()
        try:
            return int(self.lib.nes_framebuffer_view_len())
        except AttributeError as exc:
            raise NesCoreError("nes_framebuffer_view_len is required by the Python ABI") from exc

    def set_video_output(self, enabled: bool) -> None:
        """Enable or disable native framebuffer updates."""

        self._require_open()
        try:
            result = self.lib.nes_set_video_output(self._handle, bool(enabled))
        except AttributeError as exc:
            raise NesCoreError("nes_set_video_output is required by the Python ABI") from exc
        self._check_status("nes_set_video_output", result)

    @property
    def framebuffer(self) -> np.ndarray:
        """Return the native RGBA framebuffer as a NumPy view."""

        self._require_open()
        try:
            pointer = self.lib.nes_framebuffer_view(self._handle)
        except AttributeError as exc:
            raise NesCoreError("nes_framebuffer_view is required by the Python ABI") from exc
        if pointer == self.ffi.NULL:
            raise NesCoreError(f"nes_framebuffer_view failed: {last_error(self.ffi, self.lib)}")
        length = self.framebuffer_len
        return np.frombuffer(self.ffi.buffer(pointer, length), dtype=np.uint8, count=length)

    @property
    def rom_sha256(self) -> str:
        return hashlib.sha256(self._rom).hexdigest()

    def _bind_ram_view(self) -> None:
        pointer = self.lib.nes_ram_view(self._handle)
        if pointer == self.ffi.NULL:
            raise NesCoreError(f"nes_ram_view failed: {last_error(self.ffi, self.lib)}")
        try:
            length = int(self.lib.nes_ram_view_len())
        except AttributeError as exc:
            raise NesCoreError("nes_ram_view_len is required by the Python ABI") from exc
        if length != 2048:
            raise NesCoreError(f"expected a 2048-byte CPU RAM view, got {length}")
        self._ram_buffer = self.ffi.buffer(pointer, length)
        self._ram = np.frombuffer(self._ram_buffer, dtype=np.uint8, count=length)

    def _require_open(self) -> None:
        if self._closed or self._handle == self.ffi.NULL:
            raise NesCoreError("emulator is closed")

    def _check_status(self, operation: str, result: Any) -> None:
        if result is not None and int(result) != 0:
            raise NesCoreError(f"{operation} failed ({int(result)}): {last_error(self.ffi, self.lib)}")

    def advance_frames(self, controller_bits: int, frames: int = 1) -> None:
        self._require_open()
        if not 0 <= int(controller_bits) <= 0xFF:
            raise ValueError("controller_bits must fit in uint8_t")
        if frames < 0:
            raise ValueError("frames must be non-negative")
        result = self.lib.nes_advance_frames(
            self._handle, int(controller_bits), int(frames)
        )
        self._check_status("nes_advance_frames", result)

    def snapshot(self) -> NesSnapshot:
        self._require_open()
        out_snapshot = self.ffi.new("NesSnapshot **")
        status = self.lib.nes_snapshot(self._handle, out_snapshot)
        handle = out_snapshot[0]
        if int(status) != 0 or handle == self.ffi.NULL:
            if handle != self.ffi.NULL:
                self.lib.nes_snapshot_destroy(handle)
            raise NesCoreError(f"nes_snapshot failed: {last_error(self.ffi, self.lib)}")
        return NesSnapshot(self, handle)

    def restore(self, snapshot: NesSnapshot) -> None:
        self._require_open()
        if snapshot._core is not self:
            raise ValueError("snapshot belongs to a different emulator")
        result = self.lib.nes_restore(self._handle, snapshot.handle)
        self._check_status("nes_restore", result)

    def close(self) -> None:
        if not self._closed and self._handle != self.ffi.NULL:
            self.lib.nes_destroy(self._handle)
            self._handle = self.ffi.NULL
            self._closed = True
            self._ram = None
            self._ram_buffer = None

    destroy = close

    def __enter__(self) -> "NesCore":
        self._require_open()
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass

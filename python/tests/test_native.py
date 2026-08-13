import os

import pytest

from nes_gym.ffi import find_library


def test_native_library_is_optional():
    configured = os.environ.get("NES_FFI_LIBRARY")
    try:
        find_library()
    except FileNotFoundError:
        if configured:
            pytest.fail("NES_FFI_LIBRARY is set but its library is missing")
        pytest.skip("native nes-rs FFI library is not built")

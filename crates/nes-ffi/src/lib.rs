//! A small, layout-independent C ABI for the nes-core emulator.
//!
//! The ABI uses opaque heap handles. Every exported function catches Rust
//! panics before returning to C and reports failures through [`NesStatus`] and
//! [`nes_last_error`]. The RAM and framebuffer pointers are borrowed views
//! whose addresses remain stable for the lifetime of their emulator handle.

use std::{
    cell::RefCell,
    ffi::CString,
    io::Cursor,
    os::raw::c_char,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr, slice,
};

use nes_core::{
    cartridge, controller::ButtonState, ines, Console, ConsoleState, VideoBuffer, VideoOutput,
    FRAME_HEIGHT, FRAME_RGBA_BYTES, FRAME_WIDTH, NES_PALETTE_RGBA,
};

/// Number of bytes in the CPU's internal RAM, mirrored at $0000-$1fff.
pub const NES_RAM_BYTES: usize = 2048;

/// Number of bytes in the persistent RGBA framebuffer view.
pub const NES_FRAMEBUFFER_RGBA_BYTES: usize = FRAME_RGBA_BYTES;

/// C ABI result code. Zero means success; all nonzero values are failures.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NesStatus {
    Ok = 0,
    InvalidArgument = 1,
    InvalidRom = 2,
    UnsupportedMapper = 3,
    AllocationFailure = 4,
    Panic = 5,
}

/// Opaque emulator handle. Do not construct or inspect this type from C.
pub struct NesHandle {
    console: Console,
    ram_view: [u8; NES_RAM_BYTES],
    framebuffer: VideoBuffer,
    audio_view: Vec<f32>,
    video_enabled: bool,
}

/// Opaque point-in-time emulator snapshot handle.
pub struct NesSnapshot {
    state: ConsoleState,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_error(message: impl Into<String>) {
    let message = message.into().replace('\0', "");
    LAST_ERROR.with(|last_error| {
        *last_error.borrow_mut() = CString::new(message).ok();
    });
}

fn clear_error() {
    LAST_ERROR.with(|last_error| {
        *last_error.borrow_mut() = None;
    });
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "Rust panic across nes-ffi boundary".to_owned()
    }
}

fn ffi_status<F>(operation: F) -> NesStatus
where
    F: FnOnce() -> NesStatus,
{
    clear_error();
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(status) => status,
        Err(payload) => {
            set_error(format!("panic: {}", panic_message(payload)));
            NesStatus::Panic
        }
    }
}

fn ffi_ptr<T, F>(operation: F) -> *const T
where
    F: FnOnce() -> *const T,
{
    clear_error();
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(pointer) => pointer,
        Err(payload) => {
            set_error(format!("panic: {}", panic_message(payload)));
            ptr::null()
        }
    }
}

fn invalid_argument(message: &'static str) -> NesStatus {
    set_error(message);
    NesStatus::InvalidArgument
}

unsafe fn mutable_handle<'a>(handle: *mut NesHandle) -> Result<&'a mut NesHandle, NesStatus> {
    handle
        .as_mut()
        .ok_or_else(|| invalid_argument("emulator handle is null"))
}

unsafe fn handle_ref<'a>(handle: *const NesHandle) -> Result<&'a NesHandle, NesStatus> {
    handle
        .as_ref()
        .ok_or_else(|| invalid_argument("emulator handle is null"))
}

/// Return the last error for the calling thread, or null if the last call
/// succeeded. The pointer remains valid until the next nes-ffi call on the
/// same thread.
#[no_mangle]
pub extern "C" fn nes_last_error() -> *const c_char {
    match catch_unwind(AssertUnwindSafe(|| {
        LAST_ERROR.with(|last_error| {
            last_error
                .borrow()
                .as_ref()
                .map_or(ptr::null(), |message| message.as_ptr())
        })
    })) {
        Ok(pointer) => pointer,
        Err(payload) => {
            set_error(format!("panic: {}", panic_message(payload)));
            ptr::null()
        }
    }
}

/// Return the fixed CPU RAM view size in bytes.
#[no_mangle]
pub extern "C" fn nes_ram_view_len() -> usize {
    NES_RAM_BYTES
}

/// Return the fixed RGBA framebuffer view size in bytes.
#[no_mangle]
pub extern "C" fn nes_framebuffer_view_len() -> usize {
    NES_FRAMEBUFFER_RGBA_BYTES
}

/// Return the number of mono f32 samples produced by the most recent frame.
#[no_mangle]
pub unsafe extern "C" fn nes_audio_view_len(handle: *const NesHandle) -> usize {
    match handle_ref(handle) {
        Ok(handle) => handle.audio_view.len(),
        Err(_) => 0,
    }
}

/// Return the fixed audio sample rate used by nes-core.
#[no_mangle]
pub extern "C" fn nes_audio_sample_rate() -> u32 {
    nes_core::AUDIO_SAMPLE_RATE
}

/// Return the most recent frame's borrowed mono f32 audio samples.
#[no_mangle]
pub unsafe extern "C" fn nes_audio_view(handle: *const NesHandle) -> *const f32 {
    match handle_ref(handle) {
        Ok(handle) => handle.audio_view.as_ptr(),
        Err(_) => ptr::null(),
    }
}

/// Create an emulator from an in-memory iNES ROM.
///
/// The newly created emulator is headless by default. Set video output to
/// enabled with [`nes_set_video_output`] when rendered frames are needed.
#[no_mangle]
pub unsafe extern "C" fn nes_create(
    rom_bytes: *const u8,
    rom_len: usize,
    out_handle: *mut *mut NesHandle,
) -> NesStatus {
    ffi_status(|| {
        if out_handle.is_null() {
            return invalid_argument("output emulator handle is null");
        }
        *out_handle = ptr::null_mut();
        if rom_bytes.is_null() && rom_len != 0 {
            return invalid_argument("ROM pointer is null");
        }

        let rom = if rom_len == 0 {
            &[]
        } else {
            // SAFETY: The caller promises that rom_bytes points to rom_len
            // readable bytes; the null/zero case was checked above.
            slice::from_raw_parts(rom_bytes, rom_len)
        };
        let (cartridge, mapper_number) = match ines::load(&mut Cursor::new(rom)) {
            Some(result) => result,
            None => {
                set_error("invalid iNES ROM");
                return NesStatus::InvalidRom;
            }
        };
        let mapper = match cartridge::new(cartridge, mapper_number) {
            Some(mapper) => mapper,
            None => {
                set_error(format!("unsupported NES mapper {mapper_number}"));
                return NesStatus::UnsupportedMapper;
            }
        };

        let mut handle = Box::new(NesHandle {
            console: Console::new(mapper),
            ram_view: [0; NES_RAM_BYTES],
            framebuffer: VideoBuffer::new(),
            audio_view: Vec::with_capacity((nes_core::AUDIO_SAMPLE_RATE / 50) as usize),
            video_enabled: false,
        });
        handle.console.set_video_output(VideoOutput::Disabled);
        handle.refresh_ram_view();
        *out_handle = Box::into_raw(handle);
        NesStatus::Ok
    })
}

/// Destroy an emulator handle. Null is accepted as a no-op.
#[no_mangle]
pub unsafe extern "C" fn nes_destroy(handle: *mut NesHandle) {
    let _ = ffi_status(|| {
        if !handle.is_null() {
            // SAFETY: A non-null handle must have been returned by nes_create
            // and must not have been destroyed already.
            drop(Box::from_raw(handle));
        }
        NesStatus::Ok
    });
}

impl NesHandle {
    fn refresh_ram_view(&mut self) {
        self.console.copy_cpu_ram(&mut self.ram_view);
    }

    fn advance_one_frame(&mut self, controller_state: u8) {
        self.console
            .update_buttons(ButtonState::from_bits(controller_state));
        let frame = self.console.next_frame();
        self.audio_view.clear();
        self.audio_view.extend_from_slice(frame.audio_samples);
        if self.video_enabled {
            frame.copy_to(&mut self.framebuffer);
            overlay_ghosts(&frame, &mut self.framebuffer);
        }
        self.refresh_ram_view();
    }
}

fn blend_channel(destination: u8, source: u8, opacity: f32) -> u8 {
    (source as f32 * opacity + destination as f32 * (1.0 - opacity)).round() as u8
}

fn overlay_ghosts(frame: &nes_core::FrameOutput<'_>, buffer: &mut VideoBuffer) {
    let rgba = buffer.rgba_mut();
    for layer in frame.ghost_layers {
        let opacity = layer.opacity().clamp(0.0, 1.0);
        let Some(ghost_frame) = layer.current_frame() else {
            continue;
        };
        for sprite in ghost_frame.sprites() {
            let x = sprite.x as usize;
            let y = sprite.y as usize;
            if x >= FRAME_WIDTH || y >= FRAME_HEIGHT {
                continue;
            }
            let [red, green, blue, _] = NES_PALETTE_RGBA[sprite.palette_index as usize & 0x3f];
            let offset = (y * FRAME_WIDTH + x) * 4;
            rgba[offset] = blend_channel(rgba[offset], red, opacity);
            rgba[offset + 1] = blend_channel(rgba[offset + 1], green, opacity);
            rgba[offset + 2] = blend_channel(rgba[offset + 2], blue, opacity);
        }
    }
}

/// Advance complete emulator frames, applying one controller state to every
/// frame. The RAM view is refreshed after each completed frame.
#[no_mangle]
pub unsafe extern "C" fn nes_advance_frames(
    handle: *mut NesHandle,
    controller_state: u8,
    frames: u32,
) -> NesStatus {
    ffi_status(|| {
        let handle = match mutable_handle(handle) {
            Ok(handle) => handle,
            Err(status) => return status,
        };
        for _ in 0..frames {
            handle.advance_one_frame(controller_state);
        }
        NesStatus::Ok
    })
}

/// Rewind one completed frame using the emulator's in-memory rewind tape.
/// `out_rewound` is false when the tape has no older frame; that condition is
/// normal for a runtime-enabled RL action and is not an ABI error.
#[no_mangle]
pub unsafe extern "C" fn nes_rewind(handle: *mut NesHandle, out_rewound: *mut bool) -> NesStatus {
    ffi_status(|| {
        if out_rewound.is_null() {
            return invalid_argument("output rewind result is null");
        }
        *out_rewound = false;
        let handle = match mutable_handle(handle) {
            Ok(handle) => handle,
            Err(status) => return status,
        };
        *out_rewound = handle.console.rewind();
        handle.audio_view.clear();
        if *out_rewound {
            // Match the existing interactive frontends: rewind selects the
            // historical state, then next_frame renders that state through
            // the canonical frame boundary. Calling rewind alone leaves the
            // FFI framebuffer stale until a periodic tape checkpoint is
            // crossed, which produces visible snaps in replay video.
            handle.console.update_buttons(ButtonState::default());
            let frame = handle.console.next_frame();
            if handle.video_enabled {
                frame.copy_to(&mut handle.framebuffer);
                overlay_ghosts(&frame, &mut handle.framebuffer);
            }
        } else if handle.console.rewind_exhausted() {
            // Consume Console's oldest-frame hold marker so the next normal
            // FFI advance is never silently discarded.
            let frame = handle.console.next_frame();
            if handle.video_enabled {
                frame.copy_to(&mut handle.framebuffer);
                overlay_ghosts(&frame, &mut handle.framebuffer);
            }
        }
        handle.refresh_ram_view();
        NesStatus::Ok
    })
}

/// Return the stable, read-only 2 KiB CPU RAM view.
#[no_mangle]
pub extern "C" fn nes_ram_view(handle: *const NesHandle) -> *const u8 {
    ffi_ptr(|| match unsafe { handle_ref(handle) } {
        Ok(handle) => handle.ram_view.as_ptr(),
        Err(_) => ptr::null(),
    })
}

/// Return the stable, read-only RGBA framebuffer view.
#[no_mangle]
pub extern "C" fn nes_framebuffer_view(handle: *const NesHandle) -> *const u8 {
    ffi_ptr(|| match unsafe { handle_ref(handle) } {
        Ok(handle) => handle.framebuffer.rgba().as_ptr(),
        Err(_) => ptr::null(),
    })
}

/// Enable or disable final video output. Pass zero to disable and any nonzero
/// value to enable. Disabled output preserves the emulation timeline while
/// avoiding final framebuffer writes in nes-core and this wrapper.
#[no_mangle]
pub unsafe extern "C" fn nes_set_video_output(handle: *mut NesHandle, enabled: u8) -> NesStatus {
    ffi_status(|| {
        let handle = match mutable_handle(handle) {
            Ok(handle) => handle,
            Err(status) => return status,
        };
        handle.video_enabled = enabled != 0;
        handle.console.set_video_output(if handle.video_enabled {
            VideoOutput::Enabled
        } else {
            VideoOutput::Disabled
        });
        NesStatus::Ok
    })
}

/// Clone the deterministic emulator state into a new opaque snapshot handle.
#[no_mangle]
pub unsafe extern "C" fn nes_snapshot(
    handle: *const NesHandle,
    out_snapshot: *mut *mut NesSnapshot,
) -> NesStatus {
    ffi_status(|| {
        if out_snapshot.is_null() {
            return invalid_argument("output snapshot handle is null");
        }
        *out_snapshot = ptr::null_mut();
        let handle = match handle_ref(handle) {
            Ok(handle) => handle,
            Err(status) => return status,
        };
        let snapshot = Box::new(NesSnapshot {
            state: handle.console.snapshot(),
        });
        *out_snapshot = Box::into_raw(snapshot);
        NesStatus::Ok
    })
}

/// Restore an emulator to a snapshot and refresh the RAM view.
#[no_mangle]
pub unsafe extern "C" fn nes_restore(
    handle: *mut NesHandle,
    snapshot: *const NesSnapshot,
) -> NesStatus {
    ffi_status(|| {
        let handle = match mutable_handle(handle) {
            Ok(handle) => handle,
            Err(status) => return status,
        };
        let snapshot = match snapshot.as_ref() {
            Some(snapshot) => snapshot,
            None => return invalid_argument("snapshot handle is null"),
        };
        handle
            .console
            .restore_snapshot_and_reset_timeline(snapshot.state.clone());
        handle.refresh_ram_view();
        NesStatus::Ok
    })
}

/// Destroy a snapshot handle. Null is accepted as a no-op.
#[no_mangle]
pub unsafe extern "C" fn nes_snapshot_destroy(snapshot: *mut NesSnapshot) {
    let _ = ffi_status(|| {
        if !snapshot.is_null() {
            // SAFETY: A non-null snapshot must have been returned by
            // nes_snapshot and must not have been destroyed already.
            drop(Box::from_raw(snapshot));
        }
        NesStatus::Ok
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rom() -> Vec<u8> {
        let mut rom = vec![0; 16 + 0x8000 + 0x2000];
        rom[..4].copy_from_slice(b"NES\x1a");
        rom[4] = 2; // 32 KiB PRG ROM
        rom[5] = 1; // 8 KiB CHR ROM

        // LDA #$00; STA $0000; INC $0000; JMP $8005
        let program = [0xa9, 0x00, 0x85, 0x00, 0xe6, 0x00, 0x4c, 0x05, 0x80];
        let prg_start = 16;
        rom[prg_start..prg_start + program.len()].copy_from_slice(&program);
        // Reset vector at $fffc in the CPU address space, the last six bytes
        // of the 32 KiB PRG image for a 16 KiB-aligned NROM image.
        rom[prg_start + 0x7ffc] = 0x00;
        rom[prg_start + 0x7ffd] = 0x80;
        rom
    }

    unsafe fn create_test_handle(rom: &[u8]) -> *mut NesHandle {
        let mut handle = ptr::null_mut();
        assert_eq!(
            nes_create(rom.as_ptr(), rom.len(), &mut handle),
            NesStatus::Ok
        );
        assert!(!handle.is_null());
        handle
    }

    #[test]
    fn ram_pointer_is_stable_across_frames_and_restore() {
        let rom = test_rom();
        unsafe {
            let handle = create_test_handle(&rom);
            let pointer = nes_ram_view(handle);
            assert!(!pointer.is_null());
            assert_eq!(nes_advance_frames(handle, 0, 3), NesStatus::Ok);
            assert_eq!(nes_ram_view(handle), pointer);

            let mut snapshot = ptr::null_mut();
            assert_eq!(nes_snapshot(handle, &mut snapshot), NesStatus::Ok);
            assert_eq!(nes_advance_frames(handle, 0, 4), NesStatus::Ok);
            assert_eq!(nes_restore(handle, snapshot), NesStatus::Ok);
            assert_eq!(nes_ram_view(handle), pointer);
            nes_snapshot_destroy(snapshot);
            nes_destroy(handle);
        }
    }

    #[test]
    fn snapshot_restore_replays_identical_ram() {
        let rom = test_rom();
        unsafe {
            let handle = create_test_handle(&rom);
            let mut snapshot = ptr::null_mut();
            assert_eq!(nes_snapshot(handle, &mut snapshot), NesStatus::Ok);

            assert_eq!(nes_advance_frames(handle, 0x81, 8), NesStatus::Ok);
            let first = slice::from_raw_parts(nes_ram_view(handle), NES_RAM_BYTES).to_vec();

            assert_eq!(nes_restore(handle, snapshot), NesStatus::Ok);
            assert_eq!(nes_advance_frames(handle, 0x81, 8), NesStatus::Ok);
            let second = slice::from_raw_parts(nes_ram_view(handle), NES_RAM_BYTES).to_vec();
            assert_eq!(first, second);

            nes_snapshot_destroy(snapshot);
            nes_destroy(handle);
        }
    }

    #[test]
    fn rewind_restores_previous_ram_without_unwinding() {
        let rom = test_rom();
        unsafe {
            let handle = create_test_handle(&rom);
            assert_eq!(nes_advance_frames(handle, 0x81, 3), NesStatus::Ok);
            let mut rewound = false;
            assert_eq!(nes_rewind(handle, &mut rewound), NesStatus::Ok);
            assert!(rewound);
            assert_eq!(nes_rewind(handle, &mut rewound), NesStatus::Ok);
            assert!(rewound);
            assert_eq!(nes_rewind(handle, &mut rewound), NesStatus::Ok);
            assert!(rewound);
            assert_eq!(nes_rewind(handle, &mut rewound), NesStatus::Ok);
            assert!(!rewound);
            nes_destroy(handle);
        }
    }

    #[test]
    fn invalid_arguments_are_reported_without_unwinding() {
        unsafe {
            assert_eq!(
                nes_advance_frames(ptr::null_mut(), 0, 1),
                NesStatus::InvalidArgument
            );
            assert!(!nes_last_error().is_null());
            assert_eq!(nes_ram_view(ptr::null()), ptr::null());
            assert!(!nes_last_error().is_null());
        }
    }
}

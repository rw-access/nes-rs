use std::collections::VecDeque;

use crate::{console::ConsoleState, controller::ButtonState, ppu::Screen};

#[derive(Clone)]
struct ButtonSequence {
    buttons: ButtonState,
    count: u8,
}

#[derive(Clone)]
struct Checkpoint {
    base_state: ConsoleState,
    buttons_rle: VecDeque<ButtonSequence>,
}

/// A self-compressing tape of snapshots, that efficiently tracks all historical states
/// for the NES by tracking full state at periodic intervals and run length encoded button
/// presses between full state snapshots. Benefits are that memory usage is O(√Time) snapshots
/// O(Time) button presses, with a single frame rewind of O(1).
///
/// Memory layout:
/// *-+--+---+----+-----+------+-------+--------+---------+---------> time
///   |  |   |    |     |      |       |        |||       ||||||||
///   |  |   |    |     |      |       |        |         snapshots
///   |  |   |    |     |      |       |        |         
///   |  \....................................... snapshot + RLE buttons
///   \-- snapshot + RLE buttons
pub(crate) struct RewindTape {
    stored_checkpoints: Vec<Checkpoint>,
    previous_checkpoint: (Vec<(ConsoleState, ButtonState)>, VecDeque<ButtonSequence>),
    snapshot_cache: Vec<(ConsoleState, ButtonState)>,
    cache_size: usize,
    frames: usize,
}

impl RewindTape {
    pub(crate) fn new(initial_step: usize) -> Self {
        RewindTape {
            stored_checkpoints: Vec::new(),
            previous_checkpoint: (Vec::new(), VecDeque::new()),
            snapshot_cache: Vec::with_capacity(initial_step),
            cache_size: initial_step,
            frames: 0,
        }
    }

    /// Push a snapshot onto the tape, compressing full snapshots into the more compressed Checkpoint
    pub(crate) fn push_back(&mut self, state: ConsoleState) {
        // Pack the previous checkpoint first RLE.
        // There are always `cache_size` full snapshots loaded,
        // between the previous, (partially) decoded checkpoing and the pending checkpoint
        //
        //        prev     curr
        // *----+--------|-------->
        //      |||------|||||||||
        //      |||^^^   ||\- snapshot
        //      |||^^^   |\-- snapshot
        //      |||^^^   \--- snapshot
        //      |||^^^
        //      |||^^^ packed RLE buttons
        //      ||\--- unpacked snapshot
        //      |\---- unpacked snapshot
        //      \----- unpacked snapspshot
        //
        let buttons = state.bus.controller.button_state;
        let (decoded_snapshots, buttons_rle) = &mut self.previous_checkpoint;

        // Move data further "left", first storing a snapshot if one is already fully encoded
        if let Some((base_state, next_buttons)) = decoded_snapshots.pop() {
            // no snapshots left to convert to RLE button presses, ready to store
            if decoded_snapshots.is_empty() {
                let mut stored_rle = buttons_rle.split_off(0);
                stored_rle.shrink_to_fit();
                self.stored_checkpoints.push(Checkpoint {
                    base_state,
                    buttons_rle: stored_rle,
                });
                println!(
                    "stored {} frames, {} checkpoints, RLE : cap = {}/len = {}, reserved capacity = {} B, size of checkpoints = {}",
                    self.frames,
                    self.stored_checkpoints.len(),
                    self.stored_checkpoints.last().unwrap().buttons_rle.capacity(),
                    self.stored_checkpoints.last().unwrap().buttons_rle.len(),
                    self.stored_checkpoints.capacity() * std::mem::size_of::<Checkpoint>(),
                    std::mem::size_of_val(&self.stored_checkpoints[..]),
                );
            } else {
                // convert another expanded snapshot to an RLE button press
                // pack the buton onto the current sequence, preserving and building RLE
                match buttons_rle.front_mut() {
                    Some(next_buttons_rle)
                        if next_buttons_rle.buttons == next_buttons
                            && next_buttons_rle.count < u8::MAX =>
                    {
                        next_buttons_rle.count += 1;
                    }
                    _ => buttons_rle.push_front(ButtonSequence {
                        buttons: next_buttons,
                        count: 1,
                    }),
                };
            }
        }

        // Add a frame to the current full snapshot cache, pushing it back to be encoded as RLE when full
        if self.snapshot_cache.len() < self.cache_size {
            self.snapshot_cache.push((state, buttons));
        } else {
            // The previous checkpoint is empty
            // Avoid wasted allocations by keeping existing allocated buffers intact
            std::mem::swap(&mut self.previous_checkpoint.0, &mut self.snapshot_cache);
            self.previous_checkpoint.1.truncate(0);

            self.cache_size += 1;

            // Extend the buffers as necessary and initialize with a single (snapshot, buttons)
            self.snapshot_cache.truncate(0);
            self.snapshot_cache.reserve(self.cache_size);
            self.snapshot_cache.push((state, buttons));
        }

        self.frames += 1;
    }

    /// Put the next stored checkpoint into the one-frame-ahead decoder.
    ///
    /// The checkpoint is intentionally not expanded here.  Its base state and
    /// RLE stream become the previous checkpoint, and `pop_back` advances that
    /// decoder by one frame for each frame it removes from the current cache.
    fn prepare_stored_checkpoint(&mut self) -> bool {
        let Some(mut checkpoint) = self.stored_checkpoints.pop() else {
            return false;
        };

        let buttons = checkpoint.base_state.bus.controller.button_state;
        let (decoded_snapshots, buttons_rle) = &mut self.previous_checkpoint;
        decoded_snapshots.clear();
        buttons_rle.clear();
        decoded_snapshots.push((checkpoint.base_state, buttons));
        std::mem::swap(buttons_rle, &mut checkpoint.buttons_rle);

        true
    }

    fn decode_one_previous_frame(&mut self, screen: &mut Screen) {
        let (decoded_snapshots, buttons_rle) = &mut self.previous_checkpoint;
        let (Some((prev_state, _)), Some(next_buttons)) =
            (decoded_snapshots.last(), buttons_rle.front_mut())
        else {
            return;
        };

        let mut next_state = prev_state.clone();
        next_state
            .bus
            .controller
            .update_buttons(next_buttons.buttons);
        next_state.wait_vblank(screen, |_| {});
        next_state.advance_frame_number();
        decoded_snapshots.push((next_state, next_buttons.buttons));

        if next_buttons.count > 1 {
            next_buttons.count -= 1;
        } else {
            buttons_rle.pop_front();
        }
    }

    /// Pop the most recent snapshot from the end of the tape, using NES frame
    /// evaluation to expand RLE buttons as needed.
    pub(crate) fn pop_back(&mut self, screen: &mut Screen) -> Option<ConsoleState> {
        // Refill the cache before trying to pop. Previously this happened only
        // after `pop()` had already returned None, making stored checkpoints
        // unreachable at exactly the boundary where they were needed.
        if self.snapshot_cache.is_empty() {
            if !self.previous_checkpoint.0.is_empty() {
                // The previous checkpoint should already be fully decoded by
                // the one-frame-ahead work below. Finish any remaining RLE
                // here as a correctness guard rather than dropping frames at
                // the boundary.
                while !self.previous_checkpoint.1.is_empty() {
                    self.decode_one_previous_frame(screen);
                }

                std::mem::swap(&mut self.previous_checkpoint.0, &mut self.snapshot_cache);
                self.previous_checkpoint.1.truncate(0);

                self.cache_size = self.cache_size.saturating_sub(1);

                self.previous_checkpoint.0.truncate(0);
            } else if !self.prepare_stored_checkpoint() {
                return None;
            }
        }

        let (latest_snapshot, _) = self.snapshot_cache.pop()?;

        // When the previous checkpoint has been fully consumed, begin the next
        // one before decoding. This keeps its expansion overlapped with the
        // frames being popped instead of doing a whole-checkpoint replay at the
        // boundary.
        if self.previous_checkpoint.0.is_empty() && self.previous_checkpoint.1.is_empty() {
            self.prepare_stored_checkpoint();
        }
        self.decode_one_previous_frame(screen);

        self.frames -= 1;
        Some(latest_snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::{RewindTape, Screen};
    use crate::{
        cartridge::{Mapper, MirroringMode},
        console::Console,
        controller::ButtonState,
    };

    #[derive(Clone)]
    struct TestMapper;

    impl Mapper for TestMapper {
        fn mirror(&self) -> MirroringMode {
            MirroringMode::Horizontal
        }

        fn read(&self, _address: u16) -> u8 {
            0xea
        }

        fn write(&mut self, _address: u16, _data: u8) {}

        fn read_page(&self, _page: u8) -> Option<&[u8; 256]> {
            None
        }
    }

    #[test]
    fn rewind_tape_crosses_checkpoints_without_extra_frames() {
        let mut source = Console::new(Box::new(TestMapper));
        let mut states = Vec::new();
        let mut expected_buttons = Vec::new();
        let mut expected_frame_numbers = Vec::new();
        for frame in 0..24 {
            let buttons = if frame % 3 == 0 { 0x01 } else { 0x00 };
            source.update_buttons(ButtonState::from_bits(buttons));
            let _ = source.next_frame();
            states.push(source.snapshot());
            expected_buttons.push(buttons);
            expected_frame_numbers.push(source.snapshot().frame_number());
        }

        let expected_frames = states.len();
        let mut tape = RewindTape::new(1);
        for state in states {
            tape.push_back(state);
        }

        let mut screen = Screen::default();
        let mut popped_frames = 0;
        let mut popped_buttons = Vec::new();
        let mut popped_frame_numbers = Vec::new();
        while let Some(state) = tape.pop_back(&mut screen) {
            popped_frames += 1;
            popped_buttons.push(state.bus.controller.button_state.bits());
            popped_frame_numbers.push(state.frame_number());
            assert!(
                popped_frames <= expected_frames,
                "RLE decoder produced an extra rewind frame"
            );
        }

        assert_eq!(popped_frames, expected_frames);
        expected_buttons.reverse();
        assert_eq!(popped_buttons, expected_buttons);
        expected_frame_numbers.reverse();
        assert_eq!(popped_frame_numbers, expected_frame_numbers);
    }
}

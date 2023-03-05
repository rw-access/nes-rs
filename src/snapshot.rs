use std::collections::VecDeque;

use crate::{
    bus::MemoryBus, console::ConsoleState, controller::ButtonState, cpu::CPU, ppu::Screen,
};

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

    /// Pop the most recent Snapshot from the end of the tape, using one NES frame evaluation
    /// to expand out RLE buttons to the next snapshot
    pub(crate) fn pop_back(&mut self, screen: &mut Screen) -> Option<ConsoleState> {
        let (latest_snapshot, _) = self.snapshot_cache.pop()?;
        let (decoded_snapshots, buttons_rle) = &mut self.previous_checkpoint;

        // Pull the decoded previous checkpoint into the cache, if the snapshot cache is empty
        if self.snapshot_cache.is_empty() && !decoded_snapshots.is_empty() {
            // The previous checkpoint contains fully decoded snapshots
            // Avoid wasted allocations by keeping existing allocated buffers intact
            std::mem::swap(decoded_snapshots, &mut self.snapshot_cache);
            buttons_rle.truncate(0);

            self.cache_size -= 1;

            // Extend the buffers as necessary and initialize with a single (snapshot, buttons)
            decoded_snapshots.truncate(0);
        }

        // Move data further "right", restoring one when the current checkpoint is fully emptied
        // Decompress RLE and evaluate a frame
        match (decoded_snapshots.last(), buttons_rle.front_mut()) {
            (Some((prev_state, _)), Some(next_buttons)) => {
                // convert another expanded snapshot to an RLE button press
                // pack the buton onto the current sequence, preserving and building RLE
                let mut next_state = prev_state.clone();
                next_state
                    .bus
                    .controller
                    .update_buttons(next_buttons.buttons);

                next_state.wait_vblank(screen);
                decoded_snapshots.push((next_state, next_buttons.buttons));

                if next_buttons.count > 0 {
                    next_buttons.count -= 1;
                } else {
                    buttons_rle.pop_front();
                }
            }
            _ => {
                if let Some(mut checkpoint) = self.stored_checkpoints.pop() {
                    let buttons = checkpoint.base_state.bus.controller.button_state;
                    decoded_snapshots.truncate(0);
                    std::mem::swap(buttons_rle, &mut checkpoint.buttons_rle);
                    decoded_snapshots.push((checkpoint.base_state, buttons));
                }
            }
        }

        self.frames -= 1;
        Some(latest_snapshot)
    }
}

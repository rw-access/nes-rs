use std::collections::VecDeque;

use crate::{bus::MemoryBus, cpu::CPU};

pub struct Snapshot {
    pub(crate) bus: MemoryBus,
    pub(crate) cpu: CPU,
}

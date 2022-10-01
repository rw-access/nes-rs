use crate::{bus::MemoryBus, cpu::CPU};

pub(crate) struct Console {
    pub(crate) bus: MemoryBus,
    pub(crate) cpu: CPU,
}
use crate::cartridge::Mapper;

pub(crate) struct MemoryBus {
    pub(crate) mapper: Box<dyn Mapper>,
    pub(crate) ram: [u8; 0x800],
}

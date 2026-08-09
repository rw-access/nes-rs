use crate::apu::APU;
use crate::cartridge::MapperInstance;
use crate::controller::Controller;
use crate::ppu::PPU;

#[derive(Clone)]
pub(crate) struct MemoryBus {
    pub(crate) mapper: MapperInstance,
    pub(crate) ppu: PPU,
    pub(crate) apu: APU,
    pub(crate) controller: Controller,
}

impl MemoryBus {
    pub(crate) fn new(mapper: MapperInstance) -> Self {
        MemoryBus {
            mapper,
            ppu: PPU::default(),
            apu: APU::default(),
            controller: Controller::default(),
        }
    }
}

use crate::apu::APU;
use crate::cartridge::Mapper;
use crate::controller::Controller;
use crate::ppu::PPU;

#[derive(Clone)]
pub(crate) struct MemoryBus {
    pub(crate) mapper: Box<dyn Mapper>,
    pub(crate) ppu: PPU,
    pub(crate) apu: APU,
    pub(crate) controller: Controller,
}

impl MemoryBus {
    fn new(mapper: Box<dyn Mapper>) -> Self {
        MemoryBus {
            mapper: mapper,
            ppu: PPU::default(),
            apu: APU::default(),
            controller: Controller::default(),
        }
    }
}

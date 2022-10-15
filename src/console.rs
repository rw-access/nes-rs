use crate::{
    apu::APU,
    bus::MemoryBus,
    cartridge::Mapper,
    controller::{Button, Controller},
    cpu::CPU,
    ppu::{Screen, PPU},
};

pub struct Console {
    pub(crate) bus: MemoryBus,
    pub(crate) cpu: CPU,
    pub(crate) latest_screen: Screen,
}

impl Console {
    pub fn update_buttons(&mut self, button: Button, pressed: bool) {
        self.bus.controller.update_button(button, pressed);
    }

    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        let mut console = Console {
            bus: MemoryBus {
                mapper,
                ppu: PPU::default(),
                apu: APU::default(),
                controller: Controller::default(),
            },
            cpu: CPU::default(),
            latest_screen: Screen::default(),
        };

        console.bus.ppu.reset();
        console.cpu.reset(&mut console.bus);
        console
    }

    fn step(&mut self) {
        let cycles = self.cpu.step(&mut self.bus, None); // Some(&mut stdout()));
        for _ in 0..cycles {
            self.bus.ppu.step(self.bus.mapper.as_mut());
            self.bus.ppu.step(self.bus.mapper.as_mut());
            self.bus.ppu.step(self.bus.mapper.as_mut());
        }
    }

    pub fn wait_vblank(&mut self) {
        // only return on a positive edge
        while self.bus.ppu.in_vblank {
            self.step();
        }

        while !self.bus.ppu.in_vblank {
            self.step();
        }

        self.latest_screen = self.bus.ppu.pending_screen.clone();
    }

    pub fn screen(&self) -> &Screen {
        &self.latest_screen
    }
}

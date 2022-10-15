use crate::{
    apu::APU,
    bus::MemoryBus,
    cartridge::Mapper,
    controller::{Button, Controller},
    cpu::CPU,
    ppu::{Screen, PPU},
    snapshot::Snapshot,
};

pub struct Console {
    pub(crate) bus: MemoryBus,
    pub(crate) cpu: CPU,
    pub(crate) screens: [Screen; 2],
    pub(crate) latest_screen: bool,
}

impl Console {
    pub fn take_snapshot(&self) -> Snapshot {
        Snapshot {
            bus: self.bus.clone(),
            cpu: self.cpu.clone(),
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: Snapshot) {
        self.bus = snapshot.bus;
        self.cpu = snapshot.cpu;
    }

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
            latest_screen: false,
            screens: [Screen::default(), Screen::default()],
        };

        console.bus.ppu.reset();
        console.cpu.reset(&mut console.bus);
        console
    }

    fn step(&mut self) {
        let cycles = self.cpu.step(&mut self.bus, None); // Some(&mut stdout()));
        let pending_screen = &mut self.screens[(!self.latest_screen) as usize];
        for _ in 0..cycles {
            self.bus.ppu.step(self.bus.mapper.as_mut(), pending_screen);
            self.bus.ppu.step(self.bus.mapper.as_mut(), pending_screen);
            self.bus.ppu.step(self.bus.mapper.as_mut(), pending_screen);
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

        self.latest_screen = !self.latest_screen;
    }

    pub fn screen(&self) -> &Screen {
        &self.screens[self.latest_screen as usize]
    }
}

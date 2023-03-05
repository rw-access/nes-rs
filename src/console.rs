use crate::{
    apu::APU,
    bus::MemoryBus,
    cartridge::Mapper,
    controller::{Button, ButtonState, Controller},
    cpu::CPU,
    ppu::{Screen, PPU},
    snapshot::RewindTape,
};

#[derive(Clone)]
pub struct ConsoleState {
    pub(crate) bus: MemoryBus,
    pub(crate) cpu: CPU,
}

impl ConsoleState {
    fn step(&mut self, screen: &mut Screen) {
        let cycles = self.cpu.step(&mut self.bus, None); // Some(&mut stdout()));
        for _ in 0..cycles {
            for _ in 0..3 {
                self.bus.ppu.step(self.bus.mapper.as_mut(), screen);
            }
        }
    }

    pub(crate) fn wait_vblank(&mut self, screen: &mut Screen) {
        // only return on a positive edge
        while self.bus.ppu.in_vblank {
            self.step(screen);
        }

        while !self.bus.ppu.in_vblank {
            self.step(screen);
        }
    }
}

pub struct Console {
    state: ConsoleState,
    tape: RewindTape,
    screen: Screen,
    in_rewind: bool,
}

impl Console {
    pub fn snapshot(&self) -> ConsoleState {
        self.state.clone()
    }

    pub fn restore_snapshot(
        &mut self,
        snapshot: ConsoleState,
        cpu_ignore: &Vec<u16>,
        ppu_ignore: &Vec<u16>,
    ) {
        // read preserved addresses
        let cpu_backup_contents: Vec<u8> = cpu_ignore
            .iter()
            .map(|addr| self.state.cpu.read_byte(&self.state.bus, *addr))
            .collect();
        let ppu_backup_contents: Vec<u8> = ppu_ignore
            .iter()
            .map(|addr| {
                self.state
                    .bus
                    .ppu
                    .read_byte(self.state.bus.mapper.as_ref(), *addr)
            })
            .collect();

        self.state = snapshot;

        // restore preserved addresses
        cpu_ignore
            .iter()
            .zip(cpu_backup_contents)
            .for_each(|(addr, data)| {
                self.state.cpu.write_byte(&mut self.state.bus, *addr, data);
            });
        ppu_ignore
            .iter()
            .zip(ppu_backup_contents)
            .for_each(|(addr, data)| {
                self.state
                    .bus
                    .ppu
                    .write_byte(self.state.bus.mapper.as_mut(), *addr, data);
            });
    }

    pub fn rewind(&mut self) {
        if let Some(prev_state) = self.tape.pop_back(&mut self.screen) {
            self.state = prev_state;
            self.in_rewind = true;
        }
    }

    pub fn update_buttons(&mut self, state: ButtonState) {
        self.state.bus.controller.update_buttons(state);
    }

    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        const INITIAL_TAPE_STEP: usize = 60; // 1 second buffered

        let mut console = Console {
            state: ConsoleState {
                bus: MemoryBus {
                    mapper,
                    ppu: PPU::default(),
                    apu: APU::default(),
                    controller: Controller::default(),
                },
                cpu: CPU::default(),
            },
            screen: Screen::default(),
            tape: RewindTape::new(INITIAL_TAPE_STEP),
            in_rewind: false,
        };

        console.state.bus.ppu.reset();
        console.state.cpu.reset(&mut console.state.bus);
        console
    }

    pub fn next_screen(&mut self) -> &Screen {
        self.state.wait_vblank(&mut self.screen);

        if !self.in_rewind {
            self.tape.push_back(self.state.clone());
        }

        self.in_rewind = false;
        &self.screen
    }
}

use std::cell::Cell;

pub enum Button {
    A = 0,
    B = 1,
    Select = 2,
    Start = 3,
    Up = 4,
    Down = 5,
    Left = 6,
    Right = 7,
}

#[derive(Clone, Default)]
pub(crate) struct Controller {
    pub(crate) button_state: u8,
    strobe: bool,

    index: Cell<u8>,
}

impl Controller {
    pub(crate) fn update_button(&mut self, button: Button, pressed: bool) {
        let mask: u8 = 1 << button as u8;
        let new_bits: u8 = (pressed as u8) << button as u8;

        self.button_state = (self.button_state & !mask) | new_bits;
    }

    pub(crate) fn read(&self) -> u8 {
        // https://www.nesdev.org/wiki/Standard_controller
        // Each read reports one bit at a time through D0. The first 8 reads will indicate which buttons
        // or directions are pressed (1 if pressed, 0 if not pressed). All subsequent reads will return 1 on official
        // Nintendo brand controllers but may return 0 on third party controllers such as the U-Force.
        let mut result: u8 = 0;
        let index = self.index.get();

        if index < 8 {
            result = (self.button_state >> index) & 1;
            self.index.set(if !self.strobe { index + 1 } else { index });
        }

        result
    }

    pub(crate) fn write(&mut self, data: u8) {
        // https://www.nesdev.org/wiki/Standard_controller
        // 7  bit  0
        // ---- ----
        // xxxx xxxS
        //         |
        //         +- Controller shift register strobe
        self.strobe = (data & 1) == 1;

        if self.strobe {
            self.index.set(0);
        }
    }
}

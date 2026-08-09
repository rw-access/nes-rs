use std::cell::Cell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ButtonState(pub u8);

impl ButtonState {
    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub fn set(&mut self, button: Button) {
        self.0 |= 1 << (button as u8);
    }

    pub fn unset(&mut self, button: Button) {
        self.0 &= !(1 << (button as u8));
    }
}

#[derive(Clone, Default)]
pub(crate) struct Controller {
    pub(crate) button_state: ButtonState,
    strobe: bool,

    index: Cell<u8>,
}

impl Controller {
    pub(crate) fn update_buttons(&mut self, state: ButtonState) {
        self.button_state = state;
    }

    pub(crate) fn read(&self) -> u8 {
        // https://www.nesdev.org/wiki/Standard_controller
        // Each read reports one bit at a time through D0. The first 8 reads will indicate which buttons
        // or directions are pressed (1 if pressed, 0 if not pressed). All subsequent reads will return 1 on official
        // Nintendo brand controllers but may return 0 on third party controllers such as the U-Force.
        let mut result: u8 = 0;
        let index = self.index.get();

        if index < 8 {
            result = (self.button_state.0 >> index) & 1;
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

#[cfg(test)]
mod tests {
    use super::ButtonState;

    #[test]
    fn button_state_exposes_a_stable_bitmask_boundary() {
        assert_eq!(ButtonState::from_bits(0xa5).bits(), 0xa5);
    }
}

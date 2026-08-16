pub(crate) use nes_isa::CPU_OPCODES as EXTENDED_OPCODES;
pub(crate) use nes_isa::{AddressingMode, CpuOpcodeMetadata as ExtendedOpcode, Opcode};

#[derive(Clone, Debug)]
pub(crate) enum AddressInfo {
    Implied,
    Accumulator,
    Absolute {
        address: u16,
    },
    AbsoluteIndexedX {
        indirect: u16,
        address: u16,
    },
    AbsoluteIndexedY {
        indirect: u16,
        address: u16,
    },
    Immediate {
        address: u16,
    },
    IndexedIndirect {
        offset: u8,
        indirect: u16,
        address: u16,
    },
    Indirect {
        indirect: u16,
        address: u16,
    },
    IndirectIndexed {
        offset: u8,
        indirect: u16,
        address: u16,
    },
    Relative {
        offset: u8,
        address: u16,
    },
    ZeroPage {
        address: u8,
    },
    ZeroPageIndexedX {
        offset: u8,
        address: u16,
    },
    ZeroPageIndexedY {
        offset: u8,
        address: u16,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedInstruction {
    pub(crate) extended_opcode: &'static ExtendedOpcode,
    pub(crate) address_info: AddressInfo,
    pub(crate) final_address: Option<u16>,
    pub(crate) width: u8,
    pub(crate) page_boundary_hit: bool,
}

use crate::bus::MemoryBus;
use crate::instructions::DecodedInstruction;

use super::instructions;

#[derive(Clone, Debug, Default)]
pub(crate) struct CPU {
    cycles: u64,
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    status: u8,
    sp: u8,
}

fn crosses_page_boundary(a: u16, b: u16) -> bool {
    (a & 0xff00) != (b & 0xff00)
}

impl CPU {
    pub(crate) fn reset(&mut self, bus: &mut MemoryBus) {
        // https://www.nesdev.org/wiki/CPU_ALL#At_power-up
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xfd;
        self.status = 0; // TODO: set U+I
        self.pc = self.read_address(bus, 0xfffc);
        
        // TODO: disable frame IRQ, disable all audio, clear IO registers
        // 0x4000 -> 0x4013, 0x4015, 0x4017
    }

    pub(crate) fn step(&mut self, bus: &mut MemoryBus, log: Option<&mut dyn std::io::Write>) -> u16 {
        let pre_cycles = self.cycles;

        // decode the instrucation @ PC
        let instr = self.decode(bus, self.pc);

        if let Some(writer) = log {
            self.debug_instruction(bus, writer, &instr);
            writer.write(b"\n").unwrap();
        }


        self.pc += instr.width as u16;

        // TODO: dispatch the instruction

        self.cycles += (instr.extended_opcode.min_cycles as u64) + (if instr.page_boundary_hit { 1 } else {0});

        
        (self.cycles - pre_cycles) as u16
    }
    
    fn read_byte(&self, bus: &MemoryBus, addr: u16) -> u8 {
        // https://www.nesdev.org/wiki/CPU_memory_map
        if addr < 0x2000 {
            bus.ram[addr as usize % bus.ram.len()]
        } else if addr < 0x4000 {
            // ppu register read
            0
        } else if addr == 0x4016 {
            // controller read
            0
        } else if addr < 0x4018 {
            // APU read
            0
        } else if addr < 0x401f {
            // CPU test mode
            0
        } else {
            // mapper read
            bus.mapper.read(addr)
        }
    }

    fn read_address(&self, bus: &MemoryBus, addr: u16) -> u16 {
        let lo = self.read_byte(bus, addr);
        let hi = self.read_byte(bus, addr + 1);

        (lo as u16) << 8 | (hi as u16)
    }

    fn read_address_indirect(&self, bus: &MemoryBus, addr: u16) -> u16 {
        let lo = self.read_byte(bus, addr);
        let hi = self.read_byte(bus, (addr & 0xff00) | (0x00ff & (addr + 1)));

        (lo as u16) << 8 | (hi as u16)
    }

    fn write_byte(&self, bus: &mut MemoryBus, addr: u16, data: u8) {
        // https://www.nesdev.org/wiki/CPU_memory_map
        if addr < 0x2000 {
            bus.ram[addr as usize % bus.ram.len()] = data;
        } else if addr < 0x4000 {
            // ppu register write
        } else if addr == 0x4016 {
            // controller write
        } else if addr < 0x4018 {
            // APU write
        } else if addr < 0x401f {
            // CPU test mode
        } else {
            // mapper write
            bus.mapper.write(addr, data);

        }
    }

    fn decode(&self, bus: &MemoryBus, addr: u16) -> DecodedInstruction {
        let opcode = self.read_byte(bus, addr);
        let extended_opcode = &instructions::EXTENDED_OPCODES[opcode as usize];

        match extended_opcode.addressing_mode {
            instructions::AddressingMode::Absolute => {
                let address = self.read_address(bus, addr + 1);
                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::Absolute { address: address },
                    width: 3,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::Implied => DecodedInstruction {
                extended_opcode,
                address_info: instructions::AddressInfo::Implied,
                width: 1,
                page_boundary_hit: false,
                final_address: None,
            },
            instructions::AddressingMode::Accumulator => DecodedInstruction {
                extended_opcode,
                address_info: instructions::AddressInfo::Accumulator,
                width: 1,
                page_boundary_hit: false,
                final_address: None,
            },
            instructions::AddressingMode::AbsoluteIndexedX => {
                let indirect = self.read_address(bus, addr + 1);
                let address = self.read_address(bus, indirect + (self.x as u16));

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::AbsoluteIndexedX { indirect, address },
                    width: 3,
                    page_boundary_hit: extended_opcode.page_boundary_penalty
                        && crosses_page_boundary(indirect, address),
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::AbsoluteIndexedY => {
                let indirect = self.read_address(bus, addr + 1);
                let address = self.read_address(bus, indirect + (self.y as u16));

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::AbsoluteIndexedY { indirect, address },
                    width: 3,
                    page_boundary_hit: extended_opcode.page_boundary_penalty
                        && crosses_page_boundary(indirect, address),
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::Immediate => {
                let address = addr + 1;
                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::Immediate { address },
                    width: 3,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::IndexedIndirect => {
                let offset = self.read_byte(bus, addr + 1);
                let indirect = (offset + self.x) as u16;
                let address = self.read_address_indirect(bus, indirect);

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::IndexedIndirect {
                        offset,
                        indirect,
                        address,
                    },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::Indirect => {
                let indirect = self.read_address(bus, addr + 1);
                let address = self.read_address_indirect(bus, indirect);

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::Indirect { indirect, address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::IndirectIndexed => {
                let offset = self.read_byte(bus, addr + 1);
                let indirect = self.read_address(bus, offset as u16);
                let address = indirect + (self.y as u16);

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::IndirectIndexed {
                        offset,
                        indirect,
                        address,
                    },
                    width: 2,
                    page_boundary_hit: extended_opcode.page_boundary_penalty
                        && crosses_page_boundary(indirect, address),
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::Relative => {
                let offset = self.read_byte(bus, addr + 1);
                let address = (((addr + 2) as i16) + ((self.y as i8) as i16)) as u16;

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::Relative { offset, address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::ZeroPage => {
                let address = self.read_byte(bus, addr + 1);
                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::ZeroPage { address: address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address as u16),
                }
            }
            instructions::AddressingMode::ZeroPageIndexedX => {
                let offset = self.read_byte(bus, addr + 1);
                let address = (offset + self.x) as u16;

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::ZeroPageIndexedX { offset, address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            instructions::AddressingMode::ZeroPageIndexedY => {
                let offset = self.read_byte(bus, addr + 1);
                let address = (offset + self.y) as u16;

                DecodedInstruction {
                    extended_opcode,
                    address_info: instructions::AddressInfo::ZeroPageIndexedY { offset, address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
        }
    }

    fn debug_instruction(&self, bus: &MemoryBus, writer: &mut dyn std::io::Write, decoded: &DecodedInstruction) {
        // C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD CYC:7
        // PC    < raw >   < assembly >                    < registers >             < timing >

        // alocate a string on the stack, because it's fixed size and we can keep track of the position information
        // as it grows. once complete, there's a single copy to the writer
        use std::fmt::Write;
        let mut str_buf = arrayvec::ArrayString::<120>::new();

        write!(str_buf, "{:04X} ", self.pc).unwrap();

        for offset in 0..3 {
            if offset < decoded.width {
                write!(str_buf, "{:02X} ", self.read_byte(bus, self.pc + offset as u16)).unwrap();
            } else {
                write!(str_buf, "   ").unwrap();
            }
        }

        write!(str_buf, " {:?} ", decoded.extended_opcode.opcode).unwrap();

        match decoded.address_info {
            instructions::AddressInfo::Implied => Ok(()),
            instructions::AddressInfo::Accumulator => write!(str_buf, "A"),
            instructions::AddressInfo::Absolute { address } => {
                match decoded.extended_opcode.opcode {
                    instructions::Opcode::JSR | instructions::Opcode::JMP => {
                        write!(str_buf, "${:04X}", address)
                    }
                    _ => write!(
                        str_buf,
                        "${:04X} = {:02X}",
                        address,
                        self.read_byte(bus, address)
                    ),
                }
            }
            instructions::AddressInfo::AbsoluteIndexedX { indirect, address } => {
                write!(
                    str_buf,
                    "${:04X},X @ {:04X} = {:02X}",
                    indirect,
                    address,
                    self.read_byte(bus, address)
                )
            }
            instructions::AddressInfo::AbsoluteIndexedY { indirect, address } => {
                write!(
                    str_buf,
                    "${:04X},Y @ {:04X} = {:02X}",
                    indirect,
                    address,
                    self.read_byte(bus, address)
                )
            }
            instructions::AddressInfo::Immediate { address } => {
                write!(str_buf, "#${:02X}", self.read_byte(bus, address))
            }
            instructions::AddressInfo::IndexedIndirect {
                offset,
                indirect,
                address,
            } => write!(
                str_buf,
                "(${:02X},X) @ {:02X} = {:04X} = {:02X}",
                offset,
                indirect,
                address,
                self.read_byte(bus, address)
            ),
            instructions::AddressInfo::Indirect { indirect, address } => {
                write!(str_buf, "(${:04X}) = {:04X}", indirect, address)
            }
            instructions::AddressInfo::IndirectIndexed {
                offset,
                indirect,
                address,
            } => write!(
                str_buf,
                "(${:02X}),Y = {:04X} @ {:04X}",
                offset, indirect, address,
            ),
            instructions::AddressInfo::Relative { offset, address } => {
                write!(str_buf, "${:04X}", address)
            }
            instructions::AddressInfo::ZeroPage { address } => {
                write!(str_buf, "${:02X} = {:02X}", address, self.read_byte(bus, address as u16))
            }
            instructions::AddressInfo::ZeroPageIndexedX { offset, address } => {
                write!(
                    str_buf,
                    "${:02X},X @ {:02X} = {:02X}",
                    offset,
                    address,
                    self.read_byte(bus, address)
                )
            }
            instructions::AddressInfo::ZeroPageIndexedY { offset, address } => {
                write!(
                    str_buf,
                    "${:02X},Y @ {:02X} = {:02X}",
                    offset,
                    address,
                    self.read_byte(bus, address)
                )
            }
        }
        .unwrap();

        while str_buf.len() < 48 {
            str_buf.push(' ');
        }

        write!(
            str_buf,
            "A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}",
            self.a, self.x, self.y, self.status, self.sp, self.cycles
        ).unwrap();

        writer.write(&str_buf.as_bytes()).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use crate::cartridge;
    use crate::ines;

    use crate::bus::MemoryBus;
    use crate::cpu::CPU;

    #[test]
    fn test_debug_log() {
        let mut rom_file = std::fs::File::open("tests/nestest.nes").unwrap();
        let (c, m) = ines::load(&mut rom_file).expect("failed to load cartridge");

        let mut bus = MemoryBus{
            ram: [0; 0x800],
            mapper: cartridge::new(c, m).unwrap(),
        };


        let mut cpu = CPU::default();
        cpu.reset(&mut bus);
        cpu.pc = 0xc000;

        let mut buf = std::io::stdout();

        for _ in 0..1000 {
            cpu.step(&mut bus, Some(&mut buf));
        }
        
    }
}

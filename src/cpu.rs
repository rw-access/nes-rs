use crate::bus::MemoryBus;
use crate::cartridge::Mapper;
use crate::instructions::*;

enum StatusFlags {
    C = 0, // Carry Flag
    Z = 1, // Zero Flag
    I = 2, // Interrupt Disable
    D = 3, // Decimal Mode Flag
    B = 4, // Break Command
    U = 5, // Unused flag
    V = 6, // Overflow Flag
    N = 7, // Negative Flag
}

#[derive(Clone, Debug)]
pub(crate) struct CPU {
    cycles: u64,
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    status: u8,
    sp: u8,
    pub(crate) ram: [u8; 0x800],
}

impl Default for CPU {
    fn default() -> Self {
        Self {
            cycles: Default::default(),
            pc: Default::default(),
            a: Default::default(),
            x: Default::default(),
            y: Default::default(),
            status: Default::default(),
            sp: Default::default(),
            ram: [0; 0x800],
        }
    }
}

fn crosses_page_boundary(a: u16, b: u16) -> bool {
    let [_, a_page] = a.to_le_bytes();
    let [_, b_page] = b.to_le_bytes();

    a_page != b_page
}

impl CPU {
    pub(crate) fn reset(&mut self, bus: &mut MemoryBus) {
        // https://www.nesdev.org/wiki/CPU_ALL#At_power-up
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xfd;
        self.status = (1 << StatusFlags::I as u8) | (1 << StatusFlags::U as u8);
        self.pc = self.read_address(bus, 0xfffc);

        // Disable frame IRQ, disable all audio, clear IO registers
        for addr in 0x4000..=0x4013 {
            self.write_byte(bus, addr, 0x00);
        }

        self.write_byte(bus, 0x4015, 0x40);
        self.write_byte(bus, 0x4017, 0x40);
    }

    fn check_status_bit(&self, bit: StatusFlags) -> bool {
        let mask = 1 << (bit as u8);
        self.status & mask != 0
    }

    fn write_status_bit(&mut self, bit: StatusFlags, value: bool) {
        let mask = 1 << (bit as u8);

        self.status &= !mask;
        self.status |= if value { mask } else { 0 };
    }

    fn set_nz(&mut self, value: u8) {
        self.write_status_bit(StatusFlags::N, value >= 0x80);
        self.write_status_bit(StatusFlags::Z, value == 0x00);
    }

    fn set_cnz(&mut self, value: u16) {
        self.write_status_bit(StatusFlags::C, (value & 0x100) != 0);
        self.set_nz(value as u8);
    }

    pub(crate) fn step(
        &mut self,
        bus: &mut MemoryBus,
        log: Option<&mut dyn std::io::Write>,
    ) -> u16 {
        // NMI takes the highest priority
        if bus.ppu.read_nmi_line() {
            if let Some(log) = log {
                write!(log, "======== NMI ========\n").unwrap();
            }

            self.push_address(bus, self.pc);
            self.dispatch(bus, Opcode::PHP, None);
            self.pc = self.read_address(bus, 0xFFFA);
            self.write_status_bit(StatusFlags::I, true);
            self.cycles = self.cycles.wrapping_add(7);
            return 7;
        }

        let pre_cycles = self.cycles;

        // decode the instrucation @ PC
        let instr = self.decode(bus, self.pc);

        if let Some(writer) = log {
            self.debug_instruction(bus, writer, &instr);
            writer.write(b"\n").unwrap();
        }

        self.pc = self.pc.wrapping_add(instr.width as u16);
        self.cycles = self
            .cycles
            .wrapping_add(instr.extended_opcode.min_cycles as u64)
            .wrapping_add(if instr.page_boundary_hit { 1 } else { 0 });

        self.dispatch(bus, instr.extended_opcode.opcode, instr.final_address);

        self.cycles.wrapping_sub(pre_cycles) as u16
    }

    fn branch_on_flag(&mut self, flag: StatusFlags, branch_status: bool, new_pc: u16) {
        if self.check_status_bit(flag) == branch_status {
            self.cycles = self
                .cycles
                .wrapping_add(1)
                .wrapping_add(crosses_page_boundary(self.pc, new_pc) as u64);
            self.pc = new_pc;
        }
    }

    fn dispatch(&mut self, bus: &mut MemoryBus, opcode: Opcode, addr: Option<u16>) {
        match (opcode, addr) {
            (Opcode::ADC, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ADC
                let a = self.a as u16;
                let b = self.read_byte(bus, addr) as u16;
                let c = self.check_status_bit(StatusFlags::C) as u16;
                let sum = a + b + c;

                self.a = sum as u8;
                self.write_status_bit(StatusFlags::V, ((a ^ sum) & (b ^ sum) & 0x80) != 0);
                self.set_cnz(sum);
            }
            (Opcode::AHX, _) => todo!(),
            (Opcode::ALR, _) => todo!(),
            (Opcode::ANC, _) => todo!(),
            (Opcode::AND, Some(addr)) => {
                self.a &= self.read_byte(bus, addr);
                self.set_nz(self.a);
            }
            (Opcode::ARR, _) => todo!(),
            (Opcode::ASL, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ASL
                let wide = (self.a as u16) << 1;
                self.a = wide as u8;
                self.set_cnz(wide);
            }
            (Opcode::ASL, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ASL
                let wide = (self.read_byte(bus, addr) as u16) << 1;
                self.write_byte(bus, addr, wide as u8);
                self.set_cnz(wide);
            }
            (Opcode::AXS, _) => todo!(),
            (Opcode::BCC, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BCC
                self.branch_on_flag(StatusFlags::C, false, addr)
            }
            (Opcode::BCS, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BCS
                self.branch_on_flag(StatusFlags::C, true, addr)
            }
            (Opcode::BEQ, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BEQ
                self.branch_on_flag(StatusFlags::Z, true, addr)
            }
            (Opcode::BIT, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BIT
                let m = self.read_byte(bus, addr);
                let result = self.a & m;
                self.write_status_bit(StatusFlags::Z, result == 0);
                self.write_status_bit(StatusFlags::V, (m & 0b0100_0000) != 0);
                self.write_status_bit(StatusFlags::N, (m & 0b1000_0000) != 0);
            }
            (Opcode::BMI, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BMI
                self.branch_on_flag(StatusFlags::N, true, addr);
            }
            (Opcode::BNE, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BNE
                self.branch_on_flag(StatusFlags::Z, false, addr);
            }
            (Opcode::BPL, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BPL
                self.branch_on_flag(StatusFlags::N, false, addr);
            }
            (Opcode::BRK, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BRK
                self.push_address(bus, self.pc);
                self.dispatch(bus, Opcode::PHP, None);
                self.write_status_bit(StatusFlags::I, true);
                self.pc = self.read_address(bus, 0xfffe);
            }
            (Opcode::BVC, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BVC
                self.branch_on_flag(StatusFlags::V, false, addr);
            }
            (Opcode::BVS, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#BVS
                self.branch_on_flag(StatusFlags::V, true, addr);
            }
            (Opcode::CLC, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#CLC
                self.write_status_bit(StatusFlags::C, false);
            }
            (Opcode::CLD, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#CLD
                self.write_status_bit(StatusFlags::D, false);
            }
            (Opcode::CLI, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#CLI
                self.write_status_bit(StatusFlags::I, false);
            }
            (Opcode::CLV, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#CLV
                self.write_status_bit(StatusFlags::V, false);
            }
            (Opcode::CMP, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#CMP
                let a = self.a;
                let m = self.read_byte(bus, addr);
                let data = a.wrapping_sub(m);
                self.set_nz(data);
                self.write_status_bit(StatusFlags::C, a >= m);
            }
            (Opcode::CPX, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#CPX
                let x = self.x;
                let m = self.read_byte(bus, addr);
                let data = x.wrapping_sub(m);
                self.set_nz(data);
                self.write_status_bit(StatusFlags::C, x >= m);
            }
            (Opcode::CPY, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#CPY
                let y = self.y;
                let m = self.read_byte(bus, addr);
                let data = y.wrapping_sub(m);
                self.set_nz(data);
                self.write_status_bit(StatusFlags::C, y >= m);
            }
            (Opcode::DCP, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#DCP
                self.dispatch(bus, Opcode::DEC, Some(addr));
                self.dispatch(bus, Opcode::CMP, Some(addr));
            }
            (Opcode::DEC, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#DEC
                let m = self.read_byte(bus, addr).wrapping_sub(1);
                self.write_byte(bus, addr, m);
                self.set_nz(m);
            }
            (Opcode::DEX, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#DEX
                self.x = self.x.wrapping_sub(1);
                self.set_nz(self.x);
            }
            (Opcode::DEY, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#DEY
                self.y = self.y.wrapping_sub(1);
                self.set_nz(self.y);
            }
            (Opcode::EOR, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#EOR
                self.a ^= self.read_byte(bus, addr);
                self.set_nz(self.a);
            }
            (Opcode::INC, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#INC
                let data = self.read_byte(bus, addr).wrapping_add(1);
                self.write_byte(bus, addr, data);
                self.set_nz(data);
            }
            (Opcode::INX, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#INX
                self.x = self.x.wrapping_add(1);
                self.set_nz(self.x);
            }
            (Opcode::INY, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#INY
                self.y = self.y.wrapping_add(1);
                self.set_nz(self.y);
            }
            (Opcode::ISB, Some(addr)) => {
                self.dispatch(bus, Opcode::INC, Some(addr));
                self.dispatch(bus, Opcode::SBC, Some(addr));
            }
            (Opcode::JMP, Some(addr)) => {
                // // https://www.nesdev.org/obelisk-6502-guide/reference.html#JMP
                self.pc = addr;
            }
            (Opcode::JSR, Some(addr)) => {
                self.push_address(bus, self.pc.wrapping_sub(1));
                self.pc = addr;
            }
            (Opcode::LAS, _) => todo!(),
            (Opcode::LAX, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#LAX
                let data = self.read_byte(bus, addr);
                self.a = data;
                self.x = data;
                self.set_nz(data);
            }
            (Opcode::LDA, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#LDA
                self.a = self.read_byte(bus, addr);
                self.set_nz(self.a);
            }
            (Opcode::LDX, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#LDX
                self.x = self.read_byte(bus, addr);
                self.set_nz(self.x);
            }
            (Opcode::LDY, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#LDY
                self.y = self.read_byte(bus, addr);
                self.set_nz(self.y);
            }
            (Opcode::LSR, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#LSR
                let mut wide = self.a as u16;
                wide = wide >> 1 | ((wide & 0b1) << 8);
                self.a = wide as u8;
                self.set_cnz(wide);
            }
            (Opcode::LSR, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#LSR
                let mut wide = self.read_byte(bus, addr) as u16;
                wide = wide >> 1 | ((wide & 0b1) << 8);
                self.write_byte(bus, addr, wide as u8);
                self.set_cnz(wide);
            }
            (Opcode::NOP, _) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#NOP
            }
            (Opcode::ORA, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ORA
                self.a |= self.read_byte(bus, addr);
                self.set_nz(self.a);
            }
            (Opcode::PHA, None) => {
                // // https://www.nesdev.org/obelisk-6502-guide/reference.html#PHA

                self.push_byte(bus, self.a);
            }
            (Opcode::PHP, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#PHP
                self.push_byte(bus, self.status | (1 << (StatusFlags::B as u8)));
            }
            (Opcode::PLA, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#PLA
                self.a = self.pull_byte(bus);
                self.set_nz(self.a);
            }
            (Opcode::PLP, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#PLP
                self.status = self.pull_byte(bus);
                self.write_status_bit(StatusFlags::U, true);
                self.write_status_bit(StatusFlags::B, false);
            }
            (Opcode::RLA, opt_addr) => {
                // http://www.oxyron.de/html/opcodes02.html
                // RLA {adr} = ROL {adr} + AND {adr}
                self.dispatch(bus, Opcode::ROL, opt_addr);
                self.dispatch(bus, Opcode::AND, addr)
            }
            (Opcode::ROL, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ROL
                let mut wide = self.a as u16;
                wide = (wide << 1) | (self.check_status_bit(StatusFlags::C) as u16);
                self.a = wide as u8;
                self.set_cnz(wide);
            }
            (Opcode::ROL, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ROL
                let mut wide = self.read_byte(bus, addr) as u16;
                wide = (wide << 1) | (self.check_status_bit(StatusFlags::C) as u16);
                self.write_byte(bus, addr, wide as u8);
                self.set_cnz(wide);
            }
            (Opcode::ROR, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ROR
                let mut wide = self.read_byte(bus, addr) as u16;
                wide |= (self.check_status_bit(StatusFlags::C) as u16) << 8;
                wide |= (wide & 0b1) << 9;
                wide >>= 1;
                self.write_byte(bus, addr, wide as u8);
                self.set_cnz(wide);
            }
            (Opcode::ROR, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#ROR
                let mut wide = self.a as u16;
                wide |= (self.check_status_bit(StatusFlags::C) as u16) << 8;
                wide |= (wide & 0b1) << 9;
                wide >>= 1;
                self.a = wide as u8;
                self.set_cnz(wide);
            }
            (Opcode::RRA, Some(addr)) => {
                // http://www.oxyron.de/html/opcodes02.html
                // RRA {adr} = ROR {adr} + ADC {adr}
                self.dispatch(bus, Opcode::ROR, Some(addr));
                self.dispatch(bus, Opcode::ADC, Some(addr));
            }
            (Opcode::RTI, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#RTI
                self.dispatch(bus, Opcode::PLP, None);
                self.pc = self.pull_address(bus);
            }
            (Opcode::RTS, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#RTS
                self.pc = self.pull_address(bus).wrapping_add(1);
            }
            (Opcode::SAX, Some(addr)) => {
                // http://www.oxyron.de/html/opcodes02.html
                // SAX {adr} = store A&X into {adr}

                self.write_byte(bus, addr, self.a & self.x)
            }
            (Opcode::SBC, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#SBC
                let a = self.a as u16;
                let m = self.read_byte(bus, addr) as u16;
                let result = a
                    .wrapping_sub(m)
                    .wrapping_sub((!self.check_status_bit(StatusFlags::C)) as u16);
                self.a = result as u8;

                self.set_nz(self.a);
                self.write_status_bit(StatusFlags::V, (((a ^ result) & (!m ^ result)) & 0x80) != 0);
                self.write_status_bit(StatusFlags::C, (result & 0x100) == 0);
            }
            (Opcode::SEC, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#SEC
                self.write_status_bit(StatusFlags::C, true);
            }
            (Opcode::SED, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#SED
                self.write_status_bit(StatusFlags::D, true);
            }
            (Opcode::SEI, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#SEI
                self.write_status_bit(StatusFlags::I, true);
            }
            (Opcode::SHX, _) => todo!(),
            (Opcode::SHY, _) => todo!(),
            (Opcode::SLO, Some(addr)) => {
                // http://www.oxyron.de/html/opcodes02.html
                // SLO {adr} = ASL {adr} + ORA {adr}
                self.dispatch(bus, Opcode::ASL, Some(addr));
                self.dispatch(bus, Opcode::ORA, Some(addr));
            }
            (Opcode::SRE, Some(addr)) => {
                // http://www.oxyron.de/html/opcodes02.html
                // SRE {adr} = LSR {adr} + EOR {adr}
                self.dispatch(bus, Opcode::LSR, Some(addr));
                self.dispatch(bus, Opcode::EOR, Some(addr));
            }
            (Opcode::STA, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#STA
                self.write_byte(bus, addr, self.a);
            }
            (Opcode::STP, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#STP
                self.write_byte(bus, addr, self.status);
            }
            (Opcode::STX, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#STX
                self.write_byte(bus, addr, self.x);
            }
            (Opcode::STY, Some(addr)) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#STY
                self.write_byte(bus, addr, self.y);
            }
            (Opcode::TAS, _) => todo!(),
            (Opcode::TAX, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#TAX
                self.x = self.a;
                self.set_nz(self.x);
            }
            (Opcode::TAY, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#TAY
                self.y = self.a;
                self.set_nz(self.y);
            }
            (Opcode::TSX, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#TSX
                self.x = self.sp;
                self.set_nz(self.x);
            }
            (Opcode::TXA, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#TXA
                self.a = self.x;
                self.set_nz(self.a);
            }
            (Opcode::TXS, None) => {
                // https://www.nesdev.org/obelisk-6502-guide/reference.html#TXA
                self.sp = self.x;
            }
            (Opcode::TYA, None) => {
                self.a = self.y;
                self.set_nz(self.a);
            }
            (Opcode::XAA, _) => todo!(),
            _ => unreachable!("unknown instruction: {:?}", opcode),
        }
    }

    pub(crate) fn read_byte(&self, bus: &MemoryBus, addr: u16) -> u8 {
        // https://www.nesdev.org/wiki/CPU_memory_map
        match addr {
            0x0000..=0x1fff => self.ram[addr as usize % self.ram.len()],
            0x2000..=0x3fff => bus.ppu.read_register(bus.mapper.as_ref(), addr), // PPU
            0x4000..=0x4013 => 0,                                                // APU
            0x4014 => 0,                                                         // DMA
            0x4016 => bus.controller.read(),                                     // controller 1
            0x4017 => 0,                                                         // controller 2
            0x4018..=0x401F => 0, // disabled test mode
            _ => bus.mapper.read(addr),
        }
    }

    fn read_page<'a>(&'a self, mapper: &'a dyn Mapper, page: u8) -> Option<&'a [u8; 256]> {
        match page {
            0x00..=0x1f => (&self.ram[(page as usize) << 8..][..256]).try_into().ok(),
            0x20..=0x7f => None, // IO ports
            0x80.. => mapper.read_page(page),
        }
    }

    fn read_address(&self, bus: &MemoryBus, addr: u16) -> u16 {
        let lo = self.read_byte(bus, addr);
        let hi = self.read_byte(bus, addr.wrapping_add(1));

        u16::from_le_bytes([lo, hi])
    }

    fn read_address_indirect(&self, bus: &MemoryBus, addr: u16) -> u16 {
        let [offset, page] = addr.to_le_bytes();
        let lo = self.read_byte(bus, addr);
        let hi = self.read_byte(bus, u16::from_le_bytes([offset.wrapping_add(1), page]));

        u16::from_le_bytes([lo, hi])
    }

    pub(crate) fn write_byte(&mut self, bus: &mut MemoryBus, addr: u16, data: u8) {
        // https://www.nesdev.org/wiki/CPU_memory_map
        match addr {
            0x0000..=0x1fff => self.ram[addr as usize % self.ram.len()] = data,
            0x2000..=0x3fff => bus.ppu.write_register(bus.mapper.as_mut(), addr, data), // PPU
            0x4000..=0x4013 => {}                                                       // APU
            0x4014 => {
                let page = self.read_page(bus.mapper.as_ref(), data);
                bus.ppu.write_dma(page);
            } // DMA
            0x4016 => bus.controller.write(data), // controller 1
            0x4017 => {}                          // controller 2
            0x4018..=0x401F => {}                 // disabled test mode
            _ => bus.mapper.write(addr, data),
        };
    }

    fn push_byte(&mut self, bus: &mut MemoryBus, data: u8) {
        self.write_byte(bus, u16::from_le_bytes([self.sp, 0x1]), data);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pull_byte(&mut self, bus: &mut MemoryBus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read_byte(bus, u16::from_le_bytes([self.sp, 0x1]))
    }

    fn push_address(&mut self, bus: &mut MemoryBus, addr: u16) {
        let [lo, hi] = addr.to_le_bytes();
        self.push_byte(bus, hi);
        self.push_byte(bus, lo);
    }

    fn pull_address(&mut self, bus: &mut MemoryBus) -> u16 {
        let lo = self.pull_byte(bus);
        let hi = self.pull_byte(bus);

        u16::from_le_bytes([lo, hi])
    }

    fn decode(&self, bus: &MemoryBus, addr: u16) -> DecodedInstruction {
        let operand_addr = addr.wrapping_add(1);
        let opcode = self.read_byte(bus, addr);
        let extended_opcode = &EXTENDED_OPCODES[opcode as usize];

        match extended_opcode.addressing_mode {
            AddressingMode::Absolute => {
                let address = self.read_address(bus, operand_addr);
                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::Absolute { address: address },
                    width: 3,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            AddressingMode::Implied => DecodedInstruction {
                extended_opcode,
                address_info: AddressInfo::Implied,
                width: 1,
                page_boundary_hit: false,
                final_address: None,
            },
            AddressingMode::Accumulator => DecodedInstruction {
                extended_opcode,
                address_info: AddressInfo::Accumulator,
                width: 1,
                page_boundary_hit: false,
                final_address: None,
            },
            AddressingMode::AbsoluteIndexedX => {
                let indirect = self.read_address(bus, operand_addr);
                let address = indirect.wrapping_add(self.x as u16);

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::AbsoluteIndexedX { indirect, address },
                    width: 3,
                    page_boundary_hit: extended_opcode.page_boundary_penalty
                        && crosses_page_boundary(indirect, address),
                    final_address: Some(address),
                }
            }
            AddressingMode::AbsoluteIndexedY => {
                let indirect = self.read_address(bus, operand_addr);
                let address = indirect.wrapping_add(self.y as u16);

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::AbsoluteIndexedY { indirect, address },
                    width: 3,
                    page_boundary_hit: extended_opcode.page_boundary_penalty
                        && crosses_page_boundary(indirect, address),
                    final_address: Some(address),
                }
            }
            AddressingMode::Immediate => {
                let address = operand_addr;
                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::Immediate { address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            AddressingMode::IndexedIndirect => {
                let offset = self.read_byte(bus, operand_addr);
                let indirect = offset.wrapping_add(self.x) as u16;
                let address = self.read_address_indirect(bus, indirect);

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::IndexedIndirect {
                        offset,
                        indirect,
                        address,
                    },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            AddressingMode::Indirect => {
                let indirect = self.read_address(bus, operand_addr);
                let address = self.read_address_indirect(bus, indirect);

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::Indirect { indirect, address },
                    width: 3,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            AddressingMode::IndirectIndexed => {
                let offset = self.read_byte(bus, operand_addr);
                let indirect = self.read_address_indirect(bus, offset as u16);
                let address = indirect.wrapping_add(self.y as u16);

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::IndirectIndexed {
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
            AddressingMode::Relative => {
                let offset = self.read_byte(bus, operand_addr);
                let next_pc = addr.wrapping_add(2);
                let address = if offset >= 0x80 {
                    next_pc.wrapping_sub(0x100 - (offset as u16))
                } else {
                    next_pc.wrapping_add(offset as u16)
                };

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::Relative { offset, address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            AddressingMode::ZeroPage => {
                let address = self.read_byte(bus, operand_addr);
                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::ZeroPage { address: address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address as u16),
                }
            }
            AddressingMode::ZeroPageIndexedX => {
                let offset = self.read_byte(bus, operand_addr);
                let address = offset.wrapping_add(self.x) as u16;

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::ZeroPageIndexedX { offset, address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
            AddressingMode::ZeroPageIndexedY => {
                let offset = self.read_byte(bus, operand_addr);
                let address = offset.wrapping_add(self.y) as u16;

                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::ZeroPageIndexedY { offset, address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address),
                }
            }
        }
    }

    fn debug_instruction(
        &self,
        bus: &MemoryBus,
        writer: &mut dyn std::io::Write,
        decoded: &DecodedInstruction,
    ) {
        // C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD CYC:7
        // PC    < raw >   < assembly >                    < registers >             < timing >
        let prev_ppu_address = bus.ppu.last_read.get();

        // alocate a string on the stack, because it's fixed size and we can keep track of the position information
        // as it grows. once complete, there's a single copy to the writer
        use std::fmt::Write;
        let mut str_buf = arrayvec::ArrayString::<120>::new();

        write!(str_buf, "{:04X}  ", self.pc).unwrap();

        for offset in 0..3 {
            if offset < decoded.width {
                let byte_addr = self.pc.wrapping_add(offset as u16);
                write!(str_buf, "{:02X} ", self.read_byte(bus, byte_addr)).unwrap();
            } else {
                write!(str_buf, "   ").unwrap();
            }
        }

        write!(str_buf, " {:?} ", decoded.extended_opcode.opcode).unwrap();

        match decoded.address_info {
            AddressInfo::Implied => Ok(()),
            AddressInfo::Accumulator => write!(str_buf, "A"),
            AddressInfo::Absolute { address } => match decoded.extended_opcode.opcode {
                Opcode::JSR | Opcode::JMP => {
                    write!(str_buf, "${:04X}", address)
                }
                _ => write!(
                    str_buf,
                    "${:04X} = {:02X}",
                    address,
                    self.read_byte(bus, address)
                ),
            },
            AddressInfo::AbsoluteIndexedX { indirect, address } => {
                write!(
                    str_buf,
                    "${:04X},X @ {:04X} = {:02X}",
                    indirect,
                    address,
                    self.read_byte(bus, address)
                )
            }
            AddressInfo::AbsoluteIndexedY { indirect, address } => {
                write!(
                    str_buf,
                    "${:04X},Y @ {:04X} = {:02X}",
                    indirect,
                    address,
                    self.read_byte(bus, address)
                )
            }
            AddressInfo::Immediate { address } => {
                write!(str_buf, "#${:02X}", self.read_byte(bus, address))
            }
            AddressInfo::IndexedIndirect {
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
            AddressInfo::Indirect { indirect, address } => {
                write!(str_buf, "(${:04X}) = {:04X}", indirect, address)
            }
            AddressInfo::IndirectIndexed {
                offset,
                indirect,
                address,
            } => write!(
                str_buf,
                "(${:02X}),Y = {:04X} @ {:04X} = {:02X}",
                offset,
                indirect,
                address,
                self.read_byte(bus, address)
            ),
            AddressInfo::Relative { offset: _, address } => {
                write!(str_buf, "${:04X}", address)
            }
            AddressInfo::ZeroPage { address } => {
                write!(
                    str_buf,
                    "${:02X} = {:02X}",
                    address,
                    self.read_byte(bus, address as u16)
                )
            }
            AddressInfo::ZeroPageIndexedX { offset, address } => {
                write!(
                    str_buf,
                    "${:02X},X @ {:02X} = {:02X}",
                    offset,
                    address,
                    self.read_byte(bus, address)
                )
            }
            AddressInfo::ZeroPageIndexedY { offset, address } => {
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
        )
        .unwrap();

        writer.write(&str_buf.as_bytes()).unwrap();

        // restore the PPU last read address
        bus.ppu.last_read.set(prev_ppu_address);
    }
}

#[cfg(test)]
mod tests {
    use crate::cartridge;
    use crate::console::Console;
    use crate::ines;

    use crate::bus::MemoryBus;
    use crate::cpu::CPU;

    #[test]
    fn test_debug_log() {
        let mut log_file = std::fs::File::create("tests/nestest.log").unwrap();
        let mut rom_file = std::fs::File::open("tests/nestest.nes").unwrap();
        let (c, m) = ines::load(&mut rom_file).expect("failed to load cartridge");

        let mut console = Console::new(cartridge::new(c, m).unwrap());
        console.cpu.pc = 0xc000;

        // match offset for nestest.nes
        cpu.cycles = 7;

        for _ in 0..8991 {
            cpu.step(&mut bus, Some(&mut log_file));
        }
    }
}

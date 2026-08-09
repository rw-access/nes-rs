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
    #[cfg(feature = "timestamped-scheduler")]
    timestamped_decoded: Option<(u16, CompactDecodedInstruction)>,
    #[cfg(feature = "timestamped-scheduler")]
    timestamped_static_cache: [TimestampedStaticInstruction; 256],
}

#[derive(Clone, Copy, Debug)]
struct CompactDecodedInstruction {
    opcode: Opcode,
    addressing_mode: AddressingMode,
    final_address: Option<u16>,
    base_address: Option<u16>,
    width: u8,
    min_cycles: u8,
    page_boundary_hit: bool,
}

#[cfg(feature = "timestamped-scheduler")]
#[derive(Clone, Copy, Debug)]
struct TimestampedStaticInstruction {
    pc: u16,
    opcode: Opcode,
    addressing_mode: AddressingMode,
    operand: [u8; 2],
    min_cycles: u8,
    page_boundary_penalty: bool,
    valid: bool,
}

#[cfg(feature = "timestamped-scheduler")]
impl Default for TimestampedStaticInstruction {
    fn default() -> Self {
        Self {
            pc: 0,
            opcode: Opcode::NOP,
            addressing_mode: AddressingMode::Implied,
            operand: [0; 2],
            min_cycles: 2,
            page_boundary_penalty: false,
            valid: false,
        }
    }
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
            #[cfg(feature = "timestamped-scheduler")]
            timestamped_decoded: None,
            #[cfg(feature = "timestamped-scheduler")]
            timestamped_static_cache: [TimestampedStaticInstruction::default(); 256],
        }
    }
}

fn crosses_page_boundary(a: u16, b: u16) -> bool {
    let [_, a_page] = a.to_le_bytes();
    let [_, b_page] = b.to_le_bytes();

    a_page != b_page
}

#[cfg(feature = "timestamped-scheduler")]
#[inline]
fn is_ppu_register_address(address: u16) -> bool {
    (0x2000..=0x3fff).contains(&address) || address == 0x4014
}

#[cfg(feature = "timestamped-scheduler")]
#[inline]
fn is_cartridge_address(address: u16) -> bool {
    address >= 0x8000
}

#[cfg(feature = "timestamped-scheduler")]
#[inline]
fn is_non_cartridge_address(address: u16) -> bool {
    !is_cartridge_address(address)
}

#[cfg(feature = "timestamped-scheduler")]
#[inline]
fn is_indexed_ppu_access(base: u16, final_address: u16) -> bool {
    // Indexed reads, and conservatively indexed writes, can issue a
    // page-crossing dummy access using the old high byte and new low byte.
    let dummy_address = (base & 0xff00) | (final_address & 0x00ff);
    is_ppu_register_address(final_address) || is_ppu_register_address(dummy_address)
}

#[cfg(feature = "timestamped-scheduler")]
#[inline]
fn instruction_width(opcode: Opcode, addressing_mode: AddressingMode) -> u16 {
    if matches!(opcode, Opcode::BRK) {
        // BRK fetches and discards its signature byte before reading the
        // interrupt vector, even though the opcode table models it as
        // implied addressing.
        return 2;
    }

    match addressing_mode {
        AddressingMode::Implied | AddressingMode::Accumulator => 1,
        AddressingMode::Immediate
        | AddressingMode::IndexedIndirect
        | AddressingMode::IndirectIndexed
        | AddressingMode::Relative
        | AddressingMode::ZeroPage
        | AddressingMode::ZeroPageIndexedX
        | AddressingMode::ZeroPageIndexedY => 2,
        AddressingMode::Absolute
        | AddressingMode::AbsoluteIndexedX
        | AddressingMode::AbsoluteIndexedY
        | AddressingMode::Indirect => 3,
    }
}

#[cfg(feature = "timestamped-scheduler")]
#[inline]
fn is_control_flow_opcode(opcode: Opcode) -> bool {
    matches!(opcode, Opcode::JMP | Opcode::JSR)
}

#[cfg(feature = "timestamped-scheduler")]
#[inline]
fn is_timestamped_block_terminator(opcode: Opcode) -> bool {
    matches!(
        opcode,
        Opcode::BCC
            | Opcode::BCS
            | Opcode::BEQ
            | Opcode::BMI
            | Opcode::BNE
            | Opcode::BPL
            | Opcode::BVC
            | Opcode::BVS
            | Opcode::JMP
            | Opcode::JSR
            | Opcode::RTI
            | Opcode::RTS
            | Opcode::BRK
    )
}

impl CPU {
    /// Conservatively determine whether the next CPU instruction may touch a
    /// PPU register or leave the cartridge-backed instruction stream.
    ///
    /// The timestamped runner can use this before taking a long CPU run.  A
    /// `true` result is deliberately allowed to be pessimistic: reads from
    /// non-cartridge regions are not inspected because doing so could itself
    /// have I/O side effects.  In particular, code or operands below `$8000`,
    /// indirect pointers in I/O space, and `RTS`/`RTI` are treated as unsafe.
    #[cfg(feature = "timestamped-scheduler")]
    pub(crate) fn next_instruction_may_access_ppu(&self, bus: &MemoryBus) -> bool {
        // Do not probe opcode or operand bytes through the bus until every
        // byte is known to be cartridge-backed.  This keeps preflight from
        // accidentally reading a PPU register while trying to decide whether
        // the instruction reads a PPU register.
        let opcode_address = self.pc;
        if !is_cartridge_address(opcode_address) {
            return true;
        }

        let opcode = self.read_code_byte(bus, opcode_address);
        let extended_opcode = &EXTENDED_OPCODES[opcode as usize];
        let width = instruction_width(extended_opcode.opcode, extended_opcode.addressing_mode);

        for offset in 1..width {
            if !is_cartridge_address(opcode_address.wrapping_add(offset as u16)) {
                return true;
            }
        }

        let operand_address = opcode_address.wrapping_add(1);
        let may_access = match extended_opcode.addressing_mode {
            AddressingMode::Implied | AddressingMode::Accumulator | AddressingMode::Immediate => {
                false
            }
            AddressingMode::Absolute => {
                let address = self.read_code_address(bus, operand_address);
                if is_control_flow_opcode(extended_opcode.opcode) {
                    is_non_cartridge_address(address)
                } else {
                    is_ppu_register_address(address)
                }
            }
            AddressingMode::AbsoluteIndexedX => {
                let base = self.read_code_address(bus, operand_address);
                let address = base.wrapping_add(self.x as u16);
                is_indexed_ppu_access(base, address)
            }
            AddressingMode::AbsoluteIndexedY => {
                let base = self.read_code_address(bus, operand_address);
                let address = base.wrapping_add(self.y as u16);
                is_indexed_ppu_access(base, address)
            }
            AddressingMode::IndexedIndirect => {
                let offset = self.read_code_byte(bus, operand_address);
                let pointer = offset.wrapping_add(self.x) as u16;
                self.preflight_zero_page_indirect(bus, pointer)
                    .is_some_and(is_ppu_register_address)
            }
            AddressingMode::Indirect => {
                let pointer = self.read_code_address(bus, operand_address);
                let Some(address) = self.preflight_indirect(bus, pointer) else {
                    return true;
                };
                // The target is the next instruction fetch.  Treat a target
                // outside cartridge space as unsafe even when it is not
                // itself in the PPU range.
                is_non_cartridge_address(address)
            }
            AddressingMode::IndirectIndexed => {
                let offset = self.read_code_byte(bus, operand_address);
                let Some(base) = self.preflight_zero_page_indirect(bus, offset as u16) else {
                    return true;
                };
                let address = base.wrapping_add(self.y as u16);
                is_indexed_ppu_access(base, address)
            }
            AddressingMode::Relative => {
                let offset = self.read_code_byte(bus, operand_address);
                let next_pc = opcode_address.wrapping_add(2);
                let target = if offset >= 0x80 {
                    next_pc.wrapping_sub(0x100 - offset as u16)
                } else {
                    next_pc.wrapping_add(offset as u16)
                };
                is_non_cartridge_address(target)
            }
            AddressingMode::ZeroPage
            | AddressingMode::ZeroPageIndexedX
            | AddressingMode::ZeroPageIndexedY => false,
        };

        may_access || matches!(extended_opcode.opcode, Opcode::RTS | Opcode::RTI)
    }

    /// Decode the next instruction once and retain it for the timestamped
    /// runner. The decode is only retained when it is safe to perform without
    /// touching PPU registers; indirect JMP pointers in I/O space are left to
    /// the ordinary CPU step after the scheduler catches the PPU up.
    #[cfg(feature = "timestamped-scheduler")]
    fn decode_timestamped(
        &mut self,
        bus: &MemoryBus,
    ) -> Option<(u16, CompactDecodedInstruction, bool)> {
        let addr = self.pc;
        let static_instruction = self.timestamped_static_instruction(bus, addr)?;

        if matches!(static_instruction.addressing_mode, AddressingMode::Indirect) {
            let pointer = u16::from_le_bytes(static_instruction.operand);
            if self.preflight_indirect(bus, pointer).is_none() {
                return None;
            }
        }

        let decoded = self.decode_compact_static(bus, addr, static_instruction);
        let may_access = self.decoded_instruction_may_access_ppu(addr, &decoded);
        Some((addr, decoded, may_access))
    }

    /// Cache only immutable NROM instruction bytes and opcode metadata. The
    /// effective address remains decoded against live registers/RAM below, so
    /// indexed and indirect instructions retain their normal semantics.
    #[cfg(feature = "timestamped-scheduler")]
    #[inline]
    fn timestamped_static_instruction(
        &mut self,
        bus: &MemoryBus,
        addr: u16,
    ) -> Option<TimestampedStaticInstruction> {
        if !is_cartridge_address(addr) {
            return None;
        }

        let slot = (addr & 0x00ff) as usize;
        let cached = self.timestamped_static_cache[slot];
        if cached.valid && cached.pc == addr {
            return Some(cached);
        }

        let raw_opcode = self.read_code_byte(bus, addr);
        let extended_opcode = &EXTENDED_OPCODES[raw_opcode as usize];
        let width = instruction_width(extended_opcode.opcode, extended_opcode.addressing_mode);
        if (1..width).any(|offset| !is_cartridge_address(addr.wrapping_add(offset))) {
            return None;
        }

        let mut operand = [0; 2];
        for (index, byte) in operand
            .iter_mut()
            .enumerate()
            .take(width.saturating_sub(1) as usize)
        {
            *byte = self.read_code_byte(bus, addr.wrapping_add(index as u16 + 1));
        }

        let decoded = TimestampedStaticInstruction {
            pc: addr,
            opcode: extended_opcode.opcode,
            addressing_mode: extended_opcode.addressing_mode,
            operand,
            min_cycles: extended_opcode.min_cycles,
            page_boundary_penalty: extended_opcode.page_boundary_penalty,
            valid: true,
        };
        self.timestamped_static_cache[slot] = decoded;
        Some(decoded)
    }

    #[cfg(feature = "timestamped-scheduler")]
    pub(crate) fn prepare_timestamped(&mut self, bus: &MemoryBus) -> bool {
        self.timestamped_decoded = None;

        let Some((addr, decoded, may_access)) = self.decode_timestamped(bus) else {
            return true;
        };

        self.timestamped_decoded = Some((addr, decoded));
        may_access
    }

    /// Run a bounded sequence of side-effect-safe instructions without
    /// returning through the scheduler for every instruction. The first
    /// instruction that may observe or modify PPU state is decoded and held
    /// in `timestamped_decoded`; the caller must catch the PPU up before
    /// calling `step_timestamped` to execute it.
    ///
    /// This is deliberately not a general CPU block cache. The block is
    /// capped, stops at control flow, and falls back before any instruction
    /// whose decode could touch an I/O address or leave cartridge space.
    #[cfg(feature = "timestamped-scheduler")]
    pub(crate) fn run_timestamped_block(
        &mut self,
        bus: &mut MemoryBus,
        master_tick_budget: u64,
    ) -> (u16, bool) {
        const MAX_INSTRUCTIONS: usize = 32;
        let start_cycles = self.cycles;

        self.timestamped_decoded = None;
        for _ in 0..MAX_INSTRUCTIONS {
            // A PPU event can only become pending after the scheduler catches
            // up, but keep this check here so a block never runs past an
            // already-visible interrupt boundary.
            if bus.ppu.nmi_pending()
                || (bus.mapper.irq_pending() && !self.check_status_bit(StatusFlags::I))
            {
                self.step(bus, None);
                break;
            }

            let Some((addr, decoded, may_access_ppu)) = self.decode_timestamped(bus) else {
                return (self.cycles.wrapping_sub(start_cycles) as u16, true);
            };

            if may_access_ppu {
                self.timestamped_decoded = Some((addr, decoded));
                return (self.cycles.wrapping_sub(start_cycles) as u16, true);
            }

            let opcode = decoded.opcode;
            self.execute_compact_decoded(bus, decoded);

            let elapsed_master_ticks = self.cycles.wrapping_sub(start_cycles).saturating_mul(3);
            if elapsed_master_ticks >= master_tick_budget || is_timestamped_block_terminator(opcode)
            {
                break;
            }
        }

        (self.cycles.wrapping_sub(start_cycles) as u16, false)
    }

    #[cfg(feature = "timestamped-scheduler")]
    fn decoded_instruction_may_access_ppu(
        &self,
        addr: u16,
        decoded: &CompactDecodedInstruction,
    ) -> bool {
        let may_access = match decoded.addressing_mode {
            AddressingMode::Implied | AddressingMode::Accumulator | AddressingMode::Immediate => {
                false
            }
            AddressingMode::Absolute => {
                let Some(address) = decoded.final_address else {
                    return true;
                };
                if is_control_flow_opcode(decoded.opcode) {
                    is_non_cartridge_address(address)
                } else {
                    is_ppu_register_address(address)
                }
            }
            AddressingMode::AbsoluteIndexedX | AddressingMode::AbsoluteIndexedY => {
                let (Some(base), Some(address)) = (decoded.base_address, decoded.final_address)
                else {
                    return true;
                };
                is_indexed_ppu_access(base, address)
            }
            AddressingMode::IndexedIndirect => {
                decoded.final_address.is_some_and(is_ppu_register_address)
            }
            AddressingMode::Indirect => decoded.final_address.is_some_and(is_non_cartridge_address),
            AddressingMode::IndirectIndexed => {
                let (Some(base), Some(address)) = (decoded.base_address, decoded.final_address)
                else {
                    return true;
                };
                is_indexed_ppu_access(base, address)
            }
            AddressingMode::Relative => decoded.final_address.is_some_and(is_non_cartridge_address),
            AddressingMode::ZeroPage
            | AddressingMode::ZeroPageIndexedX
            | AddressingMode::ZeroPageIndexedY => false,
        };

        may_access
            || !is_cartridge_address(addr)
            || matches!(decoded.opcode, Opcode::BRK | Opcode::RTS | Opcode::RTI)
    }

    #[cfg(feature = "timestamped-scheduler")]
    fn preflight_zero_page_indirect(&self, bus: &MemoryBus, pointer: u16) -> Option<u16> {
        // The 6502 wraps the high-byte read within the zero page for both
        // ($xx,X) and ($xx),Y.
        let next = (pointer & 0xff00) | pointer.wrapping_add(1) & 0x00ff;
        let lo = self.preflight_read_byte(bus, pointer)?;
        let hi = self.preflight_read_byte(bus, next)?;
        Some(u16::from_le_bytes([lo, hi]))
    }

    #[cfg(feature = "timestamped-scheduler")]
    fn preflight_indirect(&self, bus: &MemoryBus, pointer: u16) -> Option<u16> {
        // JMP ($xxxx) has the NMOS 6502 page-wrap bug.
        let next = (pointer & 0xff00) | pointer.wrapping_add(1) & 0x00ff;
        let lo = self.preflight_read_byte(bus, pointer)?;
        let hi = self.preflight_read_byte(bus, next)?;
        Some(u16::from_le_bytes([lo, hi]))
    }

    #[cfg(feature = "timestamped-scheduler")]
    fn preflight_read_byte(&self, bus: &MemoryBus, address: u16) -> Option<u8> {
        match address {
            0x0000..=0x1fff => Some(self.ram[address as usize % self.ram.len()]),
            0x2000..=0x5fff => None,
            _ => Some(bus.mapper.read(address)),
        }
    }

    pub(crate) fn reset(&mut self, bus: &mut MemoryBus) {
        // https://www.nesdev.org/wiki/CPU_ALL#At_power-up
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xfd;
        self.status = 0;
        self.write_status_bit(StatusFlags::I, true);
        self.write_status_bit(StatusFlags::U, true);
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

    fn nmi(&mut self, bus: &mut MemoryBus, log: Option<&mut dyn std::io::Write>) -> u16 {
        if let Some(log) = log {
            write!(log, "======== NMI ========\n").unwrap();
        }

        self.push_address(bus, self.pc);
        self.dispatch(bus, Opcode::PHP, None);
        self.pc = self.read_address(bus, 0xFFFA);
        self.write_status_bit(StatusFlags::I, true);
        self.cycles = self.cycles.wrapping_add(7);
        7
    }

    fn irq(&mut self, bus: &mut MemoryBus, log: Option<&mut dyn std::io::Write>) -> u16 {
        if let Some(log) = log {
            write!(log, "======== IRQ ========\n").unwrap();
        }

        self.push_address(bus, self.pc);
        self.dispatch(bus, Opcode::PHP, None);
        self.pc = self.read_address(bus, 0xFFFE);
        self.write_status_bit(StatusFlags::I, true);
        self.cycles = self.cycles.wrapping_add(7);
        7
    }

    pub(crate) fn step(
        &mut self,
        bus: &mut MemoryBus,
        log: Option<&mut dyn std::io::Write>,
    ) -> u16 {
        // NMI takes the highest priority
        if bus.ppu.read_nmi_line() {
            return self.nmi(bus, log);
        }

        // Cartridge IRQs are maskable and are checked between instructions.
        if bus.mapper.irq_pending() && !self.check_status_bit(StatusFlags::I) {
            self.push_address(bus, self.pc);
            self.dispatch(bus, Opcode::PHP, None);
            self.pc = self.read_address(bus, 0xfffe);
            self.write_status_bit(StatusFlags::I, true);
            self.cycles = self.cycles.wrapping_add(7);
            return 7;
        }

        // APU frame IRQs are maskable and are checked between instructions.
        #[cfg(not(feature = "apu-disabled"))]
        if !self.check_status_bit(StatusFlags::I) && bus.apu.irq_line() {
            return self.irq(bus, log);
        }

        let pre_cycles = self.cycles;

        let (opcode, final_address, width, min_cycles, page_boundary_hit) = if log.is_some() {
            // The rich form is retained for debug logging and diagnostics.
            let instr = self.decode(bus, self.pc);
            if let Some(writer) = log {
                self.debug_instruction(bus, writer, &instr);
                writer.write(b"\n").unwrap();
            }
            (
                instr.extended_opcode.opcode,
                instr.final_address,
                instr.width,
                instr.extended_opcode.min_cycles,
                instr.page_boundary_hit,
            )
        } else {
            let instr = self.decode_compact(bus, self.pc);
            (
                instr.opcode,
                instr.final_address,
                instr.width,
                instr.min_cycles,
                instr.page_boundary_hit,
            )
        };

        self.pc = self.pc.wrapping_add(width as u16);
        self.cycles = self
            .cycles
            .wrapping_add(min_cycles as u64)
            .wrapping_add(page_boundary_hit as u64);

        // Memory writes occur on the final cycle of a 6502 instruction. Pass
        // the write phase and the instruction duration to the APU so $4017's
        // delayed reset is measured from the actual write cycle.
        #[cfg(not(feature = "apu-disabled"))]
        let instruction_cycles = self.cycles.wrapping_sub(pre_cycles) as u8;
        #[cfg(not(feature = "apu-disabled"))]
        let write_phase = bus
            .apu
            .phase_after_cpu_cycles(instruction_cycles.saturating_sub(1));
        #[cfg(not(feature = "apu-disabled"))]
        bus.apu
            .set_register_write_timing(write_phase, instruction_cycles);
        self.dispatch(bus, opcode, final_address);
        #[cfg(not(feature = "apu-disabled"))]
        bus.apu.clear_register_write_timing();

        self.cycles.wrapping_sub(pre_cycles) as u16
    }

    /// Execute the instruction prepared by `prepare_timestamped` after the
    /// scheduler has synchronized the PPU. Interrupts are checked again here
    /// because catch-up may have raised NMI between preparation and execution.
    #[cfg(feature = "timestamped-scheduler")]
    pub(crate) fn step_timestamped(&mut self, bus: &mut MemoryBus) -> u16 {
        let prepared = self.timestamped_decoded.take();

        if bus.ppu.nmi_pending()
            || (bus.mapper.irq_pending() && !self.check_status_bit(StatusFlags::I))
        {
            return self.step(bus, None);
        }

        #[cfg(not(feature = "apu-disabled"))]
        if !self.check_status_bit(StatusFlags::I) && bus.apu.irq_line() {
            return self.step(bus, None);
        }

        let Some((addr, decoded)) = prepared.filter(|(addr, _)| *addr == self.pc) else {
            return self.step(bus, None);
        };
        let _ = addr;
        self.execute_compact_decoded(bus, decoded)
    }

    #[cfg(feature = "timestamped-scheduler")]
    #[inline]
    fn execute_compact_decoded(
        &mut self,
        bus: &mut MemoryBus,
        decoded: CompactDecodedInstruction,
    ) -> u16 {
        let pre_cycles = self.cycles;
        self.pc = self.pc.wrapping_add(decoded.width as u16);
        self.cycles = self
            .cycles
            .wrapping_add(decoded.min_cycles as u64)
            .wrapping_add(decoded.page_boundary_hit as u64);

        #[cfg(not(feature = "apu-disabled"))]
        let instruction_cycles = self.cycles.wrapping_sub(pre_cycles) as u8;
        #[cfg(not(feature = "apu-disabled"))]
        let write_phase = bus
            .apu
            .phase_after_cpu_cycles(instruction_cycles.saturating_sub(1));
        #[cfg(not(feature = "apu-disabled"))]
        bus.apu
            .set_register_write_timing(write_phase, instruction_cycles);
        self.dispatch(bus, decoded.opcode, decoded.final_address);
        #[cfg(not(feature = "apu-disabled"))]
        bus.apu.clear_register_write_timing();

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
            (Opcode::AHX, Some(addr)) => {
                // AHX {addr} = A & X & (high byte of effective address + 1).
                let high = (addr >> 8) as u8;
                self.write_byte(bus, addr, self.a & self.x & high.wrapping_add(1));
            }
            (Opcode::ALR, Some(addr)) => {
                // ALR #imm = AND #imm followed by LSR A.
                self.a &= self.read_byte(bus, addr);
                let wide = self.a as u16;
                self.a = (wide >> 1) as u8;
                self.write_status_bit(StatusFlags::C, wide & 1 != 0);
                self.set_nz(self.a);
            }
            (Opcode::ANC, Some(addr)) => {
                // ANC #imm = AND #imm and copy the result's sign bit to C.
                self.a &= self.read_byte(bus, addr);
                self.set_nz(self.a);
                self.write_status_bit(StatusFlags::C, self.check_status_bit(StatusFlags::N));
            }
            (Opcode::AND, Some(addr)) => {
                self.a &= self.read_byte(bus, addr);
                self.set_nz(self.a);
            }
            (Opcode::ARR, Some(addr)) => {
                // ARR #imm = AND #imm followed by ROR A. Decimal mode is not
                // used by the NES, so the binary-mode flags are sufficient.
                self.a &= self.read_byte(bus, addr);
                let carry = self.check_status_bit(StatusFlags::C) as u8;
                self.a = (self.a >> 1) | (carry << 7);
                self.set_nz(self.a);
                self.write_status_bit(StatusFlags::C, self.a & 0x40 != 0);
                self.write_status_bit(
                    StatusFlags::V,
                    ((self.a & 0x40) != 0) ^ ((self.a & 0x20) != 0),
                );
            }
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
            (Opcode::AXS, Some(addr)) => {
                // AXS/SBX #imm = (A & X) - imm.
                let operand = self.read_byte(bus, addr);
                let base = self.a & self.x;
                let result = base.wrapping_sub(operand);
                self.write_status_bit(StatusFlags::C, base >= operand);
                self.x = result;
                self.set_nz(self.x);
            }
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
            (Opcode::LAS, Some(addr)) => {
                let value = self.read_byte(bus, addr) & self.sp;
                self.a = value;
                self.x = value;
                self.sp = value;
                self.set_nz(value);
            }
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
            (Opcode::SHX, Some(addr)) => {
                let high = (addr >> 8) as u8;
                self.write_byte(bus, addr, self.x & high.wrapping_add(1));
            }
            (Opcode::SHY, Some(addr)) => {
                let high = (addr >> 8) as u8;
                self.write_byte(bus, addr, self.y & high.wrapping_add(1));
            }
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
            (Opcode::TAS, Some(addr)) => {
                self.sp = self.a & self.x;
                let high = (addr >> 8) as u8;
                self.write_byte(bus, addr, self.sp & high.wrapping_add(1));
            }
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
            (Opcode::XAA, Some(addr)) => {
                // XAA is unstable on real hardware. X & imm is the
                // deterministic behavior used by common NES test ROMs.
                self.a = self.x & self.read_byte(bus, addr);
                self.set_nz(self.a);
            }
            // Every decoded opcode should have a matching addressing mode.
            // Treat a malformed/unsupported combination as a no-op instead
            // of trapping the entire WASM animation loop.
            _ => {}
        }
    }

    pub(crate) fn read_byte(&self, bus: &MemoryBus, addr: u16) -> u8 {
        // https://www.nesdev.org/wiki/CPU_memory_map
        match addr {
            0x0000..=0x1fff => self.ram[addr as usize % self.ram.len()],
            0x2000..=0x3fff => bus.ppu.read_register(&bus.mapper, addr), // PPU
            0x4000..=0x4013 | 0x4015 => bus.apu.read_register(addr),     // APU
            0x4014 => 0,                                                 // DMA
            0x4016 => bus.controller.read(),                             // controller 1
            0x4017 => 0,                                                 // controller 2
            0x4018..=0x401F => 0,                                        // disabled test mode
            _ => bus.mapper.read(addr),
        }
    }

    fn read_page<'a, M: Mapper>(&'a self, mapper: &'a M, page: u8) -> Option<&'a [u8; 256]> {
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

    #[inline]
    fn read_code_byte(&self, bus: &MemoryBus, addr: u16) -> u8 {
        if addr >= 0x8000 {
            if let Some(page) = bus.mapper.read_page((addr >> 8) as u8) {
                return page[(addr & 0xff) as usize];
            }
        }
        self.read_byte(bus, addr)
    }

    #[inline]
    fn read_code_address(&self, bus: &MemoryBus, addr: u16) -> u16 {
        let lo = self.read_code_byte(bus, addr);
        let hi = self.read_code_byte(bus, addr.wrapping_add(1));

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
            0x2000..=0x3fff => bus.ppu.write_register(&mut bus.mapper, addr, data), // PPU
            0x4000..=0x4013 | 0x4015 | 0x4017 => bus.apu.write_register(addr, data), // APU
            0x4014 => {
                let page = self.read_page(&bus.mapper, data);
                bus.ppu.write_dma(page);
            } // DMA
            0x4016 => bus.controller.write(data),                                   // controller 1
            0x4018..=0x401F => {} // disabled test mode
            _ => {
                bus.mapper.write(addr, data);
                bus.ppu.refresh_nametable_mirroring(&bus.mapper);
            }
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
        let opcode = self.read_code_byte(bus, addr);
        let extended_opcode = &EXTENDED_OPCODES[opcode as usize];

        match extended_opcode.addressing_mode {
            AddressingMode::Absolute => {
                let address = self.read_code_address(bus, operand_addr);
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
                let indirect = self.read_code_address(bus, operand_addr);
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
                let indirect = self.read_code_address(bus, operand_addr);
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
                let offset = self.read_code_byte(bus, operand_addr);
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
                let indirect = self.read_code_address(bus, operand_addr);
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
                let offset = self.read_code_byte(bus, operand_addr);
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
                let offset = self.read_code_byte(bus, operand_addr);
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
                let address = self.read_code_byte(bus, operand_addr);
                DecodedInstruction {
                    extended_opcode,
                    address_info: AddressInfo::ZeroPage { address: address },
                    width: 2,
                    page_boundary_hit: false,
                    final_address: Some(address as u16),
                }
            }
            AddressingMode::ZeroPageIndexedX => {
                let offset = self.read_code_byte(bus, operand_addr);
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
                let offset = self.read_code_byte(bus, operand_addr);
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

    #[inline]
    fn decode_compact(&self, bus: &MemoryBus, addr: u16) -> CompactDecodedInstruction {
        let opcode = self.read_code_byte(bus, addr);
        self.decode_compact_opcode(bus, addr, opcode)
    }

    #[inline]
    fn decode_compact_opcode(
        &self,
        bus: &MemoryBus,
        addr: u16,
        opcode: u8,
    ) -> CompactDecodedInstruction {
        let extended_opcode = &EXTENDED_OPCODES[opcode as usize];
        self.decode_compact_with_metadata::<false>(
            bus,
            addr,
            extended_opcode.opcode,
            extended_opcode.addressing_mode,
            extended_opcode.min_cycles,
            extended_opcode.page_boundary_penalty,
            [0; 2],
        )
    }

    #[cfg(feature = "timestamped-scheduler")]
    #[inline]
    fn decode_compact_static(
        &self,
        bus: &MemoryBus,
        addr: u16,
        instruction: TimestampedStaticInstruction,
    ) -> CompactDecodedInstruction {
        self.decode_compact_with_metadata::<true>(
            bus,
            addr,
            instruction.opcode,
            instruction.addressing_mode,
            instruction.min_cycles,
            instruction.page_boundary_penalty,
            instruction.operand,
        )
    }

    #[inline]
    fn decode_compact_with_metadata<const CACHED_OPERAND: bool>(
        &self,
        bus: &MemoryBus,
        addr: u16,
        opcode: Opcode,
        addressing_mode: AddressingMode,
        min_cycles: u8,
        page_boundary_penalty: bool,
        cached_operand: [u8; 2],
    ) -> CompactDecodedInstruction {
        let operand_addr = addr.wrapping_add(1);
        let read_operand_byte = |offset: usize| {
            if CACHED_OPERAND {
                cached_operand[offset]
            } else {
                self.read_code_byte(bus, operand_addr.wrapping_add(offset as u16))
            }
        };
        let read_operand_address =
            || u16::from_le_bytes([read_operand_byte(0), read_operand_byte(1)]);
        let (width, final_address, base_address, page_boundary_hit) = match addressing_mode {
            AddressingMode::Absolute => (3, Some(read_operand_address()), None, false),
            AddressingMode::Implied | AddressingMode::Accumulator => (1, None, None, false),
            AddressingMode::AbsoluteIndexedX => {
                let indirect = read_operand_address();
                let address = indirect.wrapping_add(self.x as u16);
                (
                    3,
                    Some(address),
                    Some(indirect),
                    page_boundary_penalty && crosses_page_boundary(indirect, address),
                )
            }
            AddressingMode::AbsoluteIndexedY => {
                let indirect = read_operand_address();
                let address = indirect.wrapping_add(self.y as u16);
                (
                    3,
                    Some(address),
                    Some(indirect),
                    page_boundary_penalty && crosses_page_boundary(indirect, address),
                )
            }
            AddressingMode::Immediate => (2, Some(operand_addr), None, false),
            AddressingMode::IndexedIndirect => {
                let offset = read_operand_byte(0);
                let indirect = offset.wrapping_add(self.x) as u16;
                (
                    2,
                    Some(self.read_address_indirect(bus, indirect)),
                    Some(indirect),
                    false,
                )
            }
            AddressingMode::Indirect => {
                let indirect = read_operand_address();
                (
                    3,
                    Some(self.read_address_indirect(bus, indirect)),
                    Some(indirect),
                    false,
                )
            }
            AddressingMode::IndirectIndexed => {
                let offset = read_operand_byte(0);
                let indirect = self.read_address_indirect(bus, offset as u16);
                let address = indirect.wrapping_add(self.y as u16);
                (
                    2,
                    Some(address),
                    Some(indirect),
                    page_boundary_penalty && crosses_page_boundary(indirect, address),
                )
            }
            AddressingMode::Relative => {
                let offset = read_operand_byte(0);
                let next_pc = addr.wrapping_add(2);
                let address = if offset >= 0x80 {
                    next_pc.wrapping_sub(0x100 - offset as u16)
                } else {
                    next_pc.wrapping_add(offset as u16)
                };
                (2, Some(address), None, false)
            }
            AddressingMode::ZeroPage => (2, Some(read_operand_byte(0) as u16), None, false),
            AddressingMode::ZeroPageIndexedX => {
                let offset = read_operand_byte(0);
                (2, Some(offset.wrapping_add(self.x) as u16), None, false)
            }
            AddressingMode::ZeroPageIndexedY => {
                let offset = read_operand_byte(0);
                (2, Some(offset.wrapping_add(self.y) as u16), None, false)
            }
        };

        CompactDecodedInstruction {
            opcode,
            addressing_mode,
            final_address,
            base_address,
            width,
            min_cycles,
            page_boundary_hit,
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

    #[cfg(feature = "timestamped-scheduler")]
    fn preflight_bus(program: &[u8]) -> crate::bus::MemoryBus {
        use crate::cartridge::{Cartridge, MapperInstance, MirroringMode, CHR, PRG};
        use std::rc::Rc;

        let mut prg = vec![[0; 0x4000]];
        prg[0][..program.len()].copy_from_slice(program);
        crate::bus::MemoryBus::new(MapperInstance::new_nrom(Cartridge {
            prg: Rc::new(PRG { banks: prg }),
            chr: CHR::ROM(Rc::new(vec![[0; 0x2000]])),
            sram: vec![[0; 0x2000]],
            mirror: MirroringMode::Horizontal,
        }))
    }

    #[test]
    fn test_debug_log() {
        if !std::path::Path::new("tests/nestest.nes").exists() {
            return;
        }

        let mut log_file = std::fs::File::create("tests/nestest.log").unwrap();
        let mut rom_file = std::fs::File::open("tests/nestest.nes").unwrap();
        let (c, m) = ines::load(&mut rom_file).expect("failed to load cartridge");

        let console = Console::new(cartridge::new(c, m).unwrap());
        let mut state = console.snapshot();
        state.cpu.pc = 0xc000;

        // match offset for nestest.nes
        state.cpu.cycles = 7;

        for _ in 0..8991 {
            state.cpu.step(&mut state.bus, Some(&mut log_file));
        }
    }

    #[cfg(feature = "timestamped-scheduler")]
    #[test]
    fn timestamp_preflight_covers_fetches_effective_addresses_and_control_flow() {
        let mut cpu = super::CPU::default();

        let bus = preflight_bus(&[0xa9, 0x42]); // LDA #$42
        cpu.pc = 0x8000;
        assert!(!cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0xad, 0x00, 0x20]); // LDA $2000
        cpu.pc = 0x8000;
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0xbd, 0xff, 0x1f]); // LDA $1fff,X
        cpu.pc = 0x8000;
        cpu.x = 1;
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0xbd, 0xff, 0x3f]); // LDA $3fff,X
        cpu.pc = 0x8000;
        cpu.x = 1; // final $4000, page-crossing dummy read mirrors to $3f00
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0xb9, 0xff, 0x1f]); // LDA $1fff,Y
        cpu.pc = 0x8000;
        cpu.y = 1;
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0xa1, 0x10]); // LDA ($10,X)
        cpu.pc = 0x8000;
        cpu.x = 0;
        cpu.ram[0x10] = 0x00;
        cpu.ram[0x11] = 0x20;
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0xb1, 0x10]); // LDA ($10),Y
        bus.ppu.last_read.set(None);
        cpu.pc = 0x8000;
        cpu.x = 0;
        cpu.y = 0;
        cpu.ram[0x10] = 0x00;
        cpu.ram[0x11] = 0x20;
        assert!(cpu.next_instruction_may_access_ppu(&bus));
        assert_eq!(bus.ppu.last_read.get(), None);

        let bus = preflight_bus(&[0x6c, 0x00, 0x20]); // JMP ($2000)
        cpu.pc = 0x8000;
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0x4c, 0x00, 0x20]); // JMP $2000
        cpu.pc = 0x8000;
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0x60]); // RTS: return target is stack-dependent
        cpu.pc = 0x8000;
        assert!(cpu.next_instruction_may_access_ppu(&bus));

        let bus = preflight_bus(&[0xa9, 0x42]);
        cpu.pc = 0x7fff; // opcode fetch itself is not cartridge-backed
        assert!(cpu.next_instruction_may_access_ppu(&bus));
    }

    #[cfg(feature = "timestamped-scheduler")]
    #[test]
    fn timestamped_prepare_reuses_safe_decode_and_defers_io_decode() {
        let mut cpu = super::CPU::default();
        let mut bus = preflight_bus(&[0xa9, 0x42]); // LDA #$42
        cpu.pc = 0x8000;
        assert!(!cpu.prepare_timestamped(&bus));
        assert_eq!(cpu.step_timestamped(&mut bus), 2);
        assert_eq!(cpu.a, 0x42);

        let mut cpu = super::CPU::default();
        let mut bus = preflight_bus(&[0xad, 0x00, 0x20]); // LDA $2000
        cpu.pc = 0x8000;
        assert!(cpu.prepare_timestamped(&bus));
        assert_eq!(bus.ppu.last_read.get(), None);
        assert_eq!(cpu.step_timestamped(&mut bus), 4);
        assert_eq!(bus.ppu.last_read.get(), Some(0x2000));

        let mut cpu = super::CPU::default();
        let bus = preflight_bus(&[0x8d, 0x14, 0x40]); // STA $4014 (OAM DMA)
        cpu.pc = 0x8000;
        assert!(cpu.prepare_timestamped(&bus));
        assert_eq!(bus.ppu.last_read.get(), None);

        let mut cpu = super::CPU::default();
        let mut bus = preflight_bus(&[0x6c, 0x00, 0x20]); // JMP ($2000)
        cpu.pc = 0x8000;
        assert!(cpu.prepare_timestamped(&bus));
        assert_eq!(bus.ppu.last_read.get(), None);
        cpu.step_timestamped(&mut bus);
        assert_eq!(bus.ppu.last_read.get(), Some(0x2001));

        let mut cpu = super::CPU::default();
        let mut bus = preflight_bus(&[0xa9, 0x42]);
        bus.ppu.in_vblank = true;
        bus.ppu.write_register(&mut bus.mapper, 0x2000, 0x80);
        cpu.pc = 0x8000;
        cpu.prepare_timestamped(&bus);
        assert_eq!(cpu.step_timestamped(&mut bus), 7);
        assert_eq!(cpu.pc, 0);
    }

    #[cfg(feature = "timestamped-scheduler")]
    #[test]
    fn timestamped_block_matches_reference_at_ppu_barrier() {
        let program = [0xa9, 0x42, 0xad, 0x00, 0x20]; // LDA #$42; LDA $2000
        let mut block_cpu = super::CPU::default();
        let mut block_bus = preflight_bus(&program);
        block_cpu.pc = 0x8000;

        let mut reference_cpu = super::CPU::default();
        let mut reference_bus = preflight_bus(&program);
        reference_cpu.pc = 0x8000;

        let (safe_cycles, ppu_barrier) = block_cpu.run_timestamped_block(&mut block_bus, u64::MAX);
        assert_eq!(safe_cycles, 2);
        assert!(ppu_barrier);
        let reference_first_cycles = reference_cpu.step(&mut reference_bus, None);
        assert_eq!(block_cpu.a, reference_cpu.a);
        assert_eq!(block_cpu.pc, 0x8002);

        let barrier_cycles = block_cpu.step_timestamped(&mut block_bus);
        let reference_cycles =
            reference_first_cycles + reference_cpu.step(&mut reference_bus, None);

        assert_eq!(barrier_cycles + safe_cycles, reference_cycles);
        assert_eq!(block_cpu.pc, reference_cpu.pc);
        assert_eq!(block_cpu.a, reference_cpu.a);
        assert_eq!(block_cpu.status, reference_cpu.status);
        assert_eq!(block_cpu.sp, reference_cpu.sp);
        assert_eq!(block_cpu.cycles, reference_cpu.cycles);
        assert_eq!(block_cpu.ram, reference_cpu.ram);
        assert_eq!(
            block_bus.ppu.last_read.get(),
            reference_bus.ppu.last_read.get()
        );
    }

    #[cfg(feature = "timestamped-scheduler")]
    #[test]
    fn timestamped_cached_decode_matches_reference_across_addressing_modes() {
        let program = [
            0xa9, 0x10, // LDA #$10
            0x85, 0x00, // STA $00
            0xa2, 0x01, // LDX #$01
            0x9d, 0xff, 0x00, // STA $00ff,X
            0xa0, 0x01, // LDY #$01
            0xa1, 0x10, // LDA ($10,X)
            0x69, 0x01, // ADC #$01
            0x81, 0x10, // STA ($10,X)
            0xb1, 0x20, // LDA ($20),Y
            0xc9, 0x11, // CMP #$11
            0x48, // PHA
            0x68, // PLA
            0xe8, // INX
            0x88, // DEY
            0xea, // NOP
            0xad, 0x00, 0x20, // LDA $2000
        ];
        let mut block_cpu = super::CPU::default();
        let mut block_bus = preflight_bus(&program);
        let mut reference_cpu = super::CPU::default();
        let mut reference_bus = preflight_bus(&program);
        block_cpu.pc = 0x8000;
        reference_cpu.pc = 0x8000;

        for cpu in [&mut block_cpu, &mut reference_cpu] {
            cpu.ram[0x11] = 0x00;
            cpu.ram[0x12] = 0x02;
            cpu.ram[0x20] = 0xff;
            cpu.ram[0x21] = 0x01;
            cpu.ram[0x200] = 0x10;
        }

        let (safe_cycles, ppu_barrier) = block_cpu.run_timestamped_block(&mut block_bus, u64::MAX);
        assert!(ppu_barrier);
        for _ in 0..15 {
            reference_cpu.step(&mut reference_bus, None);
        }

        assert_eq!(safe_cycles, reference_cpu.cycles as u16);
        assert_eq!(block_cpu.pc, reference_cpu.pc);
        assert_eq!(block_cpu.a, reference_cpu.a);
        assert_eq!(block_cpu.x, reference_cpu.x);
        assert_eq!(block_cpu.y, reference_cpu.y);
        assert_eq!(block_cpu.status, reference_cpu.status);
        assert_eq!(block_cpu.sp, reference_cpu.sp);
        assert_eq!(block_cpu.ram, reference_cpu.ram);

        let barrier_cycles = block_cpu.step_timestamped(&mut block_bus);
        let reference_cycles = reference_cpu.step(&mut reference_bus, None);
        assert_eq!(barrier_cycles, reference_cycles);
        assert_eq!(block_cpu.pc, reference_cpu.pc);
        assert_eq!(block_cpu.a, reference_cpu.a);
        assert_eq!(block_cpu.status, reference_cpu.status);
        assert_eq!(block_cpu.cycles, reference_cpu.cycles);
        assert_eq!(
            block_bus.ppu.last_read.get(),
            reference_bus.ppu.last_read.get()
        );
    }
}

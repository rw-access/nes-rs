//! NESdev hardware contracts for the 2026-11-17 audit.
//! Ignored tests are known gaps, not accepted golden behavior. Run explicitly
//! with `cargo test -p nes-core cartridge::audit -- --ignored --nocapture`.
//! See docs/mapper-audit.md for sources, confidence and interpretation.
use super::*;
use crate::{bus::MemoryBus, cpu::CPU, ines, ppu::PPU};
use std::io::Cursor;

fn tagged_cart(prg_16k: usize, chr_8k: usize) -> Cartridge {
    let mut prg = vec![[0; 0x4000]; prg_16k];
    for (bank, bytes) in prg.iter_mut().enumerate() {
        for (half, bytes) in bytes.chunks_mut(0x2000).enumerate() {
            bytes.fill((bank * 2 + half) as u8);
        }
    }
    let mut chr = vec![[0; 0x2000]; chr_8k];
    for (bank, bytes) in chr.iter_mut().enumerate() {
        for (page, bytes) in bytes.chunks_mut(0x400).enumerate() {
            bytes.fill((bank * 8 + page) as u8);
        }
    }
    Cartridge {
        submapper: None,
        prg: Rc::new(PRG { banks: prg }),
        chr: CHR::ROM(Rc::new(chr)),
        sram: vec![[0xa0; 0x2000], [0xa1; 0x2000]],
        mirror: MirroringMode::Vertical,
    }
}

fn mapper(id: u8) -> Box<dyn Mapper> {
    new(tagged_cart(8, 16), id).unwrap()
}
fn serial(mapper: &mut dyn Mapper, address: u16, value: u8) {
    for bit in 0..5 {
        mapper.write(address, (value >> bit) & 1);
    }
}
fn image_header(mapper: u8, nes2: bool) -> Vec<u8> {
    let mut bytes = vec![0; 16 + 0x8000];
    bytes[..4].copy_from_slice(b"NES\x1a");
    bytes[4] = 2;
    bytes[6] = mapper << 4;
    bytes[7] = (mapper & 0xf0) | if nes2 { 8 } else { 0 };
    if nes2 {
        bytes[11] = 7;
    } // Explicit 8 KiB CHR RAM.
    bytes
}

#[test]
fn fast_prg_pages_agree_with_bus_reads_for_all_factory_ids() {
    let mut ids = Vec::new();
    for id in 0..=255 {
        let Some(mapper) = new(tagged_cart(8, 4), id) else {
            continue;
        };
        ids.push(id);
        for page in 0x80..=0xff {
            if let Some(bytes) = mapper.read_page(page) {
                for offset in [0, 0x7f, 0xff] {
                    assert_eq!(
                        bytes[offset],
                        mapper.read((u16::from(page) << 8) | offset as u16),
                        "mapper {id}, page {page:02x}"
                    );
                }
            }
        }
    }
    assert_eq!(
        ids.len(),
        27,
        "update the audit matrix when factories change"
    );
}

#[test]
fn fme7_irq_wraps_down_and_acknowledges_on_control_write() {
    let mut mapper = mapper(69);
    for (command, data) in [(14, 0), (15, 0), (13, 0x81)] {
        mapper.write(0x8000, command);
        mapper.write(0xa000, data);
    }
    mapper.clock_cpu();
    assert!(mapper.irq_pending());
    mapper.write(0x8000, 13);
    mapper.write(0xa000, 0);
    assert!(!mapper.irq_pending());
}

#[test]
fn namco_irq_saturates_and_reads_do_not_acknowledge() {
    let mut mapper = mapper(19);
    mapper.write(0x5000, 0xfe);
    mapper.write(0x5800, 0xff);
    mapper.clock_cpu();
    assert!(mapper.irq_pending());
    assert_eq!(mapper.read(0x5000), 0xff);
    assert_eq!(mapper.read(0x5800), 0xff);
    assert!(mapper.irq_pending());
    mapper.clock_cpu();
    assert_eq!(mapper.read(0x5000), 0xff);
    mapper.write(0x5000, 0);
    assert!(!mapper.irq_pending());
}

#[test]
fn gap_g01_mmc4_upper_half_of_selected_prg_bank() {
    let mut mapper = mapper(10);
    mapper.write(0xa000, 2);
    assert_eq!(mapper.read(0x8000), 4);
    assert_eq!(mapper.read(0xa000), 5);
    assert_eq!(mapper.read_page(0xa0).unwrap()[0], 5);
}

#[test]
fn gap_g02_mmc2_does_not_latch_on_0fd9() {
    let mut mapper = mapper(9);
    mapper.write(0xb000, 0);
    mapper.write(0xc000, 1);
    mapper.read(0x0fe8);
    assert_eq!(mapper.read(0), 4);
    mapper.read(0x0fd9);
    assert_eq!(mapper.read(0), 4);
}

#[test]
fn gap_g03_mmc2_latch_changes_after_triggering_read() {
    let mut mapper = mapper(9);
    mapper.write(0xb000, 0);
    mapper.write(0xc000, 1);
    assert_eq!(mapper.read(0x0fe8), 3);
    assert_eq!(mapper.read(0), 4);
}

#[test]
fn gap_g04_mmc4_latch_changes_after_triggering_read() {
    let mut mapper = mapper(10);
    mapper.write(0xb000, 0);
    mapper.write(0xc000, 1);
    assert_eq!(mapper.read(0x0fe8), 3);
    assert_eq!(mapper.read(0), 4);
}

#[test]
fn gap_g05_vrc4_chr_nibbles_compose_on_mapper23() {
    let mut mapper = mapper(23);
    mapper.write(0xb000, 0x0a);
    mapper.write(0xb001, 3);
    assert_eq!(mapper.read(0), 0x3a);
}

#[test]
fn gap_g06_vrc2a_chr_nibbles_compose_before_shift() {
    let mut mapper = mapper(22);
    mapper.write(0xb000, 0x0a);
    mapper.write(0xb002, 3);
    assert_eq!(mapper.read(0), 0x1d);
}

#[test]
fn gap_g07_vrc4b_mapper25_chr_high_nibble_address() {
    let mut mapper = mapper(25);
    mapper.write(0xb000, 0x0a);
    mapper.write(0xb002, 3);
    assert_eq!(mapper.read(0), 0x3a);
}

#[test]
fn gap_g08_vrc4c_uses_address_bits_6_and_7() {
    let mut mapper = mapper(21);
    mapper.write(0xb000, 0x0a);
    mapper.write(0xb040, 3);
    assert_eq!(mapper.read(0), 0x3a);
}

#[test]
fn gap_g09_vrc4_cpu_irq_counts_up() {
    let mut mapper = mapper(23);
    mapper.write(0xf000, 0x0f);
    mapper.write(0xf001, 0x0f);
    mapper.write(0xf002, 6);
    mapper.clock_cpu();
    assert!(mapper.irq_pending());
}

#[test]
fn gap_g10_vrc4_prescaler_runs_without_ppu_scanline_calls() {
    let mut mapper = mapper(23);
    mapper.write(0xf000, 0x0f);
    mapper.write(0xf001, 0x0f);
    mapper.write(0xf002, 2);
    for _ in 0..114 {
        mapper.clock_cpu();
    }
    assert!(mapper.irq_pending());
}

#[test]
fn gap_g11_vrc4_ack_uses_saved_enable_after_ack() {
    let mut mapper = VRC::new(tagged_cart(8, 32), 23);
    mapper.write(0xf002, 7); // Save A=1, E=1, cycle mode.
    mapper.write(0xf003, 0); // Data is ignored by hardware.
    assert!(mapper.irq_enabled);
}

#[test]
fn gap_g12_vrc6_mapper24_irq_register_layout() {
    let mut mapper = mapper(24);
    mapper.write(0xf000, 0xff);
    mapper.write(0xf001, 6);
    mapper.clock_cpu();
    assert!(mapper.irq_pending());
}

#[test]
fn gap_g13_vrc6_mapper26_irq_register_layout() {
    let mut mapper = mapper(26);
    mapper.write(0xf000, 0xff);
    mapper.write(0xf002, 6);
    mapper.clock_cpu();
    assert!(mapper.irq_pending());
}

#[test]
fn gap_g14_namco_data_reads_auto_increment() {
    let mut mapper = mapper(19);
    mapper.write(0xf800, 0x7e);
    mapper.write(0x4800, 0xaa);
    mapper.write(0xf800, 0x7f);
    mapper.write(0x4800, 0xbb);
    mapper.write(0xf800, 0xfe);
    assert_eq!(mapper.read(0x4800), 0xaa);
    assert_eq!(mapper.read(0x4800), 0xbb);
}

#[test]
fn gap_g15_namco_data_address_saturates_at_7f() {
    let mut mapper = mapper(19);
    mapper.write(0xf800, 0xff);
    mapper.write(0x4800, 0xaa);
    mapper.write(0x4800, 0xbb);
    mapper.write(0xf800, 0x7f);
    assert_eq!(mapper.read(0x4800), 0xbb);
}

#[test]
fn gap_g16_namco_nametable_ciram_selection() {
    let mut mapper = mapper(19);
    mapper.write(0xc000, 0xe0);
    mapper.write(0xc800, 0xe1);
    let mut ppu = PPU::default();
    ppu.refresh_nametable_mirroring(mapper.as_ref());
    ppu.write_byte(mapper.as_mut(), 0x2000, 0xaa);
    ppu.write_byte(mapper.as_mut(), 0x2400, 0xbb);
    mapper.write(0xc000, 0xe1);
    ppu.refresh_nametable_mirroring(mapper.as_ref());
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0x2000), 0xbb);
}

#[test]
fn gap_g17_namco_pattern_bank_can_reference_ciram() {
    let mut mapper = mapper(19);
    mapper.write(0xc000, 0xe0);
    mapper.write(0xc800, 0xe1);
    let mut ppu = PPU::default();
    ppu.refresh_nametable_mirroring(mapper.as_ref());
    ppu.write_byte(mapper.as_mut(), 0x2400, 0xbb);
    mapper.write(0xe800, 0);
    mapper.write(0x8000, 0xe1);
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0), 0xbb);
}

#[test]
fn gap_g18_mmc1_inc_ff_reset_does_not_shift_a_zero() {
    let mut cart = tagged_cart(8, 1);
    let prg = Rc::make_mut(&mut cart.prg);
    prg.banks[7][..3].copy_from_slice(&[0xee, 0x00, 0xe1]); // INC $E100
    prg.banks[7][0x2100] = 0xff;
    prg.banks[7][0x3ffc..0x3ffe].copy_from_slice(&0xc000u16.to_le_bytes());
    let mut bus = MemoryBus::new(MapperInstance::Dynamic(new(cart, 1).unwrap()));
    let mut cpu = CPU::default();
    cpu.reset(&mut bus);
    cpu.step(&mut bus, None);
    serial(&mut bus.mapper, 0xe000, 3);
    assert_eq!(bus.mapper.read(0x8000), 6); // 16 KiB bank 3, low 8 KiB page.
}

#[test]
fn gap_g19_mmc1_surom_outer_prg_region() {
    let mut cart = tagged_cart(32, 1);
    cart.chr = CHR::RAM(vec![[0; 0x2000]]);
    let mut mapper = new(cart, 1).unwrap();
    serial(mapper.as_mut(), 0xa000, 0x10);
    serial(mapper.as_mut(), 0xe000, 0);
    assert_eq!(mapper.read(0x8000), 32);
}

#[test]
fn gap_g20_mmc3_ram_write_protection() {
    let mut mapper = mapper(4);
    mapper.write(0xa001, 0x80);
    mapper.write(0x6000, 0x55);
    mapper.write(0xa001, 0xc0);
    mapper.write(0x6000, 0xaa);
    assert_eq!(mapper.read(0x6000), 0x55);
}

#[test]
fn gap_g21_mmc5_all_nametables_can_use_ciram_zero() {
    let mut mapper = mapper(5);
    mapper.write(0x5105, 0x44); // Explicit initial CIRAM0/1 arrangement.
    let mut ppu = PPU::default();
    ppu.refresh_nametable_mirroring(mapper.as_ref());
    ppu.write_byte(mapper.as_mut(), 0x2000, 0xaa);
    ppu.write_byte(mapper.as_mut(), 0x2400, 0xbb);
    mapper.write(0x5105, 0);
    ppu.refresh_nametable_mirroring(mapper.as_ref());
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0x2400), 0xaa);
}

#[test]
fn gap_g22_mmc5_low_prg_ram_banking() {
    let mut mapper = mapper(5);
    mapper.write(0x5113, 1);
    assert_eq!(mapper.read(0x6000), 0xa1);
}

#[test]
fn gap_g23_mmc5_upper_prg_ram_window() {
    let mut mapper = mapper(5);
    mapper.write(0x5100, 3);
    mapper.write(0x5102, 2);
    mapper.write(0x5103, 1);
    mapper.write(0x5114, 1);
    mapper.write(0x8000, 0xcc);
    assert_eq!(mapper.read(0x8000), 0xcc);
}

#[test]
fn gap_g24_mmc5_exram_cpu_access() {
    let mut mapper = mapper(5);
    mapper.write(0x5104, 2);
    mapper.write(0x5c10, 0x55);
    assert_eq!(mapper.read(0x5c10), 0x55);
}

#[test]
fn gap_g25_mmc5_fill_nametable() {
    let mut mapper = mapper(5);
    let mut ppu = PPU::default();
    mapper.write(0x5105, 0xff);
    mapper.write(0x5106, 0x2a);
    mapper.write(0x5107, 3);
    ppu.refresh_nametable_mirroring(mapper.as_ref());
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0x2000), 0x2a);
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0x23c0), 0xff);
}

#[test]
fn gap_g26_mapper15_mode3_updates_both_prg_halves() {
    let mut mapper = mapper(15);
    mapper.write(0x8003, 2);
    for (address, expected) in [(0x8000, 4), (0xa000, 5), (0xc000, 4), (0xe000, 5)] {
        assert_eq!(mapper.read(address), expected);
    }
}

#[test]
fn gap_g27_mapper15_mode1_lower_prg_selection() {
    let mut mapper = mapper(15);
    mapper.write(0x8001, 2);
    assert_eq!(mapper.read(0x8000), 4);
    assert_eq!(mapper.read(0xc000), 14);
}

#[test]
#[ignore = "G28: mapper15 hardware CHR protection intentionally conflicts with some hacks"]
fn gap_g28_mapper15_real_board_chr_write_protection() {
    let mut cart = tagged_cart(8, 1);
    cart.chr = CHR::RAM(vec![[0; 0x2000]]);
    let mut mapper = new(cart, 15).unwrap();
    mapper.write(0x8000, 0);
    mapper.write(0, 0x55);
    assert_eq!(mapper.read(0), 0);
}

#[test]
fn gap_g29_nina001_banks_chr_in_two_4k_windows() {
    let mut mapper = new(tagged_cart(4, 8), 34).unwrap();
    mapper.write(0x7ffe, 2);
    mapper.write(0x7fff, 3);
    assert_eq!(mapper.read(0), 8);
    assert_eq!(mapper.read(0x1000), 12);
}

#[test]
fn gap_g30_oeka_kids_prg_bank_selection() {
    let mut cart = tagged_cart(8, 1);
    cart.chr = CHR::RAM(vec![[0; 0x2000]; 4]);
    let mut mapper = new(cart, 96).unwrap();
    mapper.write(0xffff, 2); // Current ROM byte 3 allows both PRG select bits.
    assert_eq!(mapper.read(0x8000), 8);
}

#[test]
fn gap_g31_oeka_kids_chr_ram_capacity() {
    let mut bytes = image_header(96, false);
    bytes[4] = 8;
    bytes.resize(16 + 0x20000, 0);
    let (cart, _) = ines::load(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(cart.chr.get_banks().len(), 4);
}

#[test]
#[ignore = "G32: mapper224 needs MINDKIDS outer registers at 5000"]
fn gap_g32_kt008_outer_prg_bank() {
    let mut mapper = new(tagged_cart(32, 1), 224).unwrap();
    mapper.write(0x8000, 6);
    mapper.write(0x8001, 0);
    mapper.write(0x5000, 0x41); // Force PRG A17 to offset EEE=1.
    assert_eq!(mapper.read(0x8000), 16);
}

#[test]
#[ignore = "G33: mapper20 factory is a load-only NROM placeholder, not an FDS"]
fn gap_g33_fds_ram_is_writable_above_8000() {
    let mut mapper = mapper(20);
    mapper.write(0x8000, 0x55);
    assert_eq!(mapper.read(0x8000), 0x55);
}

#[test]
fn gap_g34_nes2_submapper_does_not_allocate_prg_ram() {
    let mut bytes = image_header(2, true);
    bytes[8] = 0x20; // Submapper 2, no PRG RAM.
    let (cart, _) = ines::load(&mut Cursor::new(bytes)).unwrap();
    assert!(cart.sram.is_empty());
}

#[test]
fn gap_g35_nes2_32k_chr_ram_allocation() {
    let mut bytes = image_header(96, true);
    bytes[4] = 8;
    bytes.resize(16 + 0x20000, 0);
    bytes[11] = 9; // 64 << 9 = 32768 bytes.
    let (cart, _) = ines::load(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(cart.chr.get_banks().len(), 4);
}

#[test]
fn gap_g36_trainer_is_available_at_cpu_7000() {
    let mut bytes = image_header(1, false);
    bytes[6] |= 4;
    bytes.splice(16..16, vec![0x5a; 512]);
    let (cart, id) = ines::load(&mut Cursor::new(bytes)).unwrap();
    let mapper = new(cart, id).unwrap();
    assert_eq!(mapper.read(0x7000), 0x5a);
}

#[test]
fn vrc_chr_all_windows_and_pin_layouts_preserve_nibbles() {
    for (id, submapper, pins) in [
        (21, None, [0, 2, 4, 6]),
        (21, None, [0, 0x40, 0x80, 0xc0]),
        (22, None, [0, 2, 1, 3]),
        (23, None, [0, 1, 2, 3]),
        (23, Some(2), [0, 4, 8, 12]),
        (25, None, [0, 2, 1, 3]),
        (25, Some(2), [0, 8, 4, 12]),
    ] {
        let mut cart = tagged_cart(8, 16);
        cart.submapper = submapper;
        let mut mapper = new(cart, id).unwrap();
        for slot in 0..8 {
            let bank = (0x10 + slot) << usize::from(id == 22);
            let base = 0xb000 + (slot as u16 / 2) * 0x1000;
            let low = base + pins[(slot & 1) * 2];
            let high = base + pins[(slot & 1) * 2 + 1];
            // High first, then low: low writes must preserve the high bits.
            mapper.write(high, (bank >> 4) as u8);
            mapper.write(low, (bank & 0xf) as u8);
        }
        for slot in 0..8 {
            assert_eq!(
                mapper.read(slot * 0x400 + 0x3ff),
                0x10 + slot as u8,
                "mapper {id}/{submapper:?}, pins {pins:?}, CHR slot {slot}"
            );
        }
    }
}

#[test]
fn vrc_irq_prescaler_cadence_ack_and_disabled_control() {
    let mut vrc = VRC::new(tagged_cart(8, 1), 23);
    vrc.write(0xf000, 0xf);
    vrc.write(0xf001, 0xf);
    vrc.write(0xf002, 3); // Scanline mode, enabled, reenable after acknowledge.
    for interval in [114, 114, 113, 114] {
        for _ in 1..interval {
            vrc.clock_cpu();
        }
        assert!(!vrc.irq_pending());
        vrc.clock_cpu();
        assert!(vrc.irq_pending());
        vrc.write(0xf003, 0); // Data has no effect and phase must survive.
    }
    vrc.write(0xf002, 6); // CPU mode, no reenable after acknowledge.
    vrc.clock_cpu();
    assert!(vrc.irq_pending());
    vrc.write(0xf003, 0xff);
    assert!(!vrc.irq_enabled);
    assert!(!vrc.irq_pending());
    vrc.irq_counter = 0x42;
    vrc.write(0xf002, 4); // Disabled control must not reload the counter.
    assert_eq!(vrc.irq_counter, 0x42);
}

#[test]
fn mmc1_adjacent_serial_writes_ignore_second_bit_but_never_reset() {
    let mut mapper = mapper(1);
    mapper.write_rmw(0xe000, 1, 0);
    for bit in [1, 0, 0, 0] {
        mapper.write(0xe000, bit);
    }
    assert_eq!(mapper.read(0x8000), 6); // Serial value 3, not 5.
    mapper.write_rmw(0xe000, 0, 0x80);
    serial(mapper.as_mut(), 0xe000, 2);
    assert_eq!(mapper.read(0x8000), 4);
}

#[test]
fn cpu_memory_rmw_emits_original_then_modified_for_all_six_operations() {
    use std::cell::RefCell;
    #[derive(Clone)]
    struct Recorder {
        inner: Box<dyn Mapper>,
        writes: Rc<RefCell<Vec<(u16, u8)>>>,
    }
    impl Mapper for Recorder {
        fn mirror(&self) -> MirroringMode {
            self.inner.mirror()
        }
        fn read(&self, a: u16) -> u8 {
            self.inner.read(a)
        }
        fn read_page(&self, p: u8) -> Option<&[u8; 256]> {
            self.inner.read_page(p)
        }
        fn write(&mut self, a: u16, d: u8) {
            self.writes.borrow_mut().push((a, d));
            self.inner.write(a, d);
        }
    }
    for (opcode, result) in [
        (0x0e, 2),
        (0x4e, 0x40),
        (0x2e, 2),
        (0x6e, 0x40),
        (0xee, 0x82),
        (0xce, 0x80),
    ] {
        let mut cart = tagged_cart(2, 1);
        let prg = Rc::make_mut(&mut cart.prg);
        prg.banks[1][..3].copy_from_slice(&[opcode, 0x00, 0xe1]);
        prg.banks[1][0x2100] = 0x81;
        prg.banks[1][0x3ffc..0x3ffe].copy_from_slice(&0xc000u16.to_le_bytes());
        let writes = Rc::new(RefCell::new(Vec::new()));
        let mut bus = MemoryBus::new(MapperInstance::Dynamic(Box::new(Recorder {
            inner: new(cart, 0).unwrap(),
            writes: writes.clone(),
        })));
        let mut cpu = CPU::default();
        cpu.reset(&mut bus);
        cpu.step(&mut bus, None);
        assert_eq!(
            *writes.borrow(),
            [(0xe100, 0x81), (0xe100, result)],
            "opcode {opcode:02x}"
        );
    }
}

#[test]
fn oeka_latch_uses_address_edges_including_ppuaddr_without_reads() {
    let mut cart = tagged_cart(8, 1);
    Rc::make_mut(&mut cart.prg).banks[1][0x3fff] = 0xff; // Conflict-free latch address.
    cart.chr = CHR::RAM(vec![[0; 0x2000]; 4]);
    let mut mapper = new(cart, 96).unwrap();
    let mut ppu = PPU::default();
    let set_address = |ppu: &mut PPU, mapper: &mut dyn Mapper, address: u16| {
        ppu.write_register(mapper, 0x2006, (address >> 8) as u8);
        ppu.write_register(mapper, 0x2006, address as u8);
    };
    for bank in 0..4 {
        set_address(&mut ppu, mapper.as_mut(), 0);
        set_address(&mut ppu, mapper.as_mut(), 0x2000 + bank * 0x100);
        ppu.write_byte(mapper.as_mut(), 0, 0xa0 + bank as u8);
    }
    set_address(&mut ppu, mapper.as_mut(), 0x2100);
    set_address(&mut ppu, mapper.as_mut(), 0x23c0); // No new edge; attribute address cannot switch it.
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0), 0xa1);
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0x1000), 0xa3);
    mapper.write(0xffff, 4); // Outer 16K region has separate storage.
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0), 0);
    ppu.write_byte(mapper.as_mut(), 0, 0x55);
    mapper.write(0xffff, 0);
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0), 0xa1);
}

#[test]
fn mmc5_ram_fast_pages_protection_and_four_nametable_sources() {
    let mut mapper = mapper(5);
    let mut ppu = PPU::default();
    mapper.write(0x5100, 1); // 16K windows.
    mapper.write(0x5115, 0); // Two distinct 8K RAM banks at 8000/A000.
    mapper.write(0x5102, 2);
    mapper.write(0x5103, 1);
    mapper.write(0x8000, 0x77);
    mapper.write(0xa000, 0x88);
    assert_eq!(mapper.read_page(0x80).unwrap()[0], 0x77);
    assert_eq!(mapper.read_page(0xa0).unwrap()[0], 0x88);
    mapper.write(0x5102, 0);
    mapper.write(0x8000, 0x99);
    assert_eq!(mapper.read(0x8000), 0x77);
    mapper.write(0x5104, 2);
    mapper.write(0x5c00, 0x33);
    mapper.write(0x5104, 0);
    mapper.write(0x5105, 0xe4); // CIRAM0, CIRAM1, ExRAM, fill.
    mapper.write(0x5106, 0x44);
    mapper.write(0x5107, 2);
    ppu.write_byte(mapper.as_mut(), 0x2000, 0x11);
    ppu.write_byte(mapper.as_mut(), 0x2400, 0x22);
    for (address, value) in [
        (0x2000, 0x11),
        (0x2400, 0x22),
        (0x2800, 0x33),
        (0x2c00, 0x44),
        (0x2fc0, 0xaa),
    ] {
        assert_eq!(ppu.read_byte(mapper.as_ref(), address), value);
    }
    mapper.write(0x5104, 3);
    mapper.write(0x5c00, 0xff); // Read-only ExRAM.
    assert_eq!(mapper.read(0x5c00), 0x33);
    assert_eq!(ppu.read_byte(mapper.as_ref(), 0x2800), 0);
}

#[test]
fn nes2_submapper_and_unsupported_extended_ids_are_not_legacy_ram_counts() {
    let mut bytes = image_header(23, true);
    bytes[8] = 0x30;
    let (cart, id) = ines::load(&mut Cursor::new(bytes.clone())).unwrap();
    assert_eq!(cart.submapper, Some(3));
    let mut mapper = new(cart, id).unwrap();
    mapper.write(0xf000, 0xf);
    mapper.write(0xf001, 0xf);
    mapper.write(0xf002, 6);
    for _ in 0..256 {
        mapper.clock_cpu();
    }
    assert!(!mapper.irq_pending()); // VRC2b, not VRC4.
    bytes[8] |= 1; // Mapper 279 must not become mapper 23.
    assert!(ines::load(&mut Cursor::new(bytes)).is_none());
}

#[test]
fn vrc6_audio_register_writes_do_not_modify_chr_banks() {
    for id in [24, 26] {
        let mut mapper = mapper(id);
        for address in 0x9000..=0xb002 {
            if address & 0xfff <= 2 {
                mapper.write(address, 0xff);
            }
        }
        for slot in 0..8 {
            assert_eq!(mapper.read(slot * 0x400), 0, "mapper {id}, slot {slot}");
        }
    }
}

#[test]
fn oeka_bus_conflicts_use_the_current_prg_rom_byte() {
    let mut cart = tagged_cart(8, 1);
    cart.chr = CHR::RAM(vec![[0; 0x2000]; 4]);
    let mut mapper = new(cart, 96).unwrap();
    mapper.write(0x8000, 2); // ROM byte 0 suppresses the write's PRG bits.
    assert_eq!(mapper.read(0x8000), 0);
    mapper.write(0xffff, 2); // ROM byte 3 allows bank 2.
    assert_eq!(mapper.read(0x8000), 8);
    mapper.write(0x8000, 1); // ROM byte 8 in the current bank suppresses bit 0.
    assert_eq!(mapper.read(0x8000), 0);
}

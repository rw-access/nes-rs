use nes_disasm::chr::{decode_tile, encode_tile, inspect_chr};
use nes_disasm::decode::metadata;
use nes_disasm::{
    decode_at, recursive_traversal, Cpu, DecodeOptions, Dialect, DisasmError, InesFormat,
    InesImage, LinearSweepOptions, OpcodeClass, TraversalOptions,
};

fn official_decode_options() -> DecodeOptions {
    DecodeOptions {
        cpu: Cpu::Ricoh2A03,
        dialect: Dialect::Official6502,
    }
}

fn nrom_image(prg_size: usize) -> Vec<u8> {
    assert!(matches!(prg_size, 0x4000 | 0x8000));
    let mut rom = vec![0; 16 + prg_size];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = (prg_size / 0x4000) as u8;
    rom
}

#[test]
fn every_official_opcode_decodes_and_other_policies_are_explicit() {
    for opcode in 0..=u8::MAX {
        let bytes = [opcode, 0, 0];
        let result = decode_at(&bytes, 0x8000, None, official_decode_options());
        match metadata(opcode).class {
            OpcodeClass::Official => {
                let instruction =
                    result.unwrap_or_else(|error| panic!("official opcode ${opcode:02X}: {error}"));
                assert_eq!(instruction.opcode, opcode);
            }
            OpcodeClass::UnofficialStable | OpcodeClass::UnofficialUnstable | OpcodeClass::Halt => {
                assert_eq!(
                    result,
                    Err(DisasmError::UnsupportedOpcode {
                        address: 0x8000,
                        opcode,
                    }),
                    "non-official opcode ${opcode:02X} must be disabled by the official policy"
                );
            }
        }
    }
}

#[test]
fn brk_consumes_and_preserves_its_signature_byte() {
    let bytes = [0x00, 0x42, 0xea, 0x60];
    let brk = decode_at(&bytes, 0x8000, None, official_decode_options()).unwrap();
    assert_eq!(brk.bytes, [0x00, 0x42]);
    assert_eq!(brk.length(), 2);

    let following = decode_at(&bytes[2..], 0x8002, None, official_decode_options()).unwrap();
    assert_eq!(following.mnemonic(), "NOP");

    let analysis = nes_disasm::linear_sweep(
        &bytes,
        0x8000,
        LinearSweepOptions {
            decode: official_decode_options(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(analysis.reassemble().unwrap(), bytes);
    assert_eq!(analysis.item_at(0x8002).unwrap().bytes(), &[0xea]);
}

#[test]
fn recursive_traversal_follows_branches_calls_and_fallthrough_but_not_indirect_jumps() {
    let bytes = [
        0xd0, 0x03, // $8000: BNE $8005 (target)
        0x20, 0x0b, 0x80, // $8002: JSR $800B (fall-through and call target)
        0x6c, 0x00, 0x90, // $8005: JMP ($9000), no statically known target
        0xa9, 0x01, 0x60, // $8008: unreachable after the indirect JMP
        0xa9, 0x42, 0x60, // $800B: JSR target
    ];
    let analysis =
        recursive_traversal(&bytes, 0x8000, &[0x8000], TraversalOptions::default()).unwrap();
    let addresses: Vec<_> = analysis
        .instructions()
        .map(|instruction| instruction.address)
        .collect();

    for address in [0x8000, 0x8002, 0x8005, 0x800b, 0x800d] {
        assert!(
            addresses.contains(&address),
            "missing instruction at ${address:04X}"
        );
    }
    assert!(!addresses.contains(&0x9000), "indirect JMP was speculated");
    assert_eq!(
        analysis.item_at(0x8005).unwrap().bytes(),
        &[0x6c, 0x00, 0x90]
    );
}

#[test]
fn nrom_16k_mirroring_and_vectors_are_discovered() {
    let mut rom = nrom_image(0x4000);
    let prg = 16;
    rom[prg + 0x0000] = 0xa9;
    rom[prg + 0x3ffa..prg + 0x4000].copy_from_slice(&[
        0x34, 0x12, // NMI
        0x78, 0x56, // RESET
        0xbc, 0x9a, // IRQ/BRK
    ]);

    let image = InesImage::parse(&rom).unwrap();
    assert!(image.is_nrom());
    let mapping = image.nrom().unwrap();
    assert_eq!(mapping.byte(0x8000).unwrap(), 0xa9);
    assert_eq!(mapping.byte(0xc000).unwrap(), 0xa9);
    assert_eq!(mapping.cpu_to_file_offset(0xc000).unwrap(), prg);
    assert_eq!(mapping.vectors().unwrap(), [0x1234, 0x5678, 0x9abc]);
}

#[test]
fn nrom_32k_mapping_and_vectors_are_discovered() {
    let mut rom = nrom_image(0x8000);
    let prg = 16;
    rom[prg] = 0x11;
    rom[prg + 0x4000] = 0x22;
    rom[prg + 0x7ffa..prg + 0x8000].copy_from_slice(&[
        0x11, 0x11, // NMI
        0x22, 0x22, // RESET
        0x33, 0x33, // IRQ/BRK
    ]);

    let image = InesImage::parse(&rom).unwrap();
    let mapping = image.nrom().unwrap();
    assert_eq!(mapping.byte(0x8000).unwrap(), 0x11);
    assert_eq!(mapping.byte(0xc000).unwrap(), 0x22);
    assert_eq!(mapping.cpu_to_file_offset(0xc000).unwrap(), prg + 0x4000);
    assert_eq!(mapping.vectors().unwrap(), [0x1111, 0x2222, 0x3333]);
}

#[test]
fn ines_reassemble_preserves_trainer_chr_and_trailing_bytes() {
    let mut rom = vec![0; 16 + 512 + 0x4000 + 0x2000 + 7];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 1;
    rom[5] = 1;
    rom[6] = 0x04;
    for (index, byte) in rom[16..].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(37).wrapping_add(11);
    }

    let image = InesImage::parse(&rom).unwrap();
    assert_eq!(image.trainer, Some(16..528));
    assert_eq!(image.prg, 528..(528 + 0x4000));
    assert_eq!(image.chr, Some((528 + 0x4000)..(528 + 0x4000 + 0x2000)));
    assert_eq!(image.trailing, Some((528 + 0x4000 + 0x2000)..rom.len()));
    assert_eq!(image.reassemble(), rom);
}

#[test]
fn nes2_header_and_exponent_rom_size_are_detected() {
    let mut rom = vec![0; 16 + 0x4000];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 14 << 2; // 2^14 bytes in NES 2.0 exponent/multiplier form.
    rom[7] = 0x08; // NES 2.0 marker.
    rom[9] = 0x0f; // Exponent/multiplier form for PRG ROM size.

    let image = InesImage::parse(&rom).unwrap();
    assert_eq!(image.header.format, InesFormat::Nes2);
    assert_eq!(image.header.prg_rom_size, 0x4000);
    assert_eq!(image.reassemble(), rom);
}

#[test]
fn chr_inspection_and_tile_codec_round_trip_without_losing_trailing_bytes() {
    let mut source = vec![0; 16 * 2 + 3];
    for (index, byte) in source[..32].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(19).wrapping_add(5);
    }
    source[32..].copy_from_slice(&[0xde, 0xad, 0xbe]);

    let inspection = inspect_chr(&source);
    assert_eq!(inspection.tiles().len(), 2);
    assert_eq!(inspection.trailing(), &[0xde, 0xad, 0xbe]);
    assert_eq!(inspection.reassemble(), source);
    assert_eq!(inspection.pattern_tables().len(), 1);
    assert_eq!(inspection.pattern_tables()[0].trailing, source[32..]);

    let tile = decode_tile(&source[..16]).unwrap();
    assert_eq!(encode_tile(&tile).unwrap(), source[..16]);
    assert_eq!(inspection.tile(1).unwrap().raw, source[16..32]);
}

use nes_asm::{write_ines, Assembler, InesOptions};
use nes_render::headless::{run_rom, Status};
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn boot_source(origin: &str) -> String {
    format!(
        r#".org {origin}
reset:
    SEI
    LDA #$DE
    STA $6001
    LDA #$B0
    STA $6002
    LDA #$61
    STA $6003
    LDA #$00
    STA $6000
forever:
    JMP forever
.org $FFFA
    .word reset
    .word reset
    .word reset
"#
    )
}

fn temp_rom(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("nes-asm-{name}-{}-{nonce}.nes", std::process::id()))
}

#[test]
fn assembles_and_boots_32k_nrom_with_chr() {
    let object = Assembler::default()
        .assemble_str("boot.asm", &boot_source("$8000"))
        .unwrap();
    let rom = write_ines(
        &object,
        &InesOptions {
            prg_banks: 2,
            chr_banks: 1,
            ..InesOptions::default()
        },
    )
    .unwrap();
    assert_eq!(&rom[..4], b"NES\x1a");
    assert_eq!(rom[4], 2);
    assert_eq!(rom[5], 1);
    assert_eq!(rom.len(), 16 + 0x8000 + 0x2000);
    assert_eq!(nes_core::ines::load(&mut Cursor::new(&rom)).unwrap().1, 0);

    let path = temp_rom("32k");
    fs::write(&path, &rom).unwrap();
    let result = run_rom(&path, 10).unwrap();
    assert_eq!(result.status, Some(Status::Complete(0)));
    assert!(result.signature_valid);
    fs::remove_file(path).unwrap();
}

#[test]
fn assembles_and_boots_16k_nrom_with_chr_ram() {
    let object = Assembler::default()
        .assemble_str("boot.asm", &boot_source("$C000"))
        .unwrap();
    let rom = write_ines(
        &object,
        &InesOptions {
            prg_banks: 1,
            chr_banks: 0,
            ..InesOptions::default()
        },
    )
    .unwrap();
    assert_eq!(rom.len(), 16 + 0x4000);
    assert_eq!(rom[4], 1);
    assert_eq!(rom[5], 0);
    assert_eq!(
        &rom[16 + 0x3ffa..16 + 0x4000],
        &[0x00, 0xc0, 0x00, 0xc0, 0x00, 0xc0]
    );
    assert_eq!(nes_core::ines::load(&mut Cursor::new(&rom)).unwrap().1, 0);

    let path = temp_rom("16k");
    fs::write(&path, &rom).unwrap();
    let result = run_rom(&path, 10).unwrap();
    assert_eq!(result.status, Some(Status::Complete(0)));
    assert!(result.signature_valid);
    fs::remove_file(path).unwrap();
}

#[test]
fn cli_writes_requested_ines_shape() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nes-asm-cli-{nonce}"));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.asm");
    let output = dir.join("main.nes");
    fs::write(&source, ".org $8000\nNOP\n").unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_nes-asm"))
        .args([
            source.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--format",
            "ines",
            "--prg-banks",
            "1",
            "--chr-banks",
            "0",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let rom = fs::read(&output).unwrap();
    assert_eq!(&rom[..4], b"NES\x1a");
    assert_eq!(rom.len(), 16 + 0x4000);
    fs::remove_dir_all(dir).unwrap();
}

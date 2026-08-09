use nes_render::headless::{run_rom, Status};
use std::{env, path::PathBuf};

const ROMS: &[&str] = &[
    "apu_test/rom_singles/2-len_table.nes",
    "apu_test/rom_singles/3-irq_flag.nes",
    "apu_test/rom_singles/7-dmc_basics.nes",
    "apu_test/rom_singles/8-dmc_rates.nes",
];

#[test]
fn compatible_apu_roms() {
    if env::var("NES_RUN_APU_ROM_TESTS").ok().as_deref() != Some("1") {
        eprintln!("skipping APU ROM tests; set NES_RUN_APU_ROM_TESTS=1 to enable");
        return;
    }

    let root = env::var_os("NES_TEST_ROM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/nes-test-roms")
        });

    for relative_path in ROMS {
        let path = root.join(relative_path);
        let result =
            run_rom(&path, 300).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(
            result.signature_valid,
            "{} did not report a test signature",
            path.display()
        );
        assert_eq!(
            result.status,
            Some(Status::Complete(0)),
            "{} reported ${:02X}: {}",
            path.display(),
            result.status.map(Status::byte).unwrap_or(0),
            result.text
        );
    }
}

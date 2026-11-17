# Mapper repair review — 2026-11-17

The seven cases in [the initial review](initial-review.md) now produce
recognizable, progressing scenes. The final full run captured all fourteen
available representatives without a crash, timeout, uniform-frame warning, or
inactive-output warning. Thirteen mapper IDs still have no matching ROM.

## Repairs and evidence

| Failure | Underlying issue | Verified result |
| --- | --- | --- |
| Castlevania, mapper 2 | PPUCTRL NMI enable used the vblank *period* instead of the PPUSTATUS vblank *flag*, retriggering NMI after the game had cleared the flag. | Clean title and opening gameplay. |
| Adventure Island II, mapper 4 | Sprite evaluation cleared sprite-zero-hit every scanline. Collision also incorrectly depended on sprite/background priority instead of opaque overlap. | Full title and progression to the world map after Start. |
| Castlevania III, mapper 5 | MMC5 lacked separate sprite/background CHR fetches, used the wrong 2 KiB CHR registers, and failed to acknowledge IRQ status reads or restart its scanline counter each frame. | Castle intro and scrolling story text. |
| Klax, mapper 64 | Mapper 64 was an MMC3 alias, losing RAMBO-1 register 15, registers 8/9, and its bank layouts. | Clean title and animated credits. |
| Dragon Power, mapper 66 | GxROM PRG and CHR bank fields were reversed. | Clean title and stage introduction. |
| Batman, mapper 69 | The FME-7 `$6000` window always returned ROM; enabled RAM writes were discarded. Generic PPU fixes also apply. | Title and opening gameplay after allowing startup time. |
| VS Super Mario Bros., mapper 99 | CHR bank selection incorrectly selected PRG bank 1 instead of bank 4. Its legacy header also omitted the RGB PPU palette subtype. | Clean title and attract-mode gameplay with corrected color permutation. |

Sprite-zero detection now requires both opaque pixels, both rendering enables,
left-edge visibility, and x < 255, regardless of priority. Sprite status flags
remain latched until pre-render. The same collision rule is used in normal,
timestamped, and video-disabled rendering paths.

RAMBO-1 now has an independent mapper implementation with three switchable
8 KiB PRG windows, 4-bit register selection, 1 KiB CHR mode, CHR inversion,
and CPU/scanline IRQ clock modes. MMC3 remains separate.

## PPU/mapper interface contract

MMC5 requires distinguishing sprite pattern fetches, background pattern fetches,
and CPU PPUDATA reads. Separate default trait callbacks preserve the existing
read behavior for other mappers. MMC5 monitors fully decoded PPUCTRL writes:
8x8 rendering uses bank set A; 8x16 sprites use A while backgrounds use B;
CPU PPUDATA uses the last written set in 8x16 mode. Context-independent direct
CHR pages remain the NROM fast path. The dynamic mapper wrapper forwards every
new callback. The end-of-frame callback resets MMC5's scanline tracking.

The optional VS color permutation is applied only to emitted pixels, including
layered output, so CPU palette-RAM reads retain the original values. Legacy VS
SMB identification uses its PRG CRC32 (`8192c804`), not its filename. Unknown VS
ROMs do not inherit that game's palette. The indexed renderer maps RP2C04-0004
colors to the closest RP2C03 color indices; it does not reproduce the RGB PPU's
additional DAC colors exactly.

## Reference and replay basis

The exact same seven ROM files booted in independent FCEUmm, built from
[libretro-fceumm](https://github.com/libretro/libretro-fceumm/tree/77468e568d9fe9c48d684ca911f1a3fe7259fdf7).
Reference captures used the original boot/Start schedules, plus a longer
Castlevania III capture. Comparison was visual; palette models, startup state,
and frame alignment prevent claiming pixel-exact agreement.

Original evidence remains in `artifacts/mapper-smoke/reviewed-timing`.
Final evidence is in `artifacts/mapper-fixes/full-run`, with the full ROM hashes,
input schedules, captures, and metrics. Independent captures are in
`artifacts/mapper-fixes/reference`. These artifacts and all commercial ROMs
remain outside Git. No golden baseline was automatically accepted.

The smoke schedule now allows Batman to reach its title at frame 1800 and
captures gameplay at frame 3000. Castlevania III's final capture moves from
frame 900's mostly empty curtain transition to frame 3000's story text. These
changes expose meaningful content rather than suppressing diagnostics.

## Validation and remaining limits

Validation passed: `cargo test --locked --offline --release --workspace`,
`cargo test --locked --offline --release -p nes-core --features
"timestamped-scheduler,layered-render,audio-stems"` (114 tests), and
`python3 -m unittest discover -s tests/mapper-smoke` (3 tests). The default core
suite passes 69 tests; workspace feature unification runs 82 core tests. The
smoke manifest coverage check also passes in the workspace suite.

Regression tests cover NMI gating, latched sprite flags, collision visibility and
priority, GxROM fields, FME-7 ROM/RAM selection, RAMBO-1 PRG/CHR/IRQ modes,
MMC5 A/B banks and IRQ acknowledgement, VS bank selection, checksum-scoped
palette detection, and unchanged CPU palette RAM. The existing timestamped,
rewind, audio, and layered-output tests remain part of verification.

This repairs the observed cases; it is not complete mapper hardware validation.
MMC3/RAMBO-1 still use scanline clocks rather than filtered PPU A12 edges.
MMC5 ExRAM, extended attributes, split rendering, full PRG-RAM behavior, and
expansion audio remain incomplete. VS coin/DIP input and arbitrary RGB PPU
subtype metadata are not implemented. Its post-Start capture can be attract
mode rather than a credited game. Missing mapper representatives remain gaps.

Hardware references:
[NMI](https://www.nesdev.org/wiki/NMI),
[PPU status](https://www.nesdev.org/wiki/PPU_registers),
[MMC5](https://www.nesdev.org/wiki/MMC5),
[RAMBO-1](https://www.nesdev.org/wiki/RAMBO-1),
[GxROM](https://www.nesdev.org/wiki/GxROM),
[FME-7](https://www.nesdev.org/wiki/Sunsoft_FME-7), and
[RGB PPU palettes](https://www.nesdev.org/wiki/PPU_palettes).

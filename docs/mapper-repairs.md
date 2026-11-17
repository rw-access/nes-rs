# Repairs following the NESdev mapper audit

The [baseline audit](mapper-audit.md) identified 36 failing hardware contracts.
This repair pass promotes **33** into normal regression coverage. Three remain
explicitly ignored: real-board mapper-15 CHR write protection (G28), KT-008
multicart outer banking (G32), and the FDS placeholder (G33). These are not
counted as repaired. The original 27-ID matrix describes the pre-repair code.

## Repaired behavior

| Contracts | Changes |
| --- | --- |
| G01–04 | MMC4 selects the entire 16 KiB PRG window; MMC2 has its own lower-latch trigger range; both chips change CHR banks after returning the triggering byte. Register widths and mirroring bit decoding are corrected too. |
| G05–13 | VRC2/4 compose CHR nibbles in the correct register, keep raw register values before VRC2a's shift, and decode both legacy address-pin layouts. VRC IRQs count upward with a CPU-driven 341/3 prescaler, retain enable-after-acknowledge, and use the separate VRC6 register layout. Explicit NES 2.0 VRC pin/chip variants are selected. |
| G14–17 | Namco internal RAM increments on reads and writes, saturating at `$7F`. Pattern and nametable banks can select CIRAM, and nametable banks can select CHR memory. |
| G18–19 | Both CPU execution paths emit the original and modified RMW writes. MMC1 drops the adjacent second serial bit while retaining reset writes. SUROM selects the outer 256 KiB PRG region, and PRG-RAM disable no longer contaminates the ROM bank number. |
| G20 | MMC3 honors RAM enable and write protection after `$A001` is written. Legacy writable power-on behavior remains for compatibility. |
| G21–25 | MMC5 routes all four nametable selectors independently, implements ExRAM and fill nametables, banks low PRG RAM, maps writable RAM in the upper windows and returns matching fast-read pages. ROM selection uses bit 7. |
| G26–27 | Mapper 15 updates all PRG windows in each mode, aligns NROM-256, fixes the upper bank within the selected 128 KiB region in UNROM mode, and ignores data bit 7 outside 8 KiB mode. Mirroring follows the latch in every mode. |
| G29 | Mapper 34 selects NINA-001 via legacy CHR-ROM size or explicit submapper, retaining BNROM for the other variant. NINA's registers overlap SRAM writes correctly. |
| G30–31 | Oeka Kids has its own PRG/CHR implementation and 32 KiB CHR RAM. Writes account for PRG-ROM bus conflicts. The inner CHR bank follows a rising edge into PPU `$2xxx`; PPUADDR changes and PPUDATA increments notify the board even without another memory access. |
| G34–36 | NES 2.0 submapper no longer becomes a RAM count; aligned RAM sizes are allocated from the correct fields, and trainers populate `$7000–71FF`. Unsupported extended mapper/ROM-size encodings are rejected instead of silently decoded as legacy headers. |

Source links for each contract are in the baseline matrix. In particular,
[VRC IRQs](https://www.nesdev.org/wiki/VRC_IRQ) specify CPU-clocked operation
independent of rendering, and [Oeka Kids](https://www.nesdev.org/wiki/INES_Mapper_096)
requires address transitions rather than just nametable reads.

VRC6 audio writes also no longer fall through into the VRC4 CHR register path.
Expansion audio itself remains unsupported. Namco and MMC5 contract setups
explicitly program their initial nametable sources rather than treating
undefined hardware power-on registers as guarantees.

## PPU bug found by the new visual fixtures

The generated ROMs exposed eight incorrect pixels at the top-left of MMC4 and
VRC6 output. `can_batch_three_idle_ticks` classified pre-render dots through
325 as idle, skipping the first nametable, attribute and pattern fetches at
321/323/325. The interval now ends at 317.

A new test compares dot-by-dot execution against CPU-cycle batching and checks
that both prefetched tiles match. It failed before the change and passes
afterward. No screenshot discrepancy is cropped or tolerated to hide it.

## Validation and reproduction

The default core suite passes **115 tests**; the full-feature core suite passes
**160 tests**, with three known-gap contracts ignored in each. Workspace tests
and the three Python smoke-runner tests also pass.

The core suite includes 33 promoted contracts plus additional tests for all
VRC CHR windows/pin layouts, IRQ cadence, all six CPU memory RMW operations,
MMC1 reset exceptions, MMC5 memory sources/protection, NES 2.0 VRC2 selection,
Oeka address-only transitions, VRC6 audio-register isolation and pre-render
fetch equivalence.

```sh
cargo test --locked --offline --release -p nes-core
cargo test --locked --offline --release --workspace \
  --features nes-core/timestamped-scheduler,nes-core/layered-render,nes-core/audio-stems
python3 -m unittest discover -s tests/mapper-smoke
```

Generate seven original test ROMs (IDs 10, 21–26), without commercial ROMs:

```sh
python3 tests/mapper-smoke/generate_audit_fixtures.py artifacts/mapper-audit/fixtures
cargo build --locked --offline --release -p nes-render --bin mapper_smoke_capture
python3 scripts/mapper_smoke.py \
  --rom-root artifacts/mapper-audit/fixtures \
  --manifest artifacts/mapper-audit/fixtures/cases.tsv \
  --output artifacts/mapper-audit/fixture-final-games
```

The mapper-10 probe renders a tile selected by reading `$A000` in the switched
PRG bank. VRC2/4 probes select CHR using both register nibbles. VRC6 probes
change the visible palette from a CPU-mode IRQ handler. They are intentionally
static/mostly uniform, so the generic game-smoke heuristic reports suspicious
output; that warning is expected for these fixtures and is not their oracle.

The same ROMs were booted in **FCEUmm
`77468e568d9fe9c48d684ca911f1a3fe7259fdf7`**, at frames 60 and 120 with matching
inputs and full overscan. All **14 capture comparisons** match pixel-for-pixel
by red/green/black color class, without cropping. RGB palettes differ between
the emulators; the comparison deliberately checks these simple probes' spatial
patterns and expected color classes, not exact RGB hues. The comparison script
is limited to these fixtures and is not a general screenshot oracle.

```sh
python3 tests/mapper-smoke/compare_audit_fixtures.py \
  artifacts/mapper-audit/fixture-final-games \
  artifacts/mapper-audit/fixture-reference
```

All **14 available commercial games** were also booted and visually inspected
at the original manifest's boot/post-Start points. Thirteen IDs still have no
representative in the owner's collection. Compare `final-games/report.json`
with the earlier `artifacts/mapper-fixes/full-run/report.json`; changed pixels
are recorded in `game-comparison.json`. The only differences are the repaired
top-left prefetch pixels, not changed game progression. Start capture still
does not establish gameplay, particularly for arcade coin-input games.

Generated evidence lives under the gitignored `artifacts/mapper-audit/`:
`fixture-comparison.json`, `game-comparison.json`, `final-games/`,
`fixture-final-games/`, `fixture-reference/` and saved test logs. ROMs and
screenshots are not committed.

## Remaining limits

- G28 stays relaxed for mapper-15 hacks, as documented by
  [NESdev](https://www.nesdev.org/wiki/INES_Mapper_015). Accurate real-board
  protection needs an explicit compatibility policy rather than silently
  breaking those images.
- G32 needs the complete KT-008/MINDKIDS mode/mask/lock implementation;
  recognizing a single outer register would not establish board support.
- G33 needs an FDS RAM/BIOS/disk/timer subsystem, not a writable NROM alias.
- MMC3/RAMBO filtered A12 timing, disputed RAMBO banking behavior, MMC6,
  expansion audio, MMC5 extended attributes/split screen and general Vs.
  System variants remain outside this repair pass.
- NES 2.0 support remains partial: the mapper API is still 8-bit, extended
  ROM encodings are rejected, RAM storage is in 8 KiB banks (small RAM
  mirroring and RAM/NVRAM separation are not modeled), and bus-conflict
  profiles and console timing are not fully implemented. `Cartridge` now
  carries `submapper: Option<u8>`; manual legacy constructors use `None`.
- The instruction-atomic CPU uses an explicit adjacent-write pair for RMW;
  this repairs mapper-visible values/filtering without claiming cycle-level
  bus interleaving. SUROM's ordinary outer-bank case is covered; all MMC1
  board wiring combinations are not certified.
- Oeka's address latch is exercised, including synthetic sprite dummy
  addresses, but a real tablet game's full behavior and bus conflicts have
  not been validated. No tablet-input support is implied.

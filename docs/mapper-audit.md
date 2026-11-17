# Mapper hardware audit — 2026-11-17

Audited production baseline: `fa7e25fb40402979c4fc88ad5b966bc9d05aaea4`.
All **27 IDs accepted by `cartridge::new`** were inspected against NESdev's
board/register descriptions, together with the loader, CPU writes and PPU
address path. This is a source audit plus executable contract probes, not a
claim that every game, board revision or electrical timing case was tested.
This matrix records the original baseline. The subsequent
[repair pass](mapper-repairs.md) fixes 33 of these 36 contracts and a newly
reproduced PPU prefetch defect; consult that report for current status.

## Findings and evidence levels

There are **36 reproducible failing contracts** in
[`cartridge_audit_tests.rs`](../crates/nes-core/src/cartridge_audit_tests.rs).
These are distinct assertions, not 36 independent root causes or 36 newly
broken games. Some describe already unsupported features and intentional
compatibility choices. The table below distinguishes those from defects.

- **Defect:** implemented behavior contradicts the cited register contract.
- **Partial:** a board feature or variant has no implementation; accepting its
  mapper number does not establish support.
- **Unresolved:** sources or hardware variants disagree; no failing contract
  was added merely to enforce one emulator's interpretation.
- **Basic match:** no additional mismatch found in the ordinary bank layout;
  this is not hardware certification.

The strongest immediate fixes are MMC4's selected PRG window, shared VRC
CHR/IRQ logic, and the CPU/MMC1 consecutive-write interaction. Mapper 96 and
224 need actual board implementations. NES 2.0 metadata is foundational for
selecting variants and allocating the correct RAM; it should precede adding
variant-specific behavior heuristically.

## Complete factory matrix

| ID | Board / owning implementation | Result against NESdev | Executable gaps |
| --- | --- | --- | --- |
| 0 | NROM | **Basic match:** ordinary 16/32 KiB PRG and 8 KiB CHR. Family BASIC 2/4 KiB RAM mirroring and unusual NES 2.0 ROM sizes are not represented accurately. [NROM](https://www.nesdev.org/wiki/NROM) | Shared loader gaps G34–36 |
| 1 | MMC1 | **Defect:** CPU RMW does not emit the original-value write; mapper has no consecutive-cycle filter. **Partial:** SUROM outer PRG selection, SOROM/SXROM RAM banks and revision wiring. [MMC1](https://www.nesdev.org/wiki/INES_Mapper_001) | G18–19 |
| 2 | UxROM | **Basic match:** selected lower 16 KiB, fixed last bank. NES 2.0 bus-conflict profiles cannot be selected. Legacy unknown-conflict behavior alone is not counted as a defect. [UxROM](https://www.nesdev.org/wiki/UxROM) | — |
| 3 | CNROM | **Basic match:** 8 KiB CHR selection. Explicit AND-bus-conflict submapper and optional small RAM mirroring remain incomplete. [CNROM](https://www.nesdev.org/wiki/CNROM) | — |
| 4 | MMC3 | **Defect:** `$A001` PRG-RAM enable/protect ignored. **Partial:** IRQ uses a visible-scanline callback rather than filtered PPU A12 transitions; MMC6 and IRQ revisions are not distinguished. [MMC3](https://www.nesdev.org/wiki/MMC3) | G20 |
| 5 | MMC5 | **Partial:** independent four-nametable selectors reduced to a mirroring enum; no ExRAM/fill nametables, upper RAM windows or low RAM-bank selection. Extended attributes, split screen and expansion audio also absent. Recent ordinary CHR/IRQ repairs do not establish full support. [MMC5](https://www.nesdev.org/wiki/MMC5) | G21–25 |
| 7 | AxROM | **Basic match:** ordinary 32 KiB PRG and one-screen selection. Explicit bus-conflict profiles and larger board variants need metadata/wiring support. [AxROM](https://www.nesdev.org/wiki/AxROM) | — |
| 9 | MMC2 | **Defect:** lower latch range incorrectly includes `$0FD9–0FDF`/`$0FE9–0FEF`; triggering fetch returns the newly selected bank. Check rendered fetch ordering after repair. [MMC2](https://www.nesdev.org/wiki/MMC2), [hardware discussion](https://forums.nesdev.org/viewtopic.php?t=9325) | G02–03 |
| 10 | MMC4 | **Defect:** `$A000–BFFF` comes from the fixed last bank instead of the selected 16 KiB bank. Triggering CHR fetch switches too early, as for MMC2. [MMC4](https://www.nesdev.org/wiki/MMC4), [latch discussion](https://forums.nesdev.org/viewtopic.php?t=9325) | G01, G04 |
| 11 | Color Dreams | **Basic match:** low PRG/high CHR register fields. Bus conflicts are not modeled. [Color Dreams](https://www.nesdev.org/wiki/Color_Dreams) | — |
| 15 | K-1029/K-1030 | **Defect relative to real board:** modes 1/3 fail to update both PRG halves correctly. **Policy gap:** real-board CHR write protection conflicts with hacks that intentionally use relaxed emulation; preserve a separate compatibility decision. [Mapper 15](https://www.nesdev.org/wiki/INES_Mapper_015) | G26–28 |
| 19 | Namco 163 | **Defect:** internal data reads do not auto-increment; writes wrap at `$7F` rather than saturating. **Partial:** nametable bank registers and CHR-to-CIRAM mapping unused; expansion audio absent. Saturating IRQ and read-without-acknowledge are correct. [Mapper 19](https://www.nesdev.org/wiki/INES_Mapper_019), [data port](https://www.nesdev.org/wiki/Namco_163_audio), [hardware tests](https://forums.nesdev.org/viewtopic.php?t=25381) | G14–17 |
| 20 | FDS placeholder → NROM | **Partial, explicitly a placeholder:** no FDS RAM/BIOS/disk controller/timer/audio subsystem. Loading a wrapped file is not FDS support. [FDS programming](https://www.nesdev.org/wiki/Programming_the_FDS) | G33 |
| 21 | VRC4a/c → shared VRC | **Defect:** CHR nibble/register selection, VRC4c A6/A7 decode and IRQ logic. IRQ counts down instead of up; scanline mode relies on PPU callbacks; acknowledge uses write data instead of saved control. [VRC wiring](https://www.nesdev.org/wiki/VRC4), [IRQ](https://www.nesdev.org/wiki/VRC_IRQ) | G08–11; shared G05 |
| 22 | VRC2a → shared VRC | **Defect:** CHR nibbles do not compose correctly and CHR right shift is applied repeatedly during writes. **Partial:** exact VRC2 serial latch/RAM decode. VRC2 has no IRQ. [VRC2/4](https://www.nesdev.org/wiki/INES_Mapper_023) | G06 |
| 23 | VRC2b / VRC4e/f → shared VRC | **Defect:** CHR and VRC4 IRQ logic. **Partial:** submapper is ignored, so VRC2 versus VRC4 is not selected explicitly. [VRC2/4](https://www.nesdev.org/wiki/INES_Mapper_023), [IRQ](https://www.nesdev.org/wiki/VRC_IRQ) | G05, G09–11 |
| 24 | VRC6a → shared VRC | **Defect:** IRQ registers incorrectly use VRC4's split-nibble layout. **Partial:** `$B003` banking modes, ROM nametables and expansion audio. [VRC6](https://www.nesdev.org/wiki/VRC6) | G12 |
| 25 | VRC2c / VRC4b/d → shared VRC | **Defect:** low-address pin order and CHR nibble selection; shared IRQ defects. **Partial:** variants collapsed. [VRC2/4](https://www.nesdev.org/wiki/INES_Mapper_023) | G07; shared G09–11 |
| 26 | VRC6b → shared VRC | **Defect:** swapped-address VRC6 IRQ layout still decoded as VRC4. Same missing features as ID 24. [VRC6](https://www.nesdev.org/wiki/VRC6) | G13 |
| 34 | BNROM only | **Partial:** NINA-001 `$7FFD–7FFF` PRG/4 KiB CHR registers absent. Legacy CHR-ROM size and NES 2.0 submapper should distinguish boards. [Mapper 34](https://www.nesdev.org/wiki/INES_Mapper_034) | G29 |
| 50 | SMB2j conversion | **Basic match:** fixed/switchable 8 KiB slots and scrambled PRG bits agree. **Unresolved:** IRQ counter width/retrigger behavior has historically uncertain hardware documentation; one-shot interpretation is not newly declared defective. [Mapper 50](https://www.nesdev.org/wiki/INES_Mapper_050) | — |
| 64 | RAMBO-1 | **Partial:** A12 filtering/IRQ event timing approximated by scanline callbacks. **Unresolved:** old wiki versus newer hardware/reference behavior on PRG swap, odd CHR banks and IRQ details; see below. [RAMBO-1](https://www.nesdev.org/wiki/RAMBO-1) | No disputed contracts added |
| 66 | GxROM | **Basic match:** corrected PRG-high/CHR-low fields. AND bus conflicts and oversized board wiring remain incomplete. [GxROM](https://www.nesdev.org/wiki/GxROM) | — |
| 69 | FME-7 / 5A / 5B | **Basic match:** ordinary PRG/CHR, repaired RAM selection, down-counter and acknowledge. **Partial:** larger PRG-RAM banks, disabled-window open bus, chip variants and 5B expansion audio. [FME-7](https://www.nesdev.org/wiki/Sunsoft_FME-7) | Positive IRQ contract |
| 96 | Oeka Kids → CNROM | **Partial / wrong alias:** lacks PRG switch, 32 KiB CHR RAM and PPU-nametable-address-driven CHR latch. A 32 KiB fallback can also select NROM. [Mapper 96](https://www.nesdev.org/wiki/INES_Mapper_096) | G30–31 |
| 99 | Vs. System | **Partial:** recent bank and known-ROM palette repair covers one configuration. Missing general PPU variant metadata, 2C05 behavior, coin/DIP/controller wiring, shared small RAM mapping and RGB timing. [Vs. System](https://www.nesdev.org/wiki/Vs._System) | No additional executable contract |
| 224 | KT-008 → MMC3 | **Partial / wrong alias:** MMC3 core does not implement MINDKIDS outer bank registers, masks and GNROM modes. Small-image NROM fallback is not board support. [Mapper 224](https://www.nesdev.org/wiki/INES_Mapper_224), [COOLBOY/MINDKIDS](https://www.nesdev.org/wiki/COOLBOY) | G32 |

## Shared loader and bus boundaries

`ines.rs::INESHeader` detects NES 2.0 but continues interpreting byte 8 as an
iNES PRG-RAM count. A submapper-2 header consequently allocates 32 RAM banks.
The mapper stays 8-bit, and explicit RAM sizes, extended ROM sizes, submapper
and console/PPU details are lost. G34–35 cover the RAM mistakes. A real trainer
is consumed but discarded rather than made available at CPU `$7000` (G36).
The UNIF path ignores board identification and returns mapper 0; loading an
image should not imply that its board was recognized.
Sources: [NES 2.0](https://www.nesdev.org/wiki/NES_2.0),
[iNES](https://www.nesdev.org/wiki/INES),
[UNIF](https://www.nesdev.org/wiki/UNIF).

`Mapper` exposes neither timestamped CPU writes nor PPU address transitions.
`cpu.rs` emits only the final RMW write. `ppu.rs` caches nametable mirroring
using an enum, bypassing mapper nametable bank logic, and clocks scanline
IRQs at a fixed visible dot. Those boundaries explain several independent
symptoms: MMC1 write filtering, MMC3/RAMBO A12 timing, Oeka CHR latches and
Namco/MMC5 nametables cannot all be repaired by changing bank masks.

## Reproduction without a visual oracle

Fixtures fill each 8 KiB PRG / 1 KiB CHR page with a distinct byte. Register
writes therefore identify the selected physical page directly. PPU probes
exercise actual nametable access; the MMC1 case executes a CPU `INC $E100`
against a ROM byte `$FF`, followed by serial bank writes. These are local
synthetic cartridges, with no commercial ROM dependency.

Representative outcomes:

| Contract | Expected hardware result | Baseline result |
| --- | --- | --- |
| G01: MMC4 select 16 KiB bank 2, read `$A000` | PRG page tag 5 | Tag 15, from fixed final bank |
| G05: VRC4 `$B000=$A`, `$B001=3`, read CHR `$0000` | CHR tag `$3A` | `$0A`; high nibble applied to another register |
| G09: VRC4 latch `$FF`, CPU mode enabled, one CPU clock | IRQ pending | No IRQ |
| G30: Oeka PRG select 2, read `$8000` | PRG tag 8 | Tag 0 |
| G34: NES 2.0 submapper 2, zero declared PRG RAM | No RAM banks | 32 RAM banks |

Run normal tests (includes three new passing contracts):

```sh
cargo test --locked --offline --release -p nes-core
```

Run remaining known gaps explicitly (**expected nonzero exit**; originally
36 failed at the audited baseline, now three remain after repairs):

```sh
cargo test --locked --offline --release -p nes-core cartridge::audit -- --ignored --nocapture
```

Original audit validation before repairs:

- Default core suite: **72 passed**, 36 known-gap contracts ignored.
- `timestamped-scheduler,layered-render,audio-stems`: **117 passed**, 36 ignored.
- Explicit ignored-contract run: **36 failed**, as expected.
- Formatting, diff whitespace, 27-row coverage and 36-contract count checked.

Local generated logs and `summary.json` are in `artifacts/mapper-audit/`
(gitignored; contain no ROM images).

The `Gxx` ignore reasons are an executable backlog. Once a feature is repaired,
remove its ignore and keep the hardware assertion in the normal suite. Do not
replace expected hardware values with the current implementation's output.
These probes establish local invariants, not entire games' visible outcomes.
Game captures and reviewed reference comparisons remain necessary after fixes.

## Disputed behavior and next validation

The [RAMBO-1 wiki](https://www.nesdev.org/wiki/RAMBO-1) and the modern reference
implementation inspected during the earlier fixes disagree on some banking
and IRQ details. [Recent NESdev hardware discussion](https://forums.nesdev.org/viewtopic.php?p=311615)
also revisits them. Keep those separate from definite missing A12 timing;
resolve with a hardware test ROM/capture and documented chip revision before
changing previously repaired behavior. Mapper 50 counter retrigger semantics
need the same care. Namco's `$7F` saturation has newer
[direct hardware evidence](https://forums.nesdev.org/viewtopic.php?t=25381), so
an older emulator wrapping to zero is not a reliable contrary oracle.

Recommended sequence: repair MMC4 and VRC register contracts; add CPU bus-write
fidelity and MMC1 filtering; implement accurate NES 2.0/board selection; then
replace aliases and extend mapper-driven nametable/A12 interfaces. After each
bounded change, promote its contracts and rerun that board's game captures.
A single game per mapper remains useful as a smoke test, but cannot cover all
submappers, register modes, IRQ timing and expansion features.

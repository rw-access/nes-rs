# First mapper smoke review — 2026-11-17

Executed against the owner's NES media collection after the CHR address fix
`fcb2cf5`. Fourteen representatives were available for 27 supported mapper IDs;
the other thirteen remain explicit gaps in `cases.tsv`. No worker crashed or
exceeded its timeout. A completed run does not establish game correctness.

Visual inspection found these follow-up candidates:

| Mapper | Representative | Observation |
| --- | --- | --- |
| 2 | Castlevania | Corrupted, sparse graphics; identical sampled frames and no visible Start response. |
| 4 | Adventure Island II | Title text/graphics end abruptly; identical sampled frames and no visible Start response. |
| 5 | Castlevania III | Corrupted title graphics; boot and post-Start captures match. |
| 64 | Klax | Repeated glyphs across the display; boot and post-Start captures match. |
| 66 | Dragon Power | Uniform gray output throughout the sampled run. |
| 69 | Batman — Return of the Joker | Uniform gray output throughout the sampled run. |
| 99 | VS Super Mario Bros. | Uniform gray output; arcade coin/DIP input is also absent from this replay. |

The captures for mappers 0, 1, 3, 7, 9, 11, and 34 showed recognizable scenes
and visual changes. This is not independent hardware/reference validation.
Battletoads initially hit a nearly empty intro frame at frame 600; its boot
capture was moved to frame 900, with Start at 901 and final capture at 1200.
The original run was preserved instead of overwritten.

The initial automatic heuristics also missed the animated corruption in mapper
5 and the repeated glyphs in mapper 64. Reviewing the contact sheet remains
necessary. Static/blank-frame checks alone cannot establish correctness.

Reproduce with the commands in [README.md](README.md), using the filenames and
timelines in `cases.tsv`. Generated `report.json` records full ROM SHA-256 values,
frame hashes and diagnostics; `index.html` labels every capture. Local evidence
is under `artifacts/mapper-smoke/first-run` and
`artifacts/mapper-smoke/reviewed-timing` (both ignored by Git).

No golden baseline has been accepted. Before diagnosing the emulator or accepting
screenshots, compare these exact ROM hashes and replay inputs against an
independent emulator; malformed dumps, timing, peripherals and incomplete mapper
implementations have not yet been separated.

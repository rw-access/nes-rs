# Mapper game smoke tests

This opt-in suite captures one representative game per available mapper ID.
It uses the normal `Console` path, without a window or audio device. Commercial
ROMs stay outside Git and are never downloaded automatically. Ordinary workspace
tests do not require them.

```sh
cargo build --locked --release -p nes-render --bin mapper_smoke_capture
python3 scripts/mapper_smoke.py --rom-root /path/to/NES \
  --output artifacts/mapper-smoke/run-001 --allow-missing
```

`cases.tsv` names the files in the owner's media collection. Point `--rom-root`
at a mounted collection or an authorized local copy. Edit a copy of the manifest
and use `--manifest` for other filenames or additional representatives. Each row
contains mapper ID, relative ROM path, boot frame, Start frame, final frame, and
notes, separated by tabs. An empty ROM path explicitly records a coverage gap.
The shipped manifest retains one row for every supported mapper ID; the Rust
test checks this against the cartridge factory. Custom manifests may select a
subset for focused reruns. Different IDs sharing an implementation
remain separate coverage entries. An unavailable FDS wrapper does not imply disk
emulation support.

The worker checks the ROM's loaded mapper ID before execution and refuses small
mapper 64/96/224 headers that may route through the NROM compatibility fallback.
It does not rewrite ROM headers. Start is held for two frames beginning at the
configured frame; all other input is released. Captures are taken at the exact
listed frame numbers, without an extra frame advance. **Post-Start is not a claim
that gameplay started**: some games need more input, and VS games require coin
controls that this replay does not provide.

Each game runs in a separate process, with a default 30-second wall-clock timeout
(`--timeout` overrides it). Crashes and timeouts do not stop the remaining games.
The output directory must be new or empty, preserving previous evidence.

Outputs:

- `index.html`: labeled visual review table with boot and post-Start captures.
- `contact-sheet.png`: numeric mapper labels; boot is left, post-Start is right.
- `report.json`: ROM SHA-256, input/capture schedule, status, notes, frame metrics.
- Per-mapper PNGs, `metrics.json`, and worker logs.

Uniform captures (at least 99.5% one palette index) and lack of visual activity
in 60-frame samples are flagged as suspicious. These are heuristics: legitimate
static screens and fade frames need human review; animated garbage can evade
both checks. Successful capture is labeled `captured-needs-review`, never a
correctness pass. No screenshot is automatically accepted as a golden baseline.

Exit status is 1 for errors, timeouts, suspicious output, or baseline differences;
2 for missing representatives; otherwise 0. `--allow-missing` suppresses only
exit 2 and leaves all gaps visible in the report. Exit 0 means the smoke checks
completed without flags, not that rendering has been independently verified.

After visually reviewing output against a trusted emulator with matching ROM,
startup state, and inputs, preserve that directory and explicitly compare a new
run using `--baseline /path/to/reviewed-run`. Comparison checks raw palette-index
frame hashes and metrics, not RGB hues. Different ROM hashes or replay schedules
are incompatible, not silently compared. Trusted-emulator captures must be
reviewed separately; this tool does not integrate or install another emulator.

Runner checks (no commercial ROMs required):

```sh
python3 -m unittest discover -s tests/mapper-smoke
cargo test -p nes-render --bin mapper_smoke_capture
```

See [the initial visual review](initial-review.md) for the first run and follow-up candidates.

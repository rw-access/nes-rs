#!/usr/bin/env python3
"""Opt-in, process-isolated game smoke tests. Uses only the Python standard library."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys


def read_cases(path):
    cases = []
    seen = set()
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if not line or line.startswith('#'):
            continue
        fields = line.split('\t')
        if len(fields) != 6:
            raise ValueError(f'{path}:{number}: expected six tab-separated fields')
        mapper, rom, boot, start, end, notes = fields
        mapper, boot, start, end = map(int, (mapper, boot, start, end))
        if mapper in seen or not 0 <= mapper <= 255:
            raise ValueError(f'{path}:{number}: duplicate or invalid mapper')
        if not 0 < boot < start < start + 1 < end:
            raise ValueError(f'{path}:{number}: invalid capture/input timeline')
        if rom and (Path(rom).is_absolute() or '..' in Path(rom).parts):
            raise ValueError(f'{path}:{number}: ROM must be relative to ROM root')
        seen.add(mapper)
        cases.append(dict(mapper=mapper, rom=rom, boot=boot, start=start, end=end, notes=notes))
    if not cases:
        raise ValueError('empty manifest')
    return cases


def compare(result, previous):
    """Never compare captures with different ROM identities or input schedules."""
    identity = ('rom_sha256', 'boot', 'start', 'end', 'start_buttons')
    if previous is None or any(result.get(k) != previous.get(k) for k in identity):
        return 'incompatible'
    current = result.get('metrics', {}).get('stages', [])
    old = previous.get('metrics', {}).get('stages', [])
    if len(current) != 2 or len(old) != 2:
        return 'incompatible'
    return 'match' if current == old else 'changed'


def write_report(out, results):
    (out / 'report.json').write_text(json.dumps({'results': results}, indent=2) + '\n')
    rows = []
    for r in results:
        images = ''.join(f'<td><img width="256" height="240" src="{r["mapper"]:03}/{stage}.png" alt="{stage}"></td>'
                         if (out / f'{r["mapper"]:03}' / f'{stage}.png').exists() else '<td>—</td>'
                         for stage in ('boot', 'post-start'))
        label = f'{r["mapper"]:03}: {r["rom"] or "No representative"}'
        details = '; '.join([r['notes'], *r.get('warnings', []), r.get('error', ''), r.get('comparison', '')])
        rows.append(f'<tr><td>{html.escape(label)}<br><b>{html.escape(r["status"])}</b><br>{html.escape(details)}</td>{images}</tr>')
    (out / 'index.html').write_text('''<!doctype html><meta charset="utf-8"><title>Mapper smoke review</title>
<style>body{background:#181818;color:#eee;font:16px sans-serif}td{padding:8px;vertical-align:top}img{image-rendering:pixelated}table{border-collapse:collapse}tr{border-bottom:1px solid #555}</style>
<h1>Mapper smoke review</h1><p>Captured output requires visual review. Successful execution does not establish correctness. No reference baseline is accepted automatically.</p>
<table><tr><th>Mapper / result</th><th>Boot</th><th>After Start</th></tr>''' + ''.join(rows) + '</table>\n')


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rom-root', required=True, type=Path)
    parser.add_argument('--manifest', type=Path, default=root / 'tests/mapper-smoke/cases.tsv')
    parser.add_argument('--output', type=Path, required=True, help='new or empty output directory')
    parser.add_argument('--binary', type=Path, default=root / 'target/release/mapper_smoke_capture')
    parser.add_argument('--timeout', type=float, default=30, help='wall-clock seconds per game')
    parser.add_argument('--allow-missing', action='store_true', help='report gaps without returning exit 2')
    parser.add_argument('--baseline', type=Path, help='explicitly reviewed previous output directory')
    args = parser.parse_args()
    cases = read_cases(args.manifest)
    binary = args.binary.resolve()
    if not binary.is_file() or args.timeout <= 0:
        parser.error('build mapper_smoke_capture first; timeout must be positive')
    out = args.output.resolve()
    if out.exists() and any(out.iterdir()):
        parser.error('output directory must be empty; preserve previous evidence')
    previous = {}
    if args.baseline:
        previous = {r['mapper']: r for r in json.loads((args.baseline / 'report.json').read_text())['results']}
    out.mkdir(parents=True, exist_ok=True)
    results = []
    for case in cases:
        r = dict(case, start_buttons=8, warnings=[])
        results.append(r)
        rom = args.rom_root / case['rom']
        if not case['rom'] or not rom.is_file():
            r['status'] = 'missing'
        else:
            case_out = out / f'{case["mapper"]:03}'
            case_out.mkdir()
            r['rom_sha256'] = hashlib.sha256(rom.read_bytes()).hexdigest()
            command = [str(binary), str(case['mapper']), str(rom.resolve()), str(case['boot']),
                       str(case['start']), str(case['end']), str(case_out), '8']
            try:
                process = subprocess.run(command, capture_output=True, text=True, timeout=args.timeout)
                (case_out / 'worker.log').write_text(process.stdout + process.stderr)
                if process.returncode:
                    r.update(status='error', error=f'worker exit {process.returncode}; see worker.log')
                else:
                    r['metrics'] = json.loads((case_out / 'metrics.json').read_text())
                    for stage in r['metrics']['stages']:
                        if stage['dominant_fraction'] >= .995:
                            r['warnings'].append(f'{stage["stage"]}: nearly uniform frame')
                    if r['metrics']['sampled_unique_frames'] <= 1:
                        r['warnings'].append('no visual activity in 60-frame samples')
                    r['status'] = 'suspicious' if r['warnings'] else 'captured-needs-review'
                    if args.baseline:
                        r['comparison'] = compare(r, previous.get(case['mapper']))
                        if r['comparison'] != 'match':
                            r['status'] = 'baseline-' + r['comparison']
            except subprocess.TimeoutExpired:
                r.update(status='timeout', error=f'exceeded {args.timeout:g} seconds; worker killed')
            except (OSError, ValueError) as error:
                r.update(status='error', error=str(error))
        print(f'{case["mapper"]:03} {r["status"]}: {case["rom"] or case["notes"]}', flush=True)
        write_report(out, results)
    directories = [str(out / f'{r["mapper"]:03}') for r in results
                   if (out / f'{r["mapper"]:03}' / 'boot.png').exists()]
    if directories:
        subprocess.run([str(binary), '--sheet', str(out / 'contact-sheet.png'), *directories], check=True, timeout=30)
    print(f'Review: {out / "index.html"}')
    failures = any(r['status'] not in ('missing', 'captured-needs-review') for r in results)
    return 1 if failures else 2 if not args.allow_missing and any(r['status'] == 'missing' for r in results) else 0


if __name__ == '__main__':
    sys.exit(main())

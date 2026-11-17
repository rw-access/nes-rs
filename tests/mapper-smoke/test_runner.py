"""Dependency-free checks: python3 -m unittest discover -s tests/mapper-smoke."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('mapper_smoke', ROOT / 'scripts/mapper_smoke.py')
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class SmokeRunnerTests(unittest.TestCase):
    def test_manifest_rejects_duplicate_mappers_and_path_escape(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'cases.tsv'
            for contents in ('0\t../outside.nes\t10\t11\t20\t\n',
                             '0\ta.nes\t10\t11\t20\t\n0\tb.nes\t10\t11\t20\t\n'):
                path.write_text(contents)
                with self.assertRaises(ValueError):
                    RUNNER.read_cases(path)

    def test_baseline_requires_matching_rom_and_input_schedule(self):
        record = dict(rom_sha256='abc', boot=10, start=11, end=20, start_buttons=8,
                      metrics={'stages': [{'hash': 'a'}, {'hash': 'b'}]})
        self.assertEqual(RUNNER.compare(record, dict(record)), 'match')
        self.assertEqual(RUNNER.compare(record, dict(record, rom_sha256='other')), 'incompatible')
        self.assertEqual(RUNNER.compare(record, dict(record, start=12)), 'incompatible')
        changed = dict(record, metrics={'stages': [{'hash': 'a'}, {'hash': 'c'}]})
        self.assertEqual(RUNNER.compare(record, changed), 'changed')

    def test_timeout_does_not_prevent_following_case_or_report(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            worker = root / 'worker'
            worker.write_text('#!' + sys.executable + '\nimport sys,time\n'
                              'if sys.argv[1] == "0": time.sleep(5)\n'
                              'sys.exit(3)\n')
            worker.chmod(0o700)
            (root / 'a.nes').write_bytes(b'test')
            manifest = root / 'cases.tsv'
            manifest.write_text('0\ta.nes\t10\t11\t20\t\n1\ta.nes\t10\t11\t20\t\n')
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/mapper_smoke.py'),
                                     '--rom-root', str(root), '--manifest', str(manifest),
                                     '--binary', str(worker), '--output', str(root / 'out'),
                                     '--timeout', '0.3'], capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 1, result.stderr)
            records = json.loads((root / 'out/report.json').read_text())['results']
            self.assertEqual([r['status'] for r in records], ['timeout', 'error'])
            self.assertTrue((root / 'out/index.html').exists())


if __name__ == '__main__':
    unittest.main()

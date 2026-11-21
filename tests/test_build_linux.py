"""The Linux AppImage's file modes (build_linux.open_mode): anyone may read its files and run its programs, as the
AppImage catalog's test does as a user who owns none of them; only the owner writes."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
spec = importlib.util.spec_from_file_location('build_linux', ROOT / 'scripts/build_linux.py')
build_linux = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_linux)


class OpenModeTests(unittest.TestCase):
    def test_everyone_reads_the_files_and_runs_the_programs_and_folders(self):
        cases = {
            # Tauri's cached AppRun, packed as AppRun.wrapped: refused to anyone but its owner and group.
            (0o770, False): 0o755,
            (0o700, False): 0o755,
            (0o777, False): 0o755,
            (0o755, False): 0o755,
            (0o600, False): 0o644,
            (0o664, False): 0o644,
            (0o644, False): 0o644,
            (0o700, True): 0o755,
            (0o775, True): 0o755,
        }
        for (mode, directory), wanted in cases.items():
            self.assertEqual(oct(build_linux.open_mode(mode, directory)), oct(wanted), (oct(mode), directory))


if __name__ == '__main__':
    unittest.main()

"""Process ids read back from files (Antigravity's notes, the panel's state): a damaged one is no process."""
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import swiftbar_antigravity as antigravity


class ProcessIdTests(unittest.TestCase):
    def test_a_damaged_saved_process_id_is_no_process(self):
        # PROC-1: kill(0, 0) answers for this process's own group; past a positive pid_t, os.kill overflows.
        self.assertTrue(antigravity.alive(os.getpid()))
        for pid in (0, -1, 2**31, 2**32 - 1, 2**40, True, 1.0, '1', None):
            with self.subTest(pid=pid):self.assertFalse(antigravity.alive(pid))


if __name__ == '__main__':
    unittest.main()

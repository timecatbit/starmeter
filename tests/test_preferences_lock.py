"""Preferences changed by several processes at once (the app, the command line, the SwiftBar entry): none is lost."""
import multiprocessing
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import swiftbar_preferences as preferences

KEYS = ('chatgpt', 'claude-subscription', 'cursor-subscription', 'gemini-subscription')


def change(directory, key, times):
    for n in range(1, times + 1):
        preferences.update(directory, 'subscription-price', key, str(n))


class PreferencesLockTests(unittest.TestCase):
    def test_changes_made_at_once_from_several_processes_all_stay(self):
        with tempfile.TemporaryDirectory() as directory:
            workers = [multiprocessing.Process(target=change, args=(directory, key, 25)) for key in KEYS]
            for worker in workers:worker.start()
            for worker in workers:worker.join(60)
            self.assertTrue(all(worker.exitcode == 0 for worker in workers))
            self.assertEqual(preferences.read(directory)['subscriptionPrices'], {key: 25.0 for key in KEYS})


if __name__ == '__main__':
    unittest.main()

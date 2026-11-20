import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import swiftbar_sources as sources

spec = importlib.util.spec_from_file_location('update_pricing', ROOT / 'scripts/update_pricing.py')
update = importlib.util.module_from_spec(spec);spec.loader.exec_module(update)

TEXT = {'input': ['text'], 'output': ['text']}


class UpdatePricingTests(unittest.TestCase):

    def test_the_auto_review_timeline_is_read_where_ccusage_keeps_it(self):
        with tempfile.TemporaryDirectory() as folder:
            repo = Path(folder)
            codex = repo / 'rust/adapters/codex/src'
            (codex / 'parser').mkdir(parents=True)
            # Since November 2026: a constant in parser/model.rs, the last entry naming the default model's constant.
            (codex / 'parser/model.rs').write_text('const CODEX_AUTO_REVIEW_DEFAULT_MODEL: &str = "gpt-5";\n'
                                                   'const CODEX_AUTO_REVIEW_FALLBACKS: &[CodexAutoReviewFallback] = &[\n'
                                                   '    CodexAutoReviewFallback::new((2026, 7, 30), "gpt-5.6-luna"),\n'
                                                   '    CodexAutoReviewFallback::new((2025, 8, 7), CODEX_AUTO_REVIEW_DEFAULT_MODEL),\n];\n')
            self.assertEqual(update.auto_review_fallbacks(repo),
                             [{'releasedOn': '2026-07-30', 'model': 'gpt-5.6-luna'}, {'releasedOn': '2025-08-07', 'model': 'gpt-5'}])
            # Before: a JSON file, taken as it is.
            (codex / 'codex-auto-review-fallbacks.json').write_text('[{"releasedOn": "2026-03-05", "model": "gpt-5.4"}]')
            self.assertEqual(update.auto_review_fallbacks(repo), [{'releasedOn': '2026-03-05', 'model': 'gpt-5.4'}])
            # Neither: the update stops with a reason.
            (codex / 'codex-auto-review-fallbacks.json').unlink()
            (codex / 'parser/model.rs').write_text('')
            with self.assertRaises(SystemExit):
                update.auto_review_fallbacks(repo)

    def test_model_aliases_are_read_where_ccusage_keeps_them(self):
        with tempfile.TemporaryDirectory() as folder:
            repo = Path(folder)
            source = repo / 'rust/crates/ccusage-core/src'
            (source / 'pricing').mkdir(parents=True)
            # Before November 2026: a match in pricing.rs.
            (source / 'pricing.rs').write_text('fn other() {}\n\nfn pricing_alias(model: &str) -> Option<&str> {\n'
                                               '    match model {\n        "gpt-5.6" => Some("gpt-5.6-sol"),\n'
                                               '        "gpt-reserve" => Some("gpt-5.6-luna"),\n        _ => None,\n    }\n}\n')
            self.assertEqual(update.model_aliases(repo), {'gpt-5.6': 'gpt-5.6-sol', 'gpt-reserve': 'gpt-5.6-luna'})
            # Since: the ALIASES table in pricing/data.rs, which pricing.rs no longer defines.
            (source / 'pricing.rs').write_text('use data::ALIASES;\n')
            (source / 'pricing/data.rs').write_text('pub(super) static OTHER: &[(&str, &[&str])] = &[("qwen3-32b", &["thinking"])];\n\n'
                                                    'pub(super) static ALIASES: &[(&str, &str)] = &[\n    ("gpt-reserve", "gpt-5.6-luna"),\n'
                                                    '    ("gpt-5.6", "gpt-5.6-sol"),\n    ("gpt-5.3-spark", "gpt-5.3-codex-spark"),\n];\n')
            self.assertEqual(update.model_aliases(repo), {'gpt-reserve': 'gpt-5.6-luna', 'gpt-5.6': 'gpt-5.6-sol', 'gpt-5.3-spark': 'gpt-5.3-codex-spark'})
            # Neither: the update stops with a reason, not an IndexError.
            (source / 'pricing/data.rs').write_text('')
            with self.assertRaises(SystemExit) as stopped:
                update.model_aliases(repo)
            self.assertIn('ALIASES', str(stopped.exception))
    def test_models_dev_adds_only_newer_openai_text_models_ccusage_lacks(self):
        snapshot = {'gpt-6.1-sol': {'cost': {'input': 2, 'output': 10}}}
        cost = {'input': 2, 'output': 10, 'cache_read': 0.1, 'cache_write': 2.5,
                'tiers': [{'input': 4, 'output': 15, 'cache_read': 0.2, 'cache_write': 5, 'tier': {'type': 'context', 'size': 272000}}]}
        models = {
            'gpt-6.2-sol': {'cost': cost, 'last_updated': '2026-11-17', 'modalities': TEXT},
            # ccusage already prices it, at whatever models.dev says now.
            'gpt-6.1-sol': {'cost': {'input': 99, 'output': 99}, 'last_updated': '2026-11-17', 'modalities': TEXT},
            # Older than ccusage's snapshot and still absent: ccusage left it out on purpose.
            'gpt-image-1.5': {'cost': {'input': 5, 'output': 10}, 'last_updated': '2025-11-25', 'modalities': TEXT},
            'gpt-image-3': {'cost': {'input': 5, 'output': 10}, 'last_updated': '2026-11-17', 'modalities': {'output': ['image']}},
            'gpt-6.2-broken': {'cost': {'input': 'free', 'output': 10}, 'last_updated': '2026-11-17', 'modalities': TEXT},
            'gpt-6.2-unpriced': {'last_updated': '2026-11-17', 'modalities': TEXT},
            'whisper-2': {'cost': {'input': 1, 'output': 1}, 'last_updated': '2026-11-17', 'modalities': TEXT},
        }
        api = {'openai': {'models': models}, 'openrouter': {'models': {'gpt-6.3-sol': {'cost': cost, 'last_updated': '2026-11-18', 'modalities': TEXT}}}}
        added = update.newer_openai(api, snapshot, '2026-11-16')
        self.assertEqual(list(added), ['gpt-6.2-sol'], "Only OpenAI's own listing, never a reseller's")
        table = update.table(dict(snapshot, **added), update.OPENAI)
        self.assertEqual(table['gpt-6.2-sol'], {'input': 2, 'output': 10, 'cacheRead': 0.1, 'cacheWrite': 2.5,
                                                'longContext': {'input': 4, 'output': 15, 'cacheRead': 0.2, 'cacheWrite': 5, 'threshold': 272000}})
        self.assertEqual(table['gpt-6.1-sol']['input'], 2)
        self.assertEqual(update.newer_openai(api, snapshot, '2026-11-17'), {}, 'Updated the day the snapshot was built: left to ccusage')

    def test_a_changed_table_always_reads_newer_and_an_unchanged_one_keeps_its_stamp(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'codex-pricing.json'
            source = lambda: {'project': 'ccusage', 'commit': 'a' * 40, 'committedAt': 'x', 'license': 'MIT'}
            first = update.stamped(path, {'source': source(), 'models': {'gpt-6.1-sol': 1}}, '2026-11-16T21:23:18+01:00')
            self.assertEqual(first['source']['committedAt'], '2026-11-16T21:23:18+01:00')
            path.write_text(json.dumps(first))
            again = update.stamped(path, {'source': source(), 'models': {'gpt-6.1-sol': 1}}, '2026-11-16T21:23:18+01:00')
            self.assertEqual(again, first, 'Nothing changed, nothing to download')
            # models.dev adds a model while ccusage stands still: a second past the last stamp.
            added = update.stamped(path, {'source': source(), 'models': {'gpt-6.1-sol': 1, 'gpt-6.2-sol': 1}}, '2026-11-16T21:23:18+01:00')
            self.assertEqual(added['source']['committedAt'], '2026-11-16T21:23:19+01:00')
            self.assertGreater(sources.committed(added), sources.committed(first))
            path.write_text(json.dumps(added))
            # ccusage catches up with a newer commit: its own time again.
            caught = update.stamped(path, {'source': dict(source(), commit='b' * 40), 'models': {'gpt-6.1-sol': 1, 'gpt-6.2-sol': 1}}, '2026-11-19T08:00:00+01:00')
            self.assertEqual(caught['source']['committedAt'], '2026-11-19T08:00:00+01:00')

    def test_the_bundled_table_prices_gpt_6_2(self):
        table = sources.pricing('codex-pricing.json')
        self.assertTrue(sources.valid_pricing('codex-pricing.json', table))
        cost = sources.codex_cost({'model': 'gpt-6.2-sol', 'input': 100000, 'cached': 50000, 'output': 10000, 'writes': 0, 'timestamp': 1795400000})
        # 100K input at $2, 50K cached at $0.10, 10K output at $10, per million.
        self.assertAlmostEqual(cost, 0.305)


if __name__ == '__main__':
    unittest.main()

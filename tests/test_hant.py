"""繁體中文 (scripts/make_hant.py): the Traditional README is README.md as the converter writes it, and the converter
the panel and the engine share keeps its words."""
import importlib.util
import re
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('make_hant', ROOT / 'scripts/make_hant.py')
make_hant = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_hant)


class HantTests(unittest.TestCase):
    def test_the_traditional_readme_is_readme_md_converted(self):
        self.assertEqual((ROOT / 'README.zh-Hant.md').read_text(encoding='utf-8'), make_hant.readme(),
                         'README.md changed: run python3 scripts/make_hant.py --readme')

    def test_words_context_and_the_characters_taiwan_writes(self):
        chars, phrases = make_hant.tables()
        convert = lambda text: make_hant.convert(text, chars, phrases)
        self.assertEqual(convert('设置 · 余额 · 本周 · 项目 · 启动 · 面板里 · 每周 · Token'), '設定 · 餘額 · 本週 · 專案 · 啟動 · 面板裡 · 每週 · Token')
        # A word whose Taiwan form is longer would lay out wider: kept as ICU writes it.
        self.assertEqual(convert('刷新'), '刷新')

    def test_the_panel_the_engine_and_the_website_share_one_table(self):
        rust = (ROOT / 'crates/core/src/hant.rs').read_text(encoding='utf-8')
        ts = (ROOT / 'app/ui/src/lib/hant.ts').read_text(encoding='utf-8')
        site = (ROOT / 'docs/hant.js').read_text(encoding='utf-8')
        pairs = lambda text: text.split('PAIRS')[1].split('"')[1]
        phrases = lambda text: re.findall(r'^ *\[(".*?", ".*?")\],$', text, re.M)
        self.assertEqual(pairs(rust), pairs(ts))
        self.assertEqual(pairs(site), pairs(ts), 'run python3 scripts/make_hant.py')
        self.assertEqual(phrases(site), phrases(ts), 'run python3 scripts/make_hant.py')


if __name__ == '__main__':
    unittest.main()

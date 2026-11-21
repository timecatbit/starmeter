#!/usr/bin/env python3
"""The hand-drawn look's Chinese faces (外观 → 风格 → 手绘, app/ui/src/sketch.css): 霞鹜文楷 for Simplified Chinese,
霞鹜文楷 TC for Traditional, both in their Medium weight (the heaviest they come in: the user wanted the Chinese bold),
cut down to the Chinese characters the panel is likely to show and written as WOFF2 into app/ui/public/fonts, beside
their licence. Letters and figures stay in Chalkboard SE (the user's choice), so the
subsets hold no Latin at all, only the blocks in CHINESE, the same ranges sketch.css gives as their unicode-range.

The fonts are LXGW's (https://github.com/lxgw/LxgwWenKai, https://github.com/lxgw/LxgwWenkaiTC), under the SIL Open
Font License 1.1 with the author's additional permission to keep the reserved names on a subset converted to WOFF2
solely for web font delivery, not offered as an installable desktop font: the panel is a web page, and these files
only ever load into it. The originals are 26 MB and 15 MB; the panel keeps the common characters (GB2312's 6,763 for
Simplified, Big5's 5,401 common ones for Traditional), every character its own text uses (in both scripts, as
scripts/make_hant.py collects it), and the Chinese punctuation. A rarer character, in a session's title say, falls
back to the system's face.

Download the releases' LXGWWenKai-Medium.ttf and LXGWWenKaiTC-Medium.ttf, then, with fonttools and brotli installed
(`pip install fonttools brotli`; only this script needs them):

    python3 scripts/make_fonts.py LXGWWenKai-Medium.ttf LXGWWenKaiTC-Medium.ttf
"""
import importlib.util
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'app/ui/public/fonts'
PUNCTUATION = '，。、；：？！「」『』（）《》〈〉【】〔〕￥％＋－＝／～　'
# Kept in step with the unicode-range of 'Starmeter Kai' and 'Starmeter Kai TC' in app/ui/src/sketch.css.
CHINESE = [(0x2E80, 0x2FFF), (0x3000, 0x303F), (0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAFF), (0xFE30, 0xFE4F),
           (0xFF00, 0xFFEF)]


def chinese(c):
    return any(first <= ord(c) <= last for first, last in CHINESE)


def table(encoding, first, last):
    """The characters a double-byte encoding holds between two lead bytes (inclusive), every trail byte tried."""
    found = set()
    for lead in range(first, last + 1):
        for trail in list(range(0x40, 0x7F)) + list(range(0xA1, 0xFF)):
            try:
                found.update(bytes([lead, trail]).decode(encoding))
            except UnicodeDecodeError:
                pass
    return found


def panel_text():
    """Every Chinese character the panel's text uses, Simplified and converted to Traditional."""
    spec = importlib.util.spec_from_file_location('make_hant', ROOT / 'scripts/make_hant.py')
    make_hant = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(make_hant)
    runs = make_hant.corpus()
    chars, phrases = make_hant.tables()
    return {c for run in runs for c in run} | {c for run in runs for c in make_hant.convert(run, chars, phrases)}


def subset(source, characters, out):
    from fontTools import subset as fonts
    options = fonts.Options()
    options.flavor = 'woff2'
    options.layout_features = ['*']
    options.name_IDs = ['*']
    options.name_languages = ['*']
    options.notdef_outline = True
    font = fonts.load_font(source, options)
    keep = fonts.Subsetter(options)
    keep.populate(unicodes=sorted(ord(c) for c in characters))
    keep.subset(font)
    fonts.save_font(font, out, options)
    print('%s: %d characters, %.1f MB' % (out.name, len(characters), out.stat().st_size / 1e6))


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    own = panel_text() | set(PUNCTUATION)
    simplified = {c for c in table('gb2312', 0xB0, 0xF7) | own if chinese(c)}
    traditional = {c for c in table('big5', 0xA4, 0xC6) | own if chinese(c)}
    OUT.mkdir(parents=True, exist_ok=True)
    subset(sys.argv[1], simplified, OUT / 'LXGWWenKai-Medium.subset.woff2')
    subset(sys.argv[2], traditional, OUT / 'LXGWWenKaiTC-Medium.subset.woff2')


if __name__ == '__main__':
    main()

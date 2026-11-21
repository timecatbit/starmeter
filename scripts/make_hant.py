#!/usr/bin/env python3
"""Builds the Simplified-to-Traditional Chinese tables (繁體中文) the panel and the engine convert their Chinese with.

Everything Starmeter writes is Simplified Chinese at the source; 繁體中文 is that text converted at the edges, as English
is translated. The conversion is the system's ICU (Hans-Hant, through scripts/hant_icu.swift, so macOS only), taken
two ways: every character on its own (for any text, a session's title among it), and every run of Chinese in the
sources in context, kept as a phrase wherever the context changes a character (头发 → 頭髮, 一只 → 一隻). On top come
the Taiwan forms of a few characters ICU writes otherwise (啓 → 啟) and of the words software is written with there
(设置 → 設定, 软件 → 軟體); a replaced word keeps its length, so no page lays out wider.

Run it again after adding Chinese text with a character or word it doesn't know:

    python3 scripts/make_hant.py

It writes app/ui/src/lib/hant.ts and crates/core/src/hant.rs from the same tables. After README.md changes:

    python3 scripts/make_hant.py --readme

writes README.zh-Hant.md from it with the tables already built (no ICU needed); tests/test_hant.py checks it is
current.
"""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCES = [ROOT / 'crates', ROOT / 'app/src-tauri/src', ROOT / 'app/ui/src', ROOT / 'app/ui/fixtures/make-fixtures.mjs',
           ROOT / 'starmeter.30s.sh', ROOT / 'README.md', ROOT / 'CHANGELOG.md', ROOT / 'docs/index.html', ROOT / 'docs/site.js']
OUTPUTS = (ROOT / 'app/ui/src/lib/hant.ts', ROOT / 'crates/core/src/hant.rs')
CHINESE = re.compile(r'[\u3400-\u4dbf\u4e00-\u9fff]+')

# Characters ICU writes in a form Taiwan doesn't (啓 for 啟).
VARIANTS = {'啓': '啟', '鈎': '鉤', '綫': '線', '衆': '眾', '爲': '為', '僞': '偽', '裏': '裡', '眞': '真', '敎': '教',
            '値': '值', '吿': '告', '靑': '青', '淸': '清', '鷄': '雞', '麪': '麵', '峯': '峰', '羣': '群', '牀': '床'}
# Characters ICU gets wrong whatever their context (沉 is not 沈), or leaves where Taiwan writes another (面板里 is
# 面板裡; Starmeter's 里 is always "in", never a distance): fixed in every phrase too.
FORCED = {'沉': '沉', '里': '裡'}
# A character's form on its own, where ICU's is the rarer one; in context the phrases decide (一只 is 一隻).
SINGLE = {'只': '只', '系': '系', '尽': '盡', '里': '裡', '划': '劃'}

# The words software is written with in Taiwan, longest first where one holds another. Each keeps its length, or is
# shorter: a page laid out for the Simplified text never grows.
WORDS = {
    '设置': '設定', '软件': '軟體', '硬件': '硬體', '默认': '預設', '文件夹': '資料夾', '文件': '檔案', '信息': '資訊',
    '网络': '網路', '局域网': '區域網', '链接': '連結', '保存': '儲存', '加载': '載入', '服务器': '伺服器', '数据': '資料',
    '内存': '記憶體', '缓存': '快取', '插件': '外掛', '模块': '模組', '二维码': 'QR碼', '菜单': '選單', '屏幕': '螢幕',
    '鼠标': '滑鼠', '用户': '使用者', '视频': '影片', '搜索': '搜尋', '图标': '圖示', '窗口': '視窗', '运行': '執行',
    '登录': '登入', '刷新': '重新整理', '项目': '專案', '账户': '帳戶', '账号': '帳號', '启动': '啟動', '回复': '回覆',
    '导出': '匯出', '导入': '匯入', '粘贴': '貼上', '创建': '建立', '代码': '程式碼', '程序': '程式', '终端': '終端機',
    '命令行': '命令列', '字符': '字元', '打印': '列印', '剪贴板': '剪貼簿', '设备': '裝置', '质量': '品質',
    '优化': '最佳化', '支持': '支援', '线程': '執行緒', '日志': '日誌', '激活': '啟用', '卸载': '解除安裝',
    '源代码': '原始碼', '默认值': '預設值', '高级': '進階', '通过': '透過', '兼容': '相容', '录制': '錄製',
    '每周': '每週', '上周': '上週', '下周': '下週', '周报': '週報', '一周': '一週', '两周': '兩週', '周末': '週末',
    '配置': '設定', '实时': '即時', '运行中': '執行中', '自定义': '自訂', '交互': '互動', '界面': '介面', '屏幕截图': '螢幕截圖',
}
# Words whose Taiwan form would be longer than the Simplified one (a label laid out for it would wrap): kept as ICU
# writes them.
LONGER = {'刷新', '内存', '代码', '终端', '优化', '线程', '卸载', '源代码'}


def icu(lines):
    """Each line through ICU's Hans-Hant, in one run of the helper."""
    done = subprocess.run(['swift', str(ROOT / 'scripts/hant_icu.swift')], input='\n'.join(lines) + '\n',
                          capture_output=True, text=True, check=True)
    out = done.stdout.split('\n')[:len(lines)]
    if len(out) != len(lines):
        raise SystemExit('ICU gave %d lines for %d' % (len(out), len(lines)))
    return out


def corpus():
    """Every run of Chinese in the sources (the tables themselves left out)."""
    runs = set()
    for source in SOURCES:
        files = [source] if source.is_file() else [path for path in source.rglob('*') if path.is_file()]
        for path in files:
            if path in OUTPUTS or path.suffix not in ('.rs', '.ts', '.svelte', '.mjs', '.sh', '.md', '.html', '.js'):
                continue
            runs.update(CHINESE.findall(path.read_text(encoding='utf-8', errors='ignore')))
    return sorted(runs)


def fix(text, simplified=None):
    """ICU's Traditional text with Taiwan's forms, and the characters it always gets wrong put right."""
    text = ''.join(VARIANTS.get(character, character) for character in text)
    if simplified is not None and len(simplified) == len(text):
        text = ''.join(FORCED.get(source, target) for source, target in zip(simplified, text))
    return text


def main():
    singles = [chr(code) for code in range(0x4e00, 0xa000)] + [chr(code) for code in range(0x3400, 0x4dc0)]
    converted = icu(singles)
    chars = {}
    for simplified, traditional in zip(singles, converted):
        traditional = fix(SINGLE.get(simplified, traditional), simplified)
        if len(traditional) == 1 and traditional != simplified:
            chars[simplified] = traditional
    by_char = lambda text: ''.join(chars.get(character, character) for character in text)

    runs = [run for run in corpus() if len(run) > 1]
    whole = {run: fix(text, run) for run, text in zip(runs, icu(runs))}
    # Where the context changes a character, the smallest window around it that ICU converts the same way.
    windows = {}
    for run, traditional in whole.items():
        if len(traditional) != len(run):
            windows[run] = None
            continue
        for index, character in enumerate(traditional):
            if character != by_char(run[index]):
                for size in (2, 3, 4):
                    for start in range(max(0, index - size + 1), min(index, len(run) - size) + 1):
                        windows.setdefault(run[start:start + size], None)
    candidates = sorted(windows)
    phrases = {}
    for window, traditional in ((window, fix(text, window)) for window, text in zip(candidates, icu(candidates))):
        if len(traditional) == len(window) and traditional != by_char(window):
            phrases[window] = traditional
    # Keep a window only where it is what the whole run says; the shortest that covers each changed character.
    kept = {}
    for run, traditional in whole.items():
        if len(traditional) != len(run):
            kept[run] = traditional
            continue
        for index, character in enumerate(traditional):
            if character == by_char(run[index]):
                continue
            for size in (2, 3, 4):
                found = next((run[start:start + size] for start in range(max(0, index - size + 1), min(index, len(run) - size) + 1)
                              if phrases.get(run[start:start + size]) == traditional[start:start + size]), None)
                if found:
                    kept[found] = phrases[found]
                    break
            else:
                kept[run] = traditional
    for simplified, traditional in WORDS.items():
        if simplified in LONGER and len(traditional) > len(simplified):
            continue
        kept[simplified] = traditional
    write(chars, kept)
    print('%d characters, %d phrases' % (len(chars), len(kept)))


def write(chars, phrases):
    pairs = ''.join(simplified + traditional for simplified, traditional in sorted(chars.items()))
    ordered = sorted(phrases.items(), key=lambda item: (-len(item[0]), item[0]))
    longest = max(len(simplified) for simplified, _ in ordered)
    note = 'Generated by scripts/make_hant.py from the system\'s ICU (Hans-Hant) and its Taiwan words; run it again, don\'t edit.'
    ts = [
        '// 繁體中文: Simplified Chinese to Traditional, as the panel converts its text (see i18n.svelte.ts).',
        '// ' + note,
        'const PAIRS = ' + json_string(pairs) + ';',
        'const PHRASES: [string, string][] = [',
        *['  [%s, %s],' % (json_string(a), json_string(b)) for a, b in ordered],
        '];',
        'const LONGEST = %d;' % longest,
        '',
        'let chars: Map<string, string> | null = null;',
        'let phrases: Map<string, string> | null = null;',
        '',
        '/** The text in Traditional Chinese: the longest phrase known at each place, else each character on its own. */',
        'export function toHant(text: string): string {',
        '  if (!/[\\u3400-\\u9fff]/.test(text)) return text;',
        '  if (!chars) {',
        '    chars = new Map();',
        '    for (let i = 0; i < PAIRS.length; i += 2) chars.set(PAIRS[i], PAIRS[i + 1]);',
        '    phrases = new Map(PHRASES);',
        '  }',
        '  let out = \'\';',
        '  let i = 0;',
        '  while (i < text.length) {',
        '    let matched = false;',
        '    for (let size = Math.min(LONGEST, text.length - i); size > 1; size--) {',
        '      const found = phrases!.get(text.slice(i, i + size));',
        '      if (found !== undefined) { out += found; i += size; matched = true; break; }',
        '    }',
        '    if (matched) continue;',
        '    out += chars.get(text[i]) ?? text[i];',
        '    i++;',
        '  }',
        '  return out;',
        '}',
        '',
    ]
    OUTPUTS[0].write_text('\n'.join(ts), encoding='utf-8')
    rs = [
        '//! 繁體中文: Simplified Chinese to Traditional, as the engine converts its text for a Traditional Chinese panel,',
        '//! menu or command line.',
        '//! ' + note,
        '',
        'use std::collections::HashMap;',
        'use std::sync::OnceLock;',
        '',
        'const PAIRS: &str = ' + json_string(pairs) + ';',
        'const PHRASES: &[(&str, &str)] = &[',
        *['    (%s, %s),' % (json_string(a), json_string(b)) for a, b in ordered],
        '];',
        'const LONGEST: usize = %d;' % longest,
        '',
        '/// Each character\'s form, and each phrase\'s.',
        'type Tables = (HashMap<char, char>, HashMap<&\'static str, &\'static str>);',
        '',
        'fn tables() -> &\'static Tables {',
        '    static TABLES: OnceLock<Tables> = OnceLock::new();',
        '    TABLES.get_or_init(|| {',
        '        let letters: Vec<char> = PAIRS.chars().collect();',
        '        let chars = letters.chunks(2).map(|pair| (pair[0], pair[1])).collect();',
        '        (chars, PHRASES.iter().copied().collect())',
        '    })',
        '}',
        '',
        '/// The text in Traditional Chinese: the longest phrase known at each place, else each character on its own.',
        'pub fn convert(text: &str) -> String {',
        '    if !text.chars().any(|c| (\'\\u{3400}\'..=\'\\u{9fff}\').contains(&c)) {',
        '        return text.to_string();',
        '    }',
        '    let (chars, phrases) = tables();',
        '    let letters: Vec<char> = text.chars().collect();',
        '    let mut out = String::with_capacity(text.len());',
        '    let mut i = 0;',
        '    while i < letters.len() {',
        '        let mut matched = false;',
        '        for size in (2..=LONGEST.min(letters.len() - i)).rev() {',
        '            let window: String = letters[i..i + size].iter().collect();',
        '            if let Some(found) = phrases.get(window.as_str()) {',
        '                out.push_str(found);',
        '                i += size;',
        '                matched = true;',
        '                break;',
        '            }',
        '        }',
        '        if !matched {',
        '            out.push(*chars.get(&letters[i]).unwrap_or(&letters[i]));',
        '            i += 1;',
        '        }',
        '    }',
        '    out',
        '}',
        '',
        '#[cfg(test)]',
        'mod tests {',
        '    #[test]',
        '    fn the_words_and_their_context_convert_as_taiwan_writes_them() {',
        '        // A word of Taiwan\'s, a character its context decides (余额, 本周), one ICU writes otherwise (启): and Latin as it is.',
        '        assert_eq!(super::convert("设置 · 余额 · 本周 · 项目 · 启动 · Token"), "設定 · 餘額 · 本週 · 專案 · 啟動 · Token");',
        '    }',
        '}',
        '',
    ]
    OUTPUTS[1].write_text('\n'.join(rs), encoding='utf-8')
    # As cargo fmt would write it, so the check finds nothing to change.
    subprocess.run(['rustfmt', '--edition', '2024', str(OUTPUTS[1])], check=False)


def json_string(text):
    import json
    return json.dumps(text, ensure_ascii=False)


def tables():
    """The character and phrase tables as hant.ts holds them."""
    import json
    text = OUTPUTS[0].read_text(encoding='utf-8')
    pairs = json.loads(re.search(r'const PAIRS = (".*?");', text).group(1))
    chars = {pairs[index]: pairs[index + 1] for index in range(0, len(pairs), 2)}
    phrases = dict(json.loads('[' + ','.join(re.findall(r'^  (\[.*\]),$', text, re.M)) + ']'))
    return chars, phrases


def convert(text, chars, phrases):
    """The text in Traditional Chinese, as hant.ts and hant.rs convert it."""
    longest = max(map(len, phrases))
    out, index = [], 0
    while index < len(text):
        for size in range(min(longest, len(text) - index), 1, -1):
            found = phrases.get(text[index:index + size])
            if found is not None:
                out.append(found)
                index += size
                break
        else:
            out.append(chars.get(text[index], text[index]))
            index += 1
    return ''.join(out)


def readme():
    """README.zh-Hant.md: README.md in Traditional Chinese, with its language links and its own screenshots."""
    chars, phrases = tables()
    text = convert((ROOT / 'README.md').read_text(encoding='utf-8'), chars, phrases)
    text = text.replace('<b>簡體中文</b> · <a href="README.zh-Hant.md">繁體中文</a> · <a href="README.en.md">English</a>',
                        '<a href="README.md">简体中文</a> · <b>繁體中文</b> · <a href="README.en.md">English</a>')
    text = text.replace('docs/images/', 'docs/images/zh-Hant/')
    return text


if __name__ == '__main__':
    import sys
    if '--readme' in sys.argv:
        (ROOT / 'README.zh-Hant.md').write_text(readme(), encoding='utf-8')
    else:
        main()

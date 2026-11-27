#!/usr/bin/env python3
"""Builds the Simplified-to-Traditional Chinese tables (繁體中文) the panel and the engine convert their Chinese with.

Everything Starmeter writes is Simplified Chinese at the source; 繁體中文 is that text converted at the edges, as English
is translated. The conversion is the system's ICU (Hans-Hant, through scripts/hant_icu.swift, so macOS only), taken
two ways: every character on its own (for any text, a session's title among it), and every run of Chinese in the
sources in context, kept as a phrase wherever the context changes a character (头发 → 頭髮, 一只 → 一隻, 本周 → 本週).
It converts characters only, never words (设置 stays 設置, 软件 軟件): the text reads the same wherever Traditional
Chinese is read, and every line keeps its length. On top come the common forms of a few characters ICU writes
otherwise (啓 → 啟) and the ones it gets wrong (沉 is not 沈, 录制 is 錄製).

Run it again after adding Chinese text with a character or word it doesn't know:

    python3 scripts/make_hant.py

It writes app/ui/src/lib/hant.ts, crates/core/src/hant.rs and docs/hant.js (the website's 繁體中文) from the same
tables. After README.md changes:

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
OUTPUTS = (ROOT / 'app/ui/src/lib/hant.ts', ROOT / 'crates/core/src/hant.rs', ROOT / 'docs/hant.js')
CHINESE = re.compile(r'[\u3400-\u4dbf\u4e00-\u9fff]+')

# Characters ICU writes in a rarer form than the one Traditional Chinese is usually printed in (啓 for 啟).
VARIANTS = {'啓': '啟', '鈎': '鉤', '綫': '線', '衆': '眾', '爲': '為', '僞': '偽', '裏': '裡', '眞': '真', '敎': '教',
            '値': '值', '吿': '告', '靑': '青', '淸': '清', '鷄': '雞', '麪': '麵', '峯': '峰', '羣': '群', '牀': '床', '竪': '豎'}
# Characters ICU gets wrong whatever their context (沉 is not 沈), or leaves in the rarer form (面板里 is 面板裡;
# Starmeter's 里 is always "in", never a distance): fixed in every phrase too.
FORCED = {'沉': '沉', '里': '裡'}
# A character's form on its own, where ICU's is the rarer one; in context the phrases decide (一只 is 一隻). Starmeter's
# 余 is always "left" (余 41,521 Credits), never the pronoun ICU reads a lone 余 as.
SINGLE = {'只': '只', '系': '系', '尽': '盡', '里': '裡', '划': '劃', '余': '餘'}

# Characters whose form their context decides, where ICU misses it: each phrase is the same length as the Simplified.
WORDS = {
    '录制': '錄製', '每周': '每週', '上周': '上週', '下周': '下週', '周报': '週報', '一周': '一週', '两周': '兩週',
    '周末': '週末',
}
# Words ICU finds across a boundary in a run, which Starmeter never means (整理发布 is not 理髮, 处理发送 not 處理髮送):
# each character there stays as it is on its own, so no phrase is made of them.
NOT_WORDS = ('理发',)


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
    """ICU's Traditional text in the common forms, and the characters it always gets wrong put right."""
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
    for run, traditional in whole.items():
        if len(traditional) == len(run):
            for word in NOT_WORDS:
                for start in (found.start() for found in re.finditer(word, run)):
                    traditional = traditional[:start] + by_char(word) + traditional[start + len(word):]
            whole[run] = traditional
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
        kept[simplified] = traditional
    write(chars, kept)
    print('%d characters, %d phrases' % (len(chars), len(kept)))


def write(chars, phrases):
    pairs = ''.join(simplified + traditional for simplified, traditional in sorted(chars.items()))
    ordered = sorted(phrases.items(), key=lambda item: (-len(item[0]), item[0]))
    longest = max(len(simplified) for simplified, _ in ordered)
    note = 'Generated by scripts/make_hant.py from the system\'s ICU (Hans-Hant); run it again, don\'t edit.'
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
        'const hantCache = new Map<string, string>();',
        '',
        '/** Where a path (/…, ~/…, C:\\…) that starts at `i` ends, or -1: a name on disk stays as it is written. */',
        'function pathEnd(text: string, i: number): number {',
        '  if (i > 0 && !/[\\s（(「『"\'：:=]/.test(text[i - 1])) return -1;',
        '  const slash = (c: string | undefined) => c === \'/\' || c === \'\\\\\';',
        '  const starts = slash(text[i]) || (text[i] === \'~\' && slash(text[i + 1])) || (/[A-Za-z]/.test(text[i]) && text[i + 1] === \':\' && slash(text[i + 2]));',
        '  if (!starts) return -1;',
        '  let end = i;',
        '  while (end < text.length && !/[\\s，。；、）)」』"\']/.test(text[end])) end++;',
        '  return end;',
        '}',
        '',
        '/** The text in Traditional Chinese: the longest phrase known at each place, else each character on its own; a path',
        ' *  in it stays as it is. Memoized so repeated snapshot localizations avoid re-scanning unchanged strings (PERF-4). */',
        'export function toHant(text: string): string {',
        '  const hit = hantCache.get(text);',
        '  if (hit !== undefined) return hit;',
        '  if (!/[\\u3400-\\u9fff]/.test(text)) return text;',
        '  if (!chars) {',
        '    chars = new Map();',
        '    for (let i = 0; i < PAIRS.length; i += 2) chars.set(PAIRS[i], PAIRS[i + 1]);',
        '    phrases = new Map(PHRASES);',
        '  }',
        '  let out = \'\';',
        '  let i = 0;',
        '  while (i < text.length) {',
        '    const end = pathEnd(text, i);',
        '    if (end >= 0) { out += text.slice(i, end); i = end; continue; }',
        '    let matched = false;',
        '    for (let size = Math.min(LONGEST, text.length - i); size > 1; size--) {',
        '      const found = phrases!.get(text.slice(i, i + size));',
        '      if (found !== undefined) { out += found; i += size; matched = true; break; }',
        '    }',
        '    if (matched) continue;',
        '    out += chars.get(text[i]) ?? text[i];',
        '    i++;',
        '  }',
        '  if (hantCache.size >= 4000) hantCache.clear();',
        '  hantCache.set(text, out);',
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
        '/// Where a path (/…, ~/…, C:\\\\…) that starts at `i` ends: a name on disk stays as it is written.',
        'fn path_end(letters: &[char], i: usize) -> Option<usize> {',
        '    if i > 0 && !(letters[i - 1].is_whitespace() || "（(「『\\"\'：:=".contains(letters[i - 1])) {',
        '        return None;',
        '    }',
        '    let slash = |at: usize| matches!(letters.get(at), Some(\'/\' | \'\\\\\'));',
        '    let starts = slash(i)',
        '        || (letters[i] == \'~\' && slash(i + 1))',
        '        || (letters[i].is_ascii_alphabetic() && letters.get(i + 1) == Some(&\':\') && slash(i + 2));',
        '    if !starts {',
        '        return None;',
        '    }',
        '    let ends = |c: char| c.is_whitespace() || "，。；、）)」』\\"\'".contains(c);',
        '    Some(letters[i..].iter().position(|&c| ends(c)).map_or(letters.len(), |at| i + at))',
        '}',
        '',
        '/// The text in Traditional Chinese: the longest phrase known at each place, else each character on its own; a path',
        '/// in it stays as it is.',
        'pub fn convert(text: &str) -> String {',
        '    if !text.chars().any(|c| (\'\\u{3400}\'..=\'\\u{9fff}\').contains(&c)) {',
        '        return text.to_string();',
        '    }',
        '    let (chars, phrases) = tables();',
        '    let letters: Vec<char> = text.chars().collect();',
        '    let mut out = String::with_capacity(text.len());',
        '    let mut i = 0;',
        '    while i < letters.len() {',
        '        if let Some(end) = path_end(&letters, i) {',
        '            out.extend(&letters[i..end]);',
        '            i = end;',
        '            continue;',
        '        }',
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
        '        // Characters only, never words; the context decides a character (余额, 本周); ICU\'s rarer form put right (启).',
        '        assert_eq!(super::convert("设置 · 余额 · 本周 · 项目 · 启动 · Token"), "設置 · 餘額 · 本週 · 項目 · 啟動 · Token");',
        '    }',
        '',
        '    #[test]',
        '    fn a_path_stays_as_it_is_written() {',
        '        // A folder on disk named in either script, or both: only the words around it convert.',
        '        assert_eq!(super::convert("读取目录：/Users/demo/软件開發/项目记录 · 设置"), "讀取目錄：/Users/demo/软件開發/项目记录 · 設置");',
        '        assert_eq!(super::convert("~/软件 和 C:\\\\项目\\\\记录"), "~/软件 和 C:\\\\项目\\\\记录");',
        '        assert_eq!(super::convert("5/6 在线 · 简体/繁体"), "5/6 在線 · 簡體/繁體");',
        '    }',
        '}',
        '',
    ]
    OUTPUTS[1].write_text('\n'.join(rs), encoding='utf-8')
    # As cargo fmt would write it, so the check finds nothing to change.
    subprocess.run(['rustfmt', '--edition', '2024', str(OUTPUTS[1])], check=False)
    # The website has no build step: plain JavaScript, loaded only when a visitor reads it in 繁體中文 (see site.js).
    js = [
        '// 繁體中文 for the website: Simplified Chinese to Traditional, as the panel converts its text (site.js uses it).',
        '// ' + note,
        '(function () {',
        '  var PAIRS = ' + json_string(pairs) + ';',
        '  var PHRASES = [',
        *['    [%s, %s],' % (json_string(a), json_string(b)) for a, b in ordered],
        '  ];',
        '  var LONGEST = %d;' % longest,
        '  var chars = null, phrases = null;',
        '  // Where a path (/…, ~/…, C:\\\\…) that starts at i ends, or -1: a name on disk stays as it is written.',
        '  function pathEnd(text, i) {',
        '    if (i > 0 && !/[\\s（(「『"\'：:=]/.test(text[i - 1])) return -1;',
        '    var slash = function (c) { return c === \'/\' || c === \'\\\\\'; };',
        '    var starts = slash(text[i]) || (text[i] === \'~\' && slash(text[i + 1])) || (/[A-Za-z]/.test(text[i]) && text[i + 1] === \':\' && slash(text[i + 2]));',
        '    if (!starts) return -1;',
        '    var end = i;',
        '    while (end < text.length && !/[\\s，。；、）)」』"\']/.test(text[end])) end++;',
        '    return end;',
        '  }',
        '  // The text in Traditional Chinese: the longest phrase known at each place, else each character on its own.',
        '  window.toHant = function (text) {',
        '    if (!/[\u3400-\u9fff]/.test(text)) return text;',
        '    if (!chars) {',
        '      chars = new Map();',
        '      for (var c = 0; c < PAIRS.length; c += 2) chars.set(PAIRS[c], PAIRS[c + 1]);',
        '      phrases = new Map(PHRASES);',
        '    }',
        '    var out = \'\', i = 0;',
        '    while (i < text.length) {',
        '      var end = pathEnd(text, i);',
        '      if (end >= 0) { out += text.slice(i, end); i = end; continue; }',
        '      var matched = false;',
        '      for (var size = Math.min(LONGEST, text.length - i); size > 1; size--) {',
        '        var found = phrases.get(text.slice(i, i + size));',
        '        if (found !== undefined) { out += found; i += size; matched = true; break; }',
        '      }',
        '      if (matched) continue;',
        '      out += chars.has(text[i]) ? chars.get(text[i]) : text[i];',
        '      i++;',
        '    }',
        '    return out;',
        '  };',
        '})();',
        '',
    ]
    OUTPUTS[2].write_text('\n'.join(js), encoding='utf-8')


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


OPENERS = '（(「『"\'：:='
CLOSERS = '，。；、）)」』"\''


def path_end(text, index):
    """Where a path (/…, ~/…, C:\\…) that starts at `index` ends, or None: a name on disk stays as it is written."""
    if index > 0 and not (text[index - 1].isspace() or text[index - 1] in OPENERS):
        return None
    slash = lambda at: at < len(text) and text[at] in '/\\'
    starts = slash(index) or (text[index] == '~' and slash(index + 1)) or (
        text[index].isascii() and text[index].isalpha() and index + 1 < len(text) and text[index + 1] == ':' and slash(index + 2))
    if not starts:
        return None
    end = index
    while end < len(text) and not (text[end].isspace() or text[end] in CLOSERS):
        end += 1
    return end


def convert(text, chars, phrases):
    """The text in Traditional Chinese, as hant.ts and hant.rs convert it; a path in it stays as it is."""
    longest = max(map(len, phrases))
    out, index = [], 0
    while index < len(text):
        end = path_end(text, index)
        if end is not None:
            out.append(text[index:end])
            index = end
            continue
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

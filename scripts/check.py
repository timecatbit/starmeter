#!/usr/bin/env python3
"""Checks Starmeter: Rust format, lints and tests; an unused-code scan of lib/ and scripts/; and the Python
tests, which also hold the Rust engine's probe against the Python reference. Does not run the live plugin.
--rust-only stops after the Rust checks (the Windows runner: the Python reference is written for macOS)."""
import argparse
import ast
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def unused_code():
    """No linter dependency: top-level functions, classes and imports nothing refers to."""
    files = sorted(ROOT.glob('lib/*.py')) + sorted(ROOT.glob('scripts/*.py'))
    trees = {path: ast.parse(path.read_text(encoding='utf-8'), str(path)) for path in files}
    used = {'main'}
    for tree in trees.values():
        for node in ast.walk(tree):
            if isinstance(node, ast.Name):
                used.add(node.id)
            elif isinstance(node, ast.Attribute):
                used.add(node.attr)
            elif isinstance(node, ast.ImportFrom):
                used.update(alias.name for alias in node.names)
    problems = []
    for path, tree in trees.items():
        local = {node.id for node in ast.walk(tree) if isinstance(node, ast.Name)}
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.ClassDef)) and node.name not in used:
                problems.append('%s:%d unused %s' % (path.relative_to(ROOT), node.lineno, node.name))
            if isinstance(node, (ast.Import, ast.ImportFrom)):
                for alias in node.names:
                    name = (alias.asname or alias.name).split('.')[0]
                    if name not in local:
                        problems.append('%s:%d unused import %s' % (path.relative_to(ROOT), node.lineno, name))
    return problems


def rust():
    """Built with the real environment: cargo keeps its registry and toolchains under the real home."""
    cargo = shutil.which('cargo') or str(Path.home() / '.cargo' / 'bin' / 'cargo')
    if not os.path.isfile(cargo):
        sys.exit('Install Rust first: https://rustup.rs')
    for command in (['fmt', '--all', '--check'], ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'],
                    ['test', '--workspace', '--locked'], ['build', '--locked', '-p', 'starmeter-probe', '-p', 'starmeter-engine']):
        subprocess.run([cargo] + command, cwd=ROOT, check=True)
    return ROOT / 'target' / 'debug' / ('starmeter-probe.exe' if os.name == 'nt' else 'starmeter-probe')


def python_tests(probe):
    with tempfile.TemporaryDirectory(prefix='starmeter-tests-') as scratch:
        private_prefixes = ('OCX_', 'KOMARI_', 'DEEPSEEK_', 'XAI_', 'MINIMAX_', 'CLAUDE_', 'OPENAI_', 'ANTHROPIC_', 'MOONSHOT_',
                            'GOOGLE_', 'KIMI_', 'CODEX_', 'GEMINI_', 'WORKBUDDY_', 'QODER_', 'QODERCN_', 'OPENCODE_', 'DSH_', 'MAGPIE_')
        environment = {k: v for k, v in os.environ.items() if not k.startswith(private_prefixes)}
        isolated_home = Path(scratch) / 'home'
        isolated_home.mkdir()
        environment.update(HOME=str(isolated_home), SWIFTBAR_PLUGIN_DATA_PATH=scratch, PYTHONDONTWRITEBYTECODE='1',
                           STARMETER_PROBE=str(probe))
        subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', str(ROOT / 'tests'), '-p', 'test_*.py', '-v'],
                       check=True, env=environment)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust-only', action='store_true', help='Run only the Rust checks.')
    args = parser.parse_args()
    if os.path.isfile('/bin/bash'):
        subprocess.run(['/bin/bash', '-p', '-n', str(ROOT / 'starmeter.30s.sh')], check=True)
    problems = unused_code()
    if problems:
        sys.exit('Unused code:\n' + '\n'.join(problems))
    probe = rust()
    if not args.rust_only:
        python_tests(probe)

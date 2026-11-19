#!/usr/bin/env python3
"""Build the Rust engine on macOS, Windows or Linux; Mac release builds carry both architectures."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
MAC_TARGETS = ('aarch64-apple-darwin', 'x86_64-apple-darwin')


def cargo():
    found = shutil.which('cargo') or str(Path.home() / '.cargo' / 'bin' / 'cargo')
    if not os.path.isfile(found):
        raise SystemExit('Install Rust first: https://rustup.rs')
    return found


def build_engine(output, universal=False):
    """The engine every host runs: SwiftBar's entry, the apps' worker and the command line. Universal builds
    run on Apple silicon and Intel Macs without a local compiler."""
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    name = 'starmeter-engine.exe' if os.name == 'nt' else 'starmeter-engine'
    command = [cargo(), 'build', '--release', '--locked', '-p', 'starmeter-engine']
    environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='13.0')
    if universal:
        if sys.platform != 'darwin':
            raise SystemExit('Universal builds are for macOS.')
        slices = []
        for target in MAC_TARGETS:
            subprocess.run(command + ['--target', target], cwd=ROOT, check=True, env=environment)
            slices.append(ROOT / 'target' / target / 'release' / name)
        built = ROOT / 'target' / 'universal' / name
        built.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(['lipo', '-create'] + [str(part) for part in slices] + ['-output', str(built)], check=True)
    else:
        subprocess.run(command, cwd=ROOT, check=True, env=environment)
        built = ROOT / 'target' / 'release' / name
    # Copied beside the target, then renamed over it: rewriting a running signed binary in place would kill it.
    staged = output.with_name('.' + output.name + '.new')
    shutil.copyfile(built, staged)
    staged.chmod(0o755)
    if sys.platform == 'darwin':
        subprocess.run(['codesign', '--force', '--sign', '-', str(staged)], check=True, capture_output=True)
    os.replace(staged, output)
    print(output)
    return output


def build(output, universal=False):
    """The panel app, built with Tauri once the app shell is in place."""
    raise SystemExit('The Tauri app is not built yet; only the engine is.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--universal', action='store_true', help='Build arm64 and x86_64 slices, as Mac release packages do.')
    parser.add_argument('--engine', type=Path, default=ROOT / 'build' / 'starmeter-engine', help='Where the engine program goes.')
    args = parser.parse_args()
    build_engine(args.engine, args.universal)

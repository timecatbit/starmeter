#!/usr/bin/env python3
"""Write the Homebrew cask and formula for a published release, for the timecatbit/homebrew-tap repository.

The files and their SHA-256 come from the release itself (GitHub's asset digests), so both always describe exactly
what was published. The cask installs the app (`brew install --cask timecatbit/tap/starmeter`); the formula the
starmeter command on its own, on macOS and Linux (`brew install timecatbit/tap/starmeter-cli`).
"""
import argparse
import json
from pathlib import Path
import sys
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parent))
from package import REPOSITORY, downloads

API = 'https://api.github.com/repos/timecatbit/starmeter/releases/tags/v'


def cask(version, arm, intel):
    """The cask's Ruby for this version, given each disk image's SHA-256."""
    for digest in (arm, intel):
        if len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):raise ValueError('not a SHA-256: ' + digest)
    files = [name for name, _ in downloads(version) if name.endswith('.dmg')]
    pattern = files[0].replace(version, '#{version}').replace('apple-silicon', '#{arch}')
    return f'''cask "starmeter" do
  arch arm: "apple-silicon", intel: "intel"

  version "{version}"
  sha256 arm:   "{arm}",
         intel: "{intel}"

  url "{REPOSITORY}/releases/download/v#{{version}}/{pattern}"
  name "Starmeter"
  desc "Menu bar monitor for AI coding tools' token usage, costs and limits"
  homepage "{REPOSITORY}"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: :ventura

  app "Starmeter.app"

  zap trash: "~/Library/Caches/Token Monitor"
end
'''


def formula(version, mac, linux):
    """The formula's Ruby for this version, given each command-line archive's SHA-256: the engine under the name
    starmeter in libexec with its price tables, linked into bin."""
    for digest in (mac, linux):
        if len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):raise ValueError('not a SHA-256: ' + digest)
    files = dict((label, name) for name, label in downloads(version))
    url = REPOSITORY + '/releases/download/v' + version + '/'
    return f'''class StarmeterCli < Formula
  desc "Command-line view of AI coding tools' token usage, costs and limits"
  homepage "{REPOSITORY}"
  version "{version}"
  license "MIT"

  livecheck do
    url "{REPOSITORY}"
    strategy :github_latest
  end

  on_macos do
    url "{url}{files['Mac · Command line']}"
    sha256 "{mac}"
  end

  on_linux do
    on_intel do
      url "{url}{files['Linux · Command line']}"
      sha256 "{linux}"
    end
  end

  def install
    libexec.install "starmeter", "lib"
    bin.install_symlink libexec/"starmeter"
  end

  test do
    assert_match "starmeter #{{version}}", shell_output("#{{bin}}/starmeter --version")
  end
end
'''


def published(version, opener=urllib.request.urlopen, suffixes=('.dmg',)):
    """SHA-256 from the release's asset digests, for its files ending so (the two disk images by default), in the
    order the release lists them."""
    request = urllib.request.Request(API + version, headers={'Accept': 'application/vnd.github+json', 'User-Agent': 'Starmeter-release'})
    with opener(request, timeout=30) as response:release = json.load(response)
    digests = {asset['name']: str(asset.get('digest') or '') for asset in release.get('assets', [])}
    names = [name for name, _ in downloads(version) if name.endswith(suffixes)]
    found = [digests.get(name, '').removeprefix('sha256:') for name in names]
    if not all(found):raise SystemExit('The release has no SHA-256 for: ' + ', '.join(n for n, d in zip(names, found) if not d))
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('version', help='a published version, such as 6.0.0')
    parser.add_argument('--output', type=Path, help='where to write the cask; printed when left out')
    parser.add_argument('--formula-output', type=Path, help='where to write the starmeter-cli formula (Formula/starmeter-cli.rb)')
    args = parser.parse_args()
    text = cask(args.version, *published(args.version))
    if args.output:args.output.write_text(text, encoding='utf-8')
    else:print(text, end='')
    if args.formula_output:
        # Listed by file name: the Linux archive comes before the Mac one.
        linux, mac = published(args.version, suffixes=('-cli.tar.gz',))
        args.formula_output.write_text(formula(args.version, mac, linux), encoding='utf-8')


if __name__ == '__main__':
    main()

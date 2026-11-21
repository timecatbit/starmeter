#!/usr/bin/env python3
"""The READMEs' star chart: the repository's GitHub stars over time, drawn as the panel's 手绘 look draws (coloured
markers on dotted paper by day, chalk on a blackboard by night, inks from app/ui/src/lib/sketchPalette.ts), into
stars.svg and stars-dark.svg.

The Star chart workflow (.github/workflows/stars.yml) runs it once a day and keeps the two pictures on the stars branch,
where the READMEs show them from: the chart never waits on another site (star-history.com's, shown before, sometimes
failed to load when its own GitHub quota ran out) and main's history stays free of daily commits.

    GITHUB_TOKEN=… python3 scripts/star_chart.py timecatbit/starmeter <folder>
"""
from datetime import datetime, timedelta, timezone
import json
import math
import os
from pathlib import Path
import sys
import urllib.request

INKS = {
    False: {'paper': '#FFFDF7', 'dots': 'rgba(60, 60, 90, 0.13)', 'ink': '#2D2A33', 'soft': 'rgba(45, 42, 51, 0.62)',
            'rule': 'rgba(45, 42, 51, 0.16)', 'line': '#F0565A', 'hatch': '#1098AD', 'figure': '#3B5BDB'},
    True: {'paper': '#243029', 'dots': 'rgba(255, 255, 255, 0.05)', 'ink': '#F1EFE7', 'soft': 'rgba(241, 239, 231, 0.62)',
           'rule': 'rgba(241, 239, 231, 0.16)', 'line': '#FF8787', 'hatch': '#66D9E8', 'figure': '#91A7FF'},
}
WIDTH, HEIGHT = 800, 340
LEFT, RIGHT, TOP, BOTTOM = 70, 40, 96, 290
FACE = "'Chalkboard SE', 'Segoe Print', 'Comic Sans MS', -apple-system, 'Segoe UI', sans-serif"


def starred(repository, token):
    """When each star was given, oldest first (the API's star+json form carries the time)."""
    days, page = [], 1
    while True:
        request = urllib.request.Request(
            'https://api.github.com/repos/%s/stargazers?per_page=100&page=%d' % (repository, page),
            headers={'Accept': 'application/vnd.github.star+json', 'User-Agent': 'starmeter-star-chart',
                     **({'Authorization': 'Bearer ' + token} if token else {})})
        with urllib.request.urlopen(request, timeout=30) as answer:
            stars = json.load(answer)
        days += [datetime.fromisoformat(star['starred_at'].replace('Z', '+00:00')).date() for star in stars]
        if len(stars) < 100:
            return sorted(days)
        page += 1


def ceiling(count):
    """The axis's top and step: a round step, at least five stars high."""
    top = max(count, 5)
    step = 10 ** math.floor(math.log10(top))
    for each in (step / 5, step / 2, step, step * 2):
        if each >= 1 and top / each <= 5:
            step = each
            break
    return int(math.ceil(top / step) * step), int(step)


def chart(days, today, dark):
    """The chart as SVG: a step line of the running count from the first star to today, hatched beneath."""
    inks = INKS[dark]
    first = (days[0] if days else today) - timedelta(days=1)
    span = max((today - first).days, 1)
    top, step = ceiling(len(days))
    x = lambda day: LEFT + (WIDTH - LEFT - RIGHT) * (day - first).days / span
    y = lambda count: BOTTOM - (BOTTOM - TOP) * count / top

    points = [(x(first), y(0))]
    for count, day in enumerate(days, 1):
        points += [(x(day), points[-1][1]), (x(day), y(count))]
    points.append((x(today), y(len(days))))
    line = ' '.join('%.1f,%.1f' % point for point in points)
    area = line + ' %.1f,%.1f %.1f,%.1f' % (x(today), y(0), x(first), y(0))

    rules = []
    for count in range(0, top + 1, step):
        rules.append('<line x1="%d" x2="%d" y1="%.1f" y2="%.1f" stroke="%s" stroke-dasharray="3 5"/>'
                     % (LEFT, WIDTH - RIGHT, y(count), y(count), inks['rule']))
        rules.append('<text x="%d" y="%.1f" text-anchor="end" font-size="14" fill="%s">%d</text>'
                     % (LEFT - 12, y(count) + 5, inks['soft'], count))
    ticks = []
    shown = '%Y-%m' if span > 180 else '%m-%d'
    marked = sorted({first + timedelta(days=round(span * part / 4)) for part in range(5)})
    for day in marked:
        anchor = 'start' if day == first else 'end' if day == today else 'middle'
        ticks.append('<text x="%.1f" y="%d" text-anchor="%s" font-size="14" fill="%s">%s</text>'
                     % (x(day), BOTTOM + 26, anchor, inks['soft'], day.strftime(shown)))
    end = points[-1]
    return '\n'.join([
        '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" viewBox="0 0 %d %d" font-family="%s">'
        % (WIDTH, HEIGHT, WIDTH, HEIGHT, FACE),
        '<defs>',
        '<pattern id="dots" width="14" height="14" patternUnits="userSpaceOnUse"><circle cx="7" cy="7" r="1.1" fill="%s"/></pattern>'
        % inks['dots'],
        '<pattern id="hatch" width="7" height="7" patternUnits="userSpaceOnUse" patternTransform="rotate(-35)">'
        '<line x1="0" y1="0" x2="0" y2="7" stroke="%s" stroke-width="1.4" stroke-opacity="0.75"/></pattern>' % inks['hatch'],
        '<filter id="hand" x="-4%" y="-8%" width="108%" height="116%"><feTurbulence type="fractalNoise" baseFrequency="0.04" '
        'numOctaves="2" seed="7" result="noise"/><feDisplacementMap in="SourceGraphic" in2="noise" scale="2.2" '
        'xChannelSelector="R" yChannelSelector="G"/></filter>',
        '</defs>',
        '<rect x="1" y="1" width="%d" height="%d" rx="18" fill="%s"/>' % (WIDTH - 2, HEIGHT - 2, inks['paper']),
        '<rect x="1" y="1" width="%d" height="%d" rx="18" fill="url(#dots)"/>' % (WIDTH - 2, HEIGHT - 2),
        '<text x="%d" y="58" font-size="34" font-weight="bold" fill="%s">★ %d</text>' % (LEFT - 30, inks['figure'], len(days)),
        '<path d="M%d 70 q 12 -5 24 0 t 24 0 t 24 0" fill="none" stroke="%s" stroke-width="2" stroke-linecap="round"/>'
        % (LEFT - 28, inks['line']),
        '<text x="%d" y="54" text-anchor="end" font-size="15" fill="%s">%s</text>'
        % (WIDTH - RIGHT, inks['soft'], today.isoformat()),
        *rules,
        *ticks,
        '<g filter="url(#hand)">',
        '<polygon points="%s" fill="url(#hatch)"/>' % area,
        '<polyline points="%s" fill="none" stroke="%s" stroke-width="3.2" stroke-linejoin="round" stroke-linecap="round"/>'
        % (line, inks['line']),
        '<circle cx="%.1f" cy="%.1f" r="5" fill="%s"/>' % (end[0], end[1], inks['line']),
        '</g>',
        '</svg>',
        '',
    ])


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    days = starred(sys.argv[1], os.environ.get('GITHUB_TOKEN') or os.environ.get('GH_TOKEN'))
    today = datetime.now(timezone.utc).date()
    folder = Path(sys.argv[2])
    folder.mkdir(parents=True, exist_ok=True)
    (folder / 'stars.svg').write_text(chart(days, today, False), encoding='utf-8')
    (folder / 'stars-dark.svg').write_text(chart(days, today, True), encoding='utf-8')
    print('%d stars, drawn to %s' % (len(days), folder))


if __name__ == '__main__':
    main()

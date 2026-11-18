#!/usr/bin/env python3
"""Write the Starmeter logo: a sparkle in a night sky, drawn in halftone dots like the heat map and the share card's pattern.

Three SVG files in assets/, all from the same sparkle:
  starmeter.svg            the app icon, 15 dots across, on the macOS icon grid (an 824 tile in a 1024 canvas);
  starmeter-small.svg      the same icon with 9 larger dots, for 32 pixels and below, where 15 dots blur;
  starmeter-mark.svg       one-colour 5-dot mark on a 24 grid for the menu bar and menus, a template image;
                           it samples the middle of a 7-dot grid, where the sparkle is, so it fills the box.
The mark's dots fall on whole pixels at 20 points on a Retina screen, its menu bar size (an 8-pixel pitch).
"""
import argparse
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CANVAS, TILE, RADIUS = 1024, 824, 185
SHAPE, GLOW = 0.66, 0.3

# A night sky: deep blue, lighter at the top; the sparkle glows white with pale blue tips.
SKY = ('#1F3A7A', '#0E1D4C', '#050A24')
HALO, TIP = '#C4D8FF', (0x7E, 0xA6, 0xF5)


def defs():
    return ('<defs>'
            '<linearGradient id="bg" x1="0" y1="0" x2="0.35" y2="1">'
            '<stop offset="0" stop-color="%s"/><stop offset="0.55" stop-color="%s"/><stop offset="1" stop-color="%s"/>'
            '</linearGradient>'
            '<radialGradient id="sheen" cx="0.28" cy="0.1" r="0.8">'
            '<stop offset="0" stop-color="#FFFFFF" stop-opacity="0.22"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/>'
            '</radialGradient>'
            '<radialGradient id="glow" cx="0.5" cy="0.5" r="0.5">'
            '<stop offset="0" stop-color="%s" stop-opacity="0.5"/><stop offset="1" stop-color="%s" stop-opacity="0"/>'
            '</radialGradient>'
            '</defs>') % (SKY + (HALO, HALO))


def number(value):
    return ('%.2f' % value).rstrip('0').rstrip('.')


def brightness(x, y):
    """How bright the glowing sparkle is at (x, y), both from -1 to 1: 1 at the centre, 0 past its tips."""
    edge = (abs(x) ** SHAPE + abs(y) ** SHAPE) ** (1 / SHAPE)
    return min(1.0, max(0.0, 1 - edge) ** 0.8 * 1.25 + GLOW * math.exp(-(x * x + y * y) / 0.18))


def blend(light, value):
    """Pale blue at the tips to white at the centre."""
    return '#%02X%02X%02X' % tuple(round(a + (0xFF - a) * min(1.0, value ** light)) for a in TIP)


def dots(count, span, centre, largest, smallest, cutoff, reach=1.0):
    """Halftone: each dot's area follows the brightness under it; (x, y, radius, brightness) for the visible ones.
    reach is how far out the grid samples the sparkle: below 1 leaves out the dark rim, so the dots fill the span."""
    half = (count - 1) / 2
    step = span / (count - 1)
    for row in range(count):
        for column in range(count):
            value = brightness((column - half) / half * reach, (row - half) / half * reach)
            if value >= cutoff:
                yield (centre + (column - half) * step, centre + (row - half) * step,
                       step * max(smallest, largest * math.sqrt(value)), value)


def svg(size, body):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {0} {0}" width="{0}" height="{0}">'.format(size)
            + body + '</svg>\n')


def tile():
    edge, inner = (CANVAS - TILE) / 2, (CANVAS - TILE) / 2 + 1.5
    return ''.join('<rect x="{0}" y="{0}" width="{1}" height="{1}" rx="{2}" fill="{3}"/>'.format(
        number(edge), TILE, RADIUS, fill) for fill in ('url(#bg)', 'url(#sheen)')) + (
        '<rect x="{0}" y="{0}" width="{1}" height="{1}" rx="{2}" fill="none" stroke="#FFFFFF" stroke-opacity="0.13" '
        'stroke-width="3"/>'.format(number(inner), TILE - 3, RADIUS - 1.5))


def icon(count, span, largest, smallest, cutoff, light, background):
    centre = CANVAS / 2
    body = defs() + tile() + '<circle cx="512" cy="512" r="%s" fill="url(#glow)"/>' % number(TILE * 0.36)
    if background:
        # Faint dots fill the rest of the screen, fading toward the tile's edge.
        half, step = (count - 1) / 2, TILE * span / (count - 1)
        for row in range(count):
            for column in range(count):
                x, y = (column - half) / half, (row - half) / half
                opacity = 0.22 * (1 - math.hypot(x, y) / 1.45)
                if brightness(x, y) < cutoff and opacity >= 0.03:
                    # Like a sky full of distant stars, a few of them twinkle a little brighter.
                    twinkle = (row * 7 + column * 13) * 2654435761 % 1000 / 1000
                    bright = twinkle > 0.86
                    body += '<circle cx="%s" cy="%s" r="%s" fill="#FFFFFF" fill-opacity="%s"/>' % (
                        number(centre + (column - half) * step), number(centre + (row - half) * step),
                        number(step * smallest * (1.7 if bright else 1)),
                        number(min(0.6, opacity * (2.6 if bright else 0.55 + 0.6 * twinkle))))
    for x, y, radius, value in dots(count, TILE * span, centre, largest, smallest, cutoff):
        body += '<circle cx="%s" cy="%s" r="%s" fill="%s"/>' % (number(x), number(y), number(radius), blend(light, value))
    return svg(CANVAS, body)


def mark():
    body = ''.join('<circle cx="%s" cy="%s" r="%s" fill="#000000"/>' % (number(x), number(y), number(radius))
                   for x, y, radius, _ in dots(5, 19.2, 12, 0.46, 0.16, 0.08, 2 / 3))
    return svg(24, body)


def logos():
    return {'starmeter.svg': icon(15, 0.82, 0.46, 0.07, 0.06, 0.7, True),
            'starmeter-small.svg': icon(9, 0.7, 0.48, 0.18, 0.12, 0.6, False),
            'starmeter-mark.svg': mark()}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, default=ROOT / 'assets')
    args = parser.parse_args()
    for name, text in logos().items():
        (args.output / name).write_text(text, encoding='utf-8')
        print(args.output / name)


if __name__ == '__main__':
    main()

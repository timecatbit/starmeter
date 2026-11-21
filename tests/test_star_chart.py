"""The READMEs' star chart (scripts/star_chart.py): a running count of stars, drawn whole, with a round axis."""
from datetime import date
import importlib.util
from pathlib import Path
import unittest
import xml.dom.minidom

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('star_chart', ROOT / 'scripts/star_chart.py')
star_chart = importlib.util.module_from_spec(spec)
spec.loader.exec_module(star_chart)


class StarChartTests(unittest.TestCase):
    def test_the_count_climbs_a_step_for_each_star_and_runs_on_to_today(self):
        days = [date(2026, 11, 6), date(2026, 11, 10), date(2026, 11, 10)]
        for dark in (False, True):
            svg = star_chart.chart(days, date(2026, 11, 21), dark)
            line = xml.dom.minidom.parseString(svg).getElementsByTagName('polyline')[0].getAttribute('points').split()
            self.assertIn('★ 3', svg)
            self.assertEqual(len(line), 2 + 2 * len(days))
            self.assertEqual(float(line[-1].split(',')[0]), star_chart.WIDTH - star_chart.RIGHT)
            self.assertEqual(line[-1].split(',')[1], line[-2].split(',')[1])

    def test_no_star_yet_and_one_day_still_draw(self):
        for days, today in (([], date(2026, 11, 21)), ([date(2026, 11, 21)], date(2026, 11, 21))):
            xml.dom.minidom.parseString(star_chart.chart(days, today, False))

    def test_the_axis_rises_in_round_steps(self):
        self.assertEqual(star_chart.ceiling(0), (5, 1))
        self.assertEqual(star_chart.ceiling(6), (6, 2))
        self.assertEqual(star_chart.ceiling(37), (40, 10))
        self.assertEqual(star_chart.ceiling(1234), (1500, 500))


if __name__ == '__main__':
    unittest.main()

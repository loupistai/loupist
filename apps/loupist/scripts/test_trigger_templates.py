"""Execute bundled checks against deterministic read fixtures."""

import runpy
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch


class Unavailable(Exception):
    pass


def evaluate(name, args, assets, *, price=None, call=None):
    module = types.SimpleNamespace(
        args=args,
        state={},
        followed=lambda: assets,
        price=price,
        call=call,
        gather=lambda _: None,
        Unavailable=Unavailable,
        Refused=Unavailable,
    )
    result = {}
    module.result = lambda **fields: result.update(fields)
    script = (
        Path(__file__).resolve().parents[1]
        / "agent"
        / "trigger-templates"
        / f"{name}.py"
    )
    with patch.dict(sys.modules, {"loupist": module}):
        runpy.run_path(str(script))
    return result


class TriggerTemplates(unittest.TestCase):
    def test_volume_spike_needs_twenty_prior_days_by_default(self):
        rows = [
            {"date": f"2026-11-{day:02}", "volume": volume}
            for day, volume in enumerate([15] * 5 + [75], 1)
        ]
        assets = [{"id": "a", "name": "Northwind"}]
        args = {"tool": "daily", "calls": {"a": {"ticker": "N"}}}
        with self.assertRaisesRegex(Unavailable, "no_data"):
            evaluate("volume-spike", args, assets, call=lambda *a, **kw: {"rows": rows})
        result = evaluate(
            "volume-spike",
            {**args, "window": 5},
            assets,
            call=lambda *a, **kw: {"rows": rows},
        )
        self.assertTrue(result["met"])

    def test_price_band_reads_only_the_named_asset(self):
        assets = [{"id": "a", "name": "Northwind"}, {"id": "b", "name": "Southwind"}]
        reads = []

        def prices(asset_id, *, days):
            reads.append(asset_id)
            return {
                "points": [
                    {"date": "2026-11-24", "value": 100},
                    {"date": "2026-11-25", "value": 110},
                ]
            }

        result = evaluate(
            "price-band", {"asset_names": ["northwind"]}, assets, price=prices
        )
        self.assertEqual(reads, ["a"])
        self.assertEqual(result["reading"], "Northwind +10.0%")
        with self.assertRaises(Unavailable):
            evaluate("price-band", {"asset_names": ["Missing"]}, assets, price=prices)
        self.assertEqual(reads, ["a"])

    def test_volume_spike_uses_the_median_of_the_preceding_twenty_days(self):
        rows = [
            {"date": f"2026-11-{day:02}", "volume": volume}
            for day, volume in enumerate([5] + [10] * 10 + [20] * 10 + [75], 1)
        ]
        assets = [{"id": "a", "name": "Northwind"}]
        args = {"tool": "daily", "calls": {"a": {"ticker": "N"}}}
        result = evaluate(
            "volume-spike", args, assets, call=lambda *a, **kw: {"rows": rows}
        )
        self.assertTrue(result["met"])
        (spike,) = result["evidence"]["spiked"]
        self.assertEqual(
            (spike["volume"], spike["median"], spike["ratio"]), (75, 15, 5.0)
        )
        self.assertEqual(result["reading"], "Northwind volume 5.0x its median")

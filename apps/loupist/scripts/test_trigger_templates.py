"""Execute bundled checks against deterministic read fixtures."""

import builtins
import io
import json
import random
import runpy
import sys
import unittest
from datetime import date, timedelta
from pathlib import Path
from unittest.mock import patch


APP = Path(__file__).resolve().parents[1]
BOOTSTRAP = APP / "src-tauri/src/python_run/loupist_bootstrap.py"
CALENDAR = json.loads((APP / "market-calendar.json").read_text())["markets"]


def sessions(market, start, end):
    closed = CALENDAR.get(market, {}).get("closed", {})
    return [
        day.isoformat()
        for offset in range((end - start).days + 1)
        if (day := start + timedelta(days=offset)).weekday() < 5
        and day.isoformat() not in closed
    ]


def evaluation_dates(now):
    day = date.fromisoformat(now)
    dates = {}
    for market in ("", "US", "KRX"):
        last, previous, prior = reversed(
            sessions(market, day - timedelta(days=30), day)[-3:]
        )
        dates[market] = (
            last,
            previous if day.isoformat() == last else last,
            prior if day.isoformat() == last else previous,
        )
    return dates


class Unavailable(Exception):
    pass


def evaluate(
    name, args, assets, *, price=None, call=None, now="2026-11-25", state=None
):
    real_open = builtins.open

    def open_scratch(path, *args, **kwargs):
        if path == "/scratch/.loupist-ledger.json":
            return io.StringIO(
                json.dumps(
                    {
                        "id": "test",
                        "args": args,
                        "entries": [],
                        "now": now,
                        "inputs": {},
                        "bar_dates": evaluation_dates(now),
                        "check_stale_days": 5,
                    }
                )
            )
        if path == "/scratch/.loupist-code.py":
            return io.StringIO("pass")
        return real_open(path, *args, **kwargs)

    result = {}
    script = APP / "agent" / "trigger-templates" / f"{name}.py"
    with patch.dict(sys.modules):
        with patch("builtins.open", open_scratch):
            runpy.run_path(str(BOOTSTRAP))
        module = sys.modules["loupist"]
        module.args = args
        module.state = {} if state is None else state
        module.followed = lambda: assets
        module.price = price
        module.call = call
        module.gather = lambda _: None
        module.result = lambda **fields: result.update(fields)
        try:
            runpy.run_path(str(script))
        except module.Unavailable as error:
            raise Unavailable(error.code) from error
        result["state"] = json.loads(json.dumps(module.state))
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
            now="2026-11-06",
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
            for day, volume in enumerate([5] + [10] * 10 + [20] * 10 + [75], 2)
        ]
        assets = [{"id": "a", "name": "Northwind"}]
        args = {"tool": "daily", "calls": {"a": {"ticker": "N"}}}
        result = evaluate(
            "volume-spike",
            args,
            assets,
            call=lambda *a, **kw: {"rows": rows},
            now="2026-11-23",
        )
        self.assertTrue(result["met"])
        (spike,) = result["evidence"]["spiked"]
        self.assertEqual(
            (spike["volume"], spike["median"], spike["ratio"]), (75, 15, 5.0)
        )
        self.assertEqual(result["reading"], "Northwind volume 5.0x its median")

    def test_mixed_dates_in_each_market_template(self):
        assets = [{"id": key, "name": key.upper()} for key in ("a", "b", "c")]
        histories = {
            "a": [("2026-01-01", 100), ("2026-01-02", 110)],
            "b": [("2026-11-24", 100), ("2026-11-25", 101)],
            "c": [("2026-11-23", 100), ("2026-11-24", 110)],
        }

        def price(asset, **kw):
            return {
                "points": [
                    {"date": day, "value": value} for day, value in histories[asset]
                ]
            }

        for template, args, key in [
            ("price-band", {"band_percent": 5}, "moved"),
            ("level-cross", {"levels": {key: 105 for key in histories}}, "crossed"),
        ]:
            with self.subTest(template=template):
                quiet = evaluate(template, args, assets[:2], price=price)
                self.assertFalse(quiet["met"])
                self.assertEqual(
                    quiet["evidence"]["unread"],
                    [
                        {"name": "A", "why": "stale", "date": "2026-01-02"},
                    ],
                )
                met = evaluate(template, args, assets, price=price)
                self.assertEqual(met["as_of"], "2026-11-24")
                self.assertEqual([row["id"] for row in met["evidence"][key]], ["c"])
                self.assertEqual(met["evidence"][key][0]["from"]["date"], "2026-11-23")
        args = {
            "tool": "daily",
            "window": 1,
            "calls": {key: {"symbol": key} for key in histories},
        }

        def volume(_, *, symbol):
            return {
                "rows": [
                    {"date": day, "volume": value} for day, value in histories[symbol]
                ]
            }

        args["multiple"] = 1.05
        met = evaluate("volume-spike", args, assets, call=volume)
        self.assertEqual(met["as_of"], "2026-11-24")
        self.assertEqual([row["id"] for row in met["evidence"]["spiked"]], ["c"])
        self.assertEqual(met["evidence"]["unread"][0]["name"], "A")

    def test_a_market_can_move_forward_even_when_another_market_regresses(self):
        assets = [{"id": key, "name": key} for key in ("a", "b")]
        pairs = {"a": ("2026-11-24", "2026-11-25"), "b": ("2026-11-23", "2026-11-24")}
        state = {"a": "2026-11-25"}  # The earlier format kept just the last met date.

        def price(asset, **_):
            before, after = pairs[asset]
            return {
                "points": [
                    {"date": before, "value": 100},
                    {"date": after, "value": 110},
                ]
            }

        result = evaluate("price-band", {}, assets, price=price, state=state)
        self.assertEqual([row["id"] for row in result["evidence"]["moved"]], ["b"])
        pairs["a"] = ("2026-11-23", "2026-11-24")
        result = evaluate("price-band", {}, assets, price=price, state=state)
        self.assertFalse(result["met"])
        self.assertEqual(result["evidence"]["unread"][0]["name"], "a")

    def test_item_dates_are_checked_before_a_new_id_can_fire(self):
        baseline = {"seen": ["baseline"], "since": "2026-11-25"}
        rows = [
            {"id": "old", "date": "20260102"},
            {"id": "undated"},
            {"id": "future", "date": "2026-11-26"},
            {"id": "baseline", "date": "2026-11-25"},
            {"id": "new", "date": "20261124", "title": "New filing"},
        ]
        result = evaluate(
            "new-item",
            {"tool": "filings"},
            [],
            state=baseline,
            call=lambda *_: {"rows": rows},
        )
        self.assertEqual(result["as_of"], "2026-11-24")
        self.assertEqual(result["evidence"]["new"], [rows[-1]])
        self.assertEqual(result["evidence"]["unavailable"], 3)

    def test_price_band_matches_an_independent_oracle_for_seeded_histories(self):
        rng = random.Random(441791)
        for now in (
            "2026-10-09",
            "2026-10-12",
            "2026-11-26",
            "2026-11-27",
            "2026-11-29",
        ):
            today = date.fromisoformat(now)
            for trial in range(50):
                histories, expected, unread, usable = {}, {}, set(), []
                assets = []
                for index in range(8):
                    key = str(index)
                    market = rng.choice(["KRX", "US"])
                    assets.append(
                        {"id": key, "name": key, "reference": {"exchange": market}}
                    )
                    calendar = sessions(market, today - timedelta(days=40), today)
                    intervals = [
                        day
                        for day in calendar
                        if day >= (today - timedelta(days=15)).isoformat()
                    ]
                    indices = sorted(
                        rng.sample(
                            range(len(intervals)), rng.randint(2, len(intervals))
                        )
                    )
                    prices = [rng.choice([60, 88, 97, 100, 103, 120]) for _ in indices]
                    history = list(zip([intervals[i] for i in indices], prices))
                    histories[key] = history
                    (before, old), (after, new) = history[-2:]
                    next_dates = {day: i for i, day in enumerate(calendar)}
                    eligible = calendar[-2:] if now in calendar else calendar[-1:]
                    if (
                        after not in eligible
                        or next_dates[after] - next_dates[before] != 1
                    ):
                        unread.add(key)
                        continue
                    usable.append(after)
                    # Compare integer deltas, independently of the template percentage.
                    if 20 * abs(new - old) > old:
                        expected[key] = (before, after)

                def price(asset, **kwargs):
                    return {
                        "points": [
                            {"date": day, "value": value}
                            for day, value in histories[asset]
                        ]
                    }

                with self.subTest(now=now, trial=trial):
                    if not usable:
                        with self.assertRaisesRegex(Unavailable, "stale"):
                            evaluate("price-band", {}, assets, price=price, now=now)
                        continue
                    result = evaluate("price-band", {}, assets, price=price, now=now)
                    actual = {
                        row["id"]: (row["from"]["date"], row["to"]["date"])
                        for row in result["evidence"]["moved"]
                    }
                    self.assertEqual(actual, expected)
                    self.assertEqual(result["met"], bool(expected))
                    self.assertEqual(
                        result["as_of"],
                        max([end for _, end in expected.values()] or usable),
                    )
                    self.assertEqual(
                        {row["name"] for row in result["evidence"]["unread"]}, unread
                    )

    def test_world_change_records_a_start_then_reports_each_new_fact_once(self):
        assets = [
            {"id": "ast_a", "name": "Northwind"},
            {"id": "ast_b", "name": "Other"},
        ]
        pages = {
            "1970-01-01": {
                "changes": [
                    self._fact("old", "ended", "Old guidance was lowered", "2026-11-01")
                ],
                "lens": [],
                "pending": 0,
                "failed": 0,
                "capped": False,
                "next": "wch1.start",
            },
            "wch1.start": {
                "changes": [
                    self._fact("guide", "ended", "Guidance was lowered", "2026-11-20")
                ],
                "lens": [],
                "pending": 0,
                "failed": 0,
                "capped": False,
                "next": "wch1.after-guide",
            },
            "wch1.after-guide": {
                "changes": [
                    self._fact("guide", "ended", "Guidance was lowered", "2026-11-20")
                ],
                "lens": [],
                "pending": 0,
                "failed": 0,
                "capped": False,
                "next": "wch1.after-guide",
            },
        }
        calls = []

        def feed(tool, **arguments):
            calls.append((tool, arguments.get("since"), arguments.get("about")))
            if tool == "followed":
                return assets
            self.assertEqual(tool, "world_changes")
            return pages[arguments["since"]]

        first = evaluate(
            "world-change",
            {"asset_ids": ["ast_a"]},
            assets,
            call=feed,
            now="2026-11-20",
        )
        self.assertFalse(first["met"])
        self.assertEqual(first["state"]["cursor"], "wch1.start")
        self.assertEqual(first["evidence"]["rows"], [])
        second = evaluate(
            "world-change",
            {"asset_ids": ["ast_a"]},
            assets,
            call=feed,
            now="2026-11-21",
            state=first["state"],
        )
        self.assertTrue(second["met"])
        self.assertEqual(second["as_of"], "2026-11-20")
        self.assertEqual(
            second["evidence"]["rows"][0]["statement"], "Guidance was lowered"
        )
        self.assertEqual(second["evidence"]["rows"][0]["source"], "Filing")
        self.assertEqual(second["state"]["cursor"], "wch1.after-guide")
        third = evaluate(
            "world-change",
            {"asset_ids": ["ast_a"]},
            assets,
            call=feed,
            now="2026-11-22",
            state=second["state"],
        )
        self.assertFalse(third["met"])
        self.assertEqual(third["evidence"]["rows"], [])
        self.assertIn(("world_changes", "wch1.start", ["ast_a"]), calls)

    def test_world_change_sequences_skip_nothing_and_wait_out_pending_jobs(self):
        assets = [{"id": "ast_a", "name": "Northwind"}]
        # Publications arrive between evaluations. A row that appears while jobs
        # are pending is not consumed; the next quiet evaluation reports it once.
        steps = [
            ("1970-01-01", [], 0, False, "wch1.base"),
            (
                "wch1.base",
                [self._fact("one", "ended", "Guidance was lowered", "2026-11-10")],
                1,
                False,
                "wch1.base",
            ),
            (
                "wch1.base",
                [self._fact("one", "ended", "Guidance was lowered", "2026-11-10")],
                0,
                True,
                "wch1.one",
            ),
            (
                "wch1.one",
                [self._fact("one", "ended", "Guidance was lowered", "2026-11-10")],
                0,
                False,
                "wch1.one",
            ),
        ]
        seen = []
        state = None
        for since, changes, pending, met, nxt in steps:
            pages = {
                since: {
                    "changes": changes,
                    "lens": [],
                    "pending": pending,
                    "failed": 0,
                    "capped": False,
                    "next": nxt,
                }
            }

            def feed(tool, expected=since, pages=pages, **arguments):
                if tool == "followed":
                    return assets
                self.assertEqual(arguments["since"], expected)
                return pages[expected]

            result = evaluate(
                "world-change", {"asset_ids": ["ast_a"]}, assets, call=feed, state=state
            )
            self.assertEqual(result["met"], met, since)
            seen.extend(row["statement"] for row in result["evidence"]["rows"])
            if pending:
                self.assertEqual(result["evidence"]["pending"], pending)
                self.assertEqual(
                    result["state"].get("cursor"),
                    state.get("cursor") if state else None,
                )
            state = result["state"]
        self.assertEqual(seen, ["Guidance was lowered"])

    def test_world_change_reads_one_page_per_evaluation(self):
        assets = [{"id": "ast_a", "name": "Northwind"}]
        pages = {
            "1970-01-01": {
                "changes": [
                    self._fact(f"c{index}", "ended", f"Fact {index}", "2026-11-01")
                    for index in range(30)
                ],
                "lens": [],
                "pending": 0,
                "failed": 0,
                "capped": True,
                "next": "wch1.page2",
            },
            "wch1.page2": {
                "changes": [self._fact("c30", "ended", "Fact 30", "2026-11-01")],
                "lens": [],
                "pending": 0,
                "failed": 0,
                "capped": False,
                "next": "wch1.done",
            },
        }
        calls = []

        def feed(tool, **arguments):
            if tool == "followed":
                return assets
            calls.append(arguments["since"])
            return pages[arguments["since"]]

        first = evaluate("world-change", {"asset_ids": ["ast_a"]}, assets, call=feed)
        self.assertFalse(first["met"])
        self.assertEqual(calls, ["1970-01-01"])
        self.assertEqual(first["state"]["cursor"], "wch1.page2")
        calls.clear()
        second = evaluate(
            "world-change",
            {"asset_ids": ["ast_a"]},
            assets,
            call=feed,
            state=first["state"],
        )
        self.assertEqual(calls, ["wch1.page2"])
        self.assertEqual(second["state"]["cursor"], "wch1.done")
        self.assertEqual(second["evidence"]["rows"], [])

    def test_world_change_baseline_survives_pending_and_a_failed_source_does_not_freeze(
        self,
    ):
        assets = [{"id": "ast_a", "name": "Northwind"}]
        history = self._fact("old", "ended", "Old guidance was lowered", "2026-11-01")
        appeared = self._fact("new", "ended", "Guidance was lowered", "2026-11-20")
        # The first page is pending, so the old row is the baseline, not an alert.
        # A later failed source does not stop the new row.
        steps = [
            ("1970-01-01", [history], 1, 0, False, "wch1.base"),
            ("wch1.base", [history], 0, 0, False, "wch1.base"),
            ("wch1.base", [appeared], 0, 1, True, "wch1.after"),
            ("wch1.after", [appeared], 0, 1, False, "wch1.after"),
        ]
        state = None
        reported = []
        for since, changes, pending, failed, met, nxt in steps:
            page = {
                "changes": changes,
                "lens": [],
                "pending": pending,
                "failed": failed,
                "capped": False,
                "next": nxt,
            }

            def feed(tool, page=page, expected=since, **arguments):
                if tool == "followed":
                    return assets
                self.assertEqual(arguments["since"], expected)
                return page

            result = evaluate(
                "world-change", {"asset_ids": ["ast_a"]}, assets, call=feed, state=state
            )
            self.assertEqual(result["met"], met, since)
            reported.extend(row["statement"] for row in result["evidence"]["rows"])
            if failed:
                self.assertEqual(result["evidence"]["failed"], failed)
            state = result["state"]
        self.assertEqual(reported, ["Guidance was lowered"])

    def test_world_change_keeps_a_later_lens_supersession_inside_the_state_limit(self):
        assets = [{"id": "ast_a", "name": "Northwind"}]
        wide = "rec_" + ("a" * 2400)

        def lens(at):
            return {
                "recordId": wide,
                "title": "Notes",
                "sectionKey": "steps",
                "state": "superseded",
                "recordedAtMs": at,
                "claimIds": [],
            }

        pages = {
            "1970-01-01": {
                "changes": [],
                "lens": [],
                "pending": 0,
                "failed": 0,
                "capped": False,
                "next": "wch1.base",
            },
        }
        for index, at in enumerate((1, 2, 3, 4), start=1):
            pages[f"wch1.{index - 1}" if index > 1 else "wch1.base"] = {
                "changes": [],
                "lens": [lens(at)],
                "pending": 0,
                "failed": 0,
                "capped": False,
                "next": f"wch1.{index}",
            }
        pages["1970-01-01"] = {
            "changes": [],
            "lens": [],
            "pending": 0,
            "failed": 0,
            "capped": False,
            "next": "wch1.base",
        }

        def feed(tool, **arguments):
            if tool == "followed":
                return assets
            return pages[arguments["since"]]

        state = None
        results = []
        for _ in range(5):
            result = evaluate(
                "world-change",
                {"asset_ids": ["ast_a"]},
                assets,
                call=feed,
                state=state,
            )
            self.assertLessEqual(len(json.dumps(result["state"])), 8192)
            results.append(result)
            state = result["state"]
        self.assertFalse(results[0]["met"])
        self.assertTrue(results[1]["met"])
        self.assertTrue(results[-1]["met"])

    def test_world_change_contains_keeps_guidance_and_drops_management(self):
        assets = [{"id": "ast_a", "name": "Northwind"}]
        page = {
            "changes": [
                self._fact("guide", "ended", "Guidance was lowered", "2026-11-20"),
                self._fact("boss", "ended", "Management was replaced", "2026-11-20"),
            ],
            "lens": [],
            "pending": 0,
            "failed": 0,
            "capped": False,
            "next": "wch1.next",
        }

        def feed(tool, **arguments):
            if tool == "followed":
                return assets
            return (
                page
                if arguments["since"] == "wch1.start"
                else {
                    "changes": [],
                    "lens": [],
                    "pending": 0,
                    "failed": 0,
                    "capped": False,
                    "next": "wch1.start",
                }
            )

        first = evaluate(
            "world-change",
            {"asset_ids": ["ast_a"], "contains": ["guidance"]},
            assets,
            call=feed,
        )
        result = evaluate(
            "world-change",
            {"asset_ids": ["ast_a"], "contains": ["guidance"]},
            assets,
            call=feed,
            state=first["state"],
        )
        self.assertTrue(result["met"])
        self.assertEqual(
            [row["statement"] for row in result["evidence"]["rows"]],
            ["Guidance was lowered"],
        )

    @staticmethod
    def _fact(claim, kind, statement, day):
        return {
            "kind": kind,
            "claimId": claim,
            "episodeId": f"eps-{claim}",
            "proposition": statement,
            "source": {"title": "Filing", "publicationTime": {"value": day}},
            "ending": {"at": {"value": day}},
        }

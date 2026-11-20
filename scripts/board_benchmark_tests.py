#!/usr/bin/env python3
"""Statistical and input-contract regressions for frozen bankroll evaluation."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

try:
    import board_benchmark as benchmark
except ModuleNotFoundError:
    benchmark = None


def row(session=0, profit=0, hands=100, requested=100):
    """Exact unit records; estimator fixtures need not be full engine histories."""
    initial = 10_000_000
    final = initial + profit
    buyin = 2_000_000
    can_continue = final >= buyin
    return dict(session=session, seed=2026112007, hand_offset=session * requested,
                start_button=session % 6, hands_requested=requested, hands_played=hands,
                initial_units=initial, final_units=final, profit_units=profit,
                buyin_units=buyin, stop_loss_units=initial, max_wager_units=buyin,
                max_drawdown_units=max(0, -profit), rake_table_units=0,
                hero_allin_actions=0, hero_max_single_action_units=0,
                hero_max_hand_exposure_units=0, reached_horizon=hands == requested,
                can_continue=can_continue,
                stop_reason="hands_limit" if can_continue else "insufficient_buyin")


def stdout(rows, summary_changes=None):
    def line(prefix, values):
        return prefix + " " + " ".join(
            f"{key}={str(value).lower() if isinstance(value, bool) else value}"
            for key, value in values.items())
    summary = dict(sessions=len(rows), hands_played=sum(r["hands_played"] for r in rows),
                   profit_units=sum(r["profit_units"] for r in rows))
    summary.update(summary_changes or {})
    return "\n".join([*(line("SIXMAX_RISK_SESSION", r) for r in rows),
                       line("SIXMAX_RISK_SUMMARY", summary)]) + "\n"


def protocol():
    return {"protocol_version": 1, "experiment": "board-aware-risk-v1", "sessions": 64,
            "hands_per_session": 2000, "master_seed": 2026112007, "equity_samples": 96,
            "bankroll_usd": 50, "stop_loss_usd": 50, "max_bet_usd": 10,
            "bootstrap": {"resamples": 5000, "seed": 2026112011},
            "models": {name: {"kind": "phh" if name == "phh" else "tabular", "sha256": "0" * 64}
                       for name in ("ten", "twenty", "phh")},
            "exclude_streams": [{"seed": 2026111901, "start": 0, "count": 5000000,
                                 "reason": "known training"}],
            "conditions": [{"id": "100bb_rake5", "stack_bb": 100, "rake_bps": 500, "rake_cap_bb": 3},
                           {"id": "50bb_rake5", "stack_bb": 50, "rake_bps": 500, "rake_cap_bb": 3},
                           {"id": "100bb_norake", "stack_bb": 100, "rake_bps": 0, "rake_cap_bb": 0}],
            "opponents": ["equity_multiway", "tight_aggressive", "loose_aggressive"],
            "phh_condition": "100bb_rake5", "provenance_note": "known manifest streams only"}


class BenchmarkTests(unittest.TestCase):
    def call(self, name, *args, **kwargs):
        self.assertIsNotNone(benchmark, "board benchmark implementation is absent")
        self.assertTrue(hasattr(benchmark, name), f"missing benchmark behavior: {name}")
        return getattr(benchmark, name)(*args, **kwargs)

    def test_pooled_rate_uses_sum_of_played_hands(self):
        # $10 / 100 hands and -$1 / 10 hands pool to $9 / 110 hands.
        result = self.call("summarize", [row(profit=2_000_000),
                                        row(1, profit=-200_000, hands=10)], resamples=200)
        self.assertAlmostEqual(result["usd100"], 8.181818181818182)
        self.assertAlmostEqual(result["bb100"], 81.81818181818181)
        self.assertEqual(result["profit_units"], 1_800_000)
        self.assertEqual(result["hands_played"], 110)

    def test_zero_hand_sessions_remain_in_endpoint_denominators(self):
        result = self.call("summarize", [row(profit=2_000_000), row(1, hands=0)], resamples=200)
        self.assertEqual(result["sessions"], 2)
        self.assertEqual(result["zero_hand_sessions"], 1)
        self.assertEqual(result["mean_profit_usd"], 5.0)
        self.assertEqual(result["scheduled_usd100"], 5.0)
        self.assertEqual(result["usd100"], 10.0)
        self.assertIsNone(result["ci95"]["usd100"])
        self.assertGreater(result["bootstrap_undefined"]["usd100"], 0)

    def test_all_zero_hand_rates_are_null_in_strict_json(self):
        result = self.call("summarize", [row(hands=0), row(1, hands=0)], resamples=20)
        self.assertIsNone(result["usd100"])
        self.assertIsNone(result["bb100"])
        self.assertEqual(result["mean_profit_usd"], 0.0)
        self.assertIsNone(result["ci95"]["bb100"])
        json.dumps(result, allow_nan=False)

    def test_stopped_sessions_and_last_hand_depletion_are_counted(self):
        stopped = row(1, profit=-9_000_000, hands=100)
        result = self.call("summarize", [row(), stopped], resamples=200)
        self.assertEqual(result["reached_horizon"], 2)
        self.assertEqual(result["unable_to_continue"], 1)
        self.assertEqual(result["stop_counts"]["insufficient_buyin"], 1)
        low, high = result["stop_fraction_wilson95"]
        self.assertAlmostEqual(low, 0.09453120573423074)
        self.assertAlmostEqual(high, 0.9054687942657693)

    def test_paired_identical_records_have_zero_difference_and_interval(self):
        rows = [row(profit=400_000), row(1, profit=-200_000)]
        result = self.call("paired_comparison", rows, list(reversed(rows)), resamples=200)
        self.assertEqual(result["delta_usd100"], 0.0)
        self.assertEqual(result["delta_mean_profit_usd"], 0.0)
        self.assertEqual(result["ci95"]["delta_usd100"], [0.0, 0.0])

    def test_paired_rates_recompute_each_candidates_own_denominator(self):
        ten = [row(profit=2_000_000, hands=100), row(1, profit=-200_000, hands=10)]
        twenty = [row(profit=800_000, hands=20), row(1, profit=-400_000, hands=100)]
        result = self.call("paired_comparison", ten, twenty, resamples=2000)
        self.assertAlmostEqual(result["delta_usd100"], -6.515151515151516)
        self.assertAlmostEqual(result["delta_mean_profit_usd"], -3.5)
        self.assertAlmostEqual(result["ci95"]["delta_usd100"][0], -6.515151515151516)
        self.assertAlmostEqual(result["ci95"]["delta_usd100"][1], 10.0)

    def test_pairing_rejects_missing_or_different_seed_records(self):
        rows = [row(), row(1)]
        with self.assertRaises(ValueError):
            self.call("paired_comparison", rows, rows[:1], resamples=20)
        changed = copy.deepcopy(rows)
        changed[1]["seed"] += 1
        with self.assertRaises(ValueError):
            self.call("paired_comparison", rows, changed, resamples=20)

    def test_bootstrap_is_reproducible_with_reported_seed(self):
        rows = [row(profit=400_000), row(1, profit=-9_000_000, hands=10), row(2, profit=1)]
        first = self.call("summarize", rows, resamples=200, bootstrap_seed=11)
        second = self.call("summarize", rows, resamples=200, bootstrap_seed=11)
        self.assertEqual(first, second)
        self.assertEqual(first["bootstrap_seed"], 11)
        self.assertEqual(first["bootstrap_resamples"], 200)

    def parse(self, text, sessions=2, hands=100, stack_bb=100):
        return self.call("parse_sessions", text, sessions=sessions, hands=hands,
                         seed=2026112007, stack_bb=stack_bb,
                         bankroll_usd=50, stop_loss_usd=50, max_bet_usd=10,
                         rake_bps=500, rake_cap_bb=3)

    def test_exact_rows_reconcile_and_preserve_integer_money(self):
        rows = [row(profit=1), row(1, profit=-9_000_001, hands=10)]
        self.assertEqual(self.parse(stdout(rows)), rows)
        for changes in ({"sessions": 3}, {"hands_played": 999}, {"profit_units": 2}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                self.parse(stdout(rows, changes))

    def test_missing_duplicate_and_malformed_rows_are_rejected(self):
        rows = [row(), row(1)]
        variants = [stdout(rows[:1]), stdout([rows[0], rows[0]]),
                    stdout(rows).replace("hands_played=100", "hands_played=100.0", 1),
                    stdout(rows).replace("can_continue=true", "can_continue=yes", 1),
                    stdout(rows).replace("profit_units=0", "profit_units=0 profit_units=9", 1)]
        for text in variants:
            with self.subTest(text=text[:60]), self.assertRaises(ValueError):
                self.parse(text)

    def test_risk_fields_and_bankroll_floor_are_checked_independently(self):
        mutations = {"final_units": -1, "buyin_units": 1_000_000,
                     "hero_max_hand_exposure_units": 2_000_001,
                     "hero_max_single_action_units": 2_000_001,
                     "can_continue": False, "stop_reason": "insufficient_buyin",
                     "reached_horizon": False, "rake_table_units": 6_000_001,
                     "hand_offset": 999, "hands_requested": 101}
        for key, value in mutations.items():
            changed = [row(), row(1)]
            changed[1][key] = value
            with self.subTest(field=key), self.assertRaises(ValueError):
                self.parse(stdout(changed))

    def test_no_hand_cannot_record_a_loss_or_action(self):
        empty = row(profit=-9_000_000, hands=0)
        with self.assertRaises(ValueError):
            self.parse(stdout([empty, row(1)]))

    def test_smaller_stack_keeps_ten_dollar_cap_but_reserves_five(self):
        rows = [row(profit=-8_800_000), row(1)]
        for record in rows:
            record["buyin_units"] = 1_000_000
            record["can_continue"] = True
            record["stop_reason"] = "hands_limit"
        self.assertEqual(self.parse(stdout(rows), stack_bb=50), rows)

    def test_write_json_rejects_nonfinite_and_existing_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "report.json"
            self.call("write_json", target, {"rate": None})
            self.assertEqual(json.loads(target.read_text()), {"rate": None})
            with self.assertRaises(FileExistsError):
                self.call("write_json", target, {"rate": 1})
            with self.assertRaises(ValueError):
                self.call("write_json", Path(temporary) / "bad.json", {"rate": float("nan")})

    def test_model_identity_requires_the_expected_checkpoint_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "checkpoint.bin"
            path.write_bytes(b"frozen bytes")
            with self.assertRaises(ValueError):
                self.call("verify_models", {"ten": path},
                          {"ten": {"kind": "tabular", "sha256": "0" * 64}})

    def test_case_commands_keep_full_and_smoke_streams_separate(self):
        paths = {name: Path(f"/{name}.bin") for name in ("ten", "twenty", "phh")}
        full = self.call("build_cases", protocol(), Path("/solverpk"), paths, smoke=False)
        smoke = self.call("build_cases", protocol(), Path("/solverpk"), paths, smoke=True)
        self.assertEqual(len(full), 20)
        self.assertEqual(len(smoke), 20)
        for cases, seed, excluded, sessions, hands in (
                (full, "2026112007", "2026112099:0:24", "64", "2000"),
                (smoke, "2026112099", "2026112007:0:128000", "2", "12")):
            for case in cases:
                command = case["command"]
                self.assertEqual(command[command.index("--seed") + 1], seed)
                self.assertEqual(command[command.index("--sessions") + 1], sessions)
                self.assertEqual(command[command.index("--hands") + 1], hands)
                self.assertEqual(command[command.index("--equity-samples") + 1], "96")
                self.assertIn(excluded, command)
                self.assertIn("2026111901:0:5000000", command)
                self.assertEqual(command[command.index("--max-bet-usd") + 1], "10")
                if case["condition"] == "100bb_norake":
                    self.assertNotIn("--rake-cap-bb", command, "Rust rejects an explicit zero cap")
                if case["opponent"] == "phh":
                    self.assertEqual(case["condition"], "100bb_rake5")
                    self.assertIn("--phh-model", command)

    def test_smoke_omits_only_its_reserved_manifest_exclusion(self):
        spec = protocol()
        spec["exclude_streams"].append({"seed": 2026112099, "start": 0, "count": 24,
                                        "reason": "reserved smoke stream"})
        paths = {name: Path(f"/{name}.bin") for name in ("ten", "twenty", "phh")}
        cases = self.call("build_cases", spec, Path("/solverpk"), paths, smoke=True)
        for case in cases:
            self.assertNotIn("2026112099:0:24", case["command"])
            self.assertIn("2026111901:0:5000000", case["command"])
            self.assertIn("2026112007:0:128000", case["command"])

    def test_protocol_rejects_missing_hash_and_changed_published_budget(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "protocol.json"
            for mutate in (lambda p: p["models"]["ten"].pop("sha256"),
                           lambda p: p.update(sessions=2),
                           lambda p: p["conditions"][0].update(stack_bb=200),
                           lambda p: p.update(opponents=["phh"])):
                value = protocol()
                mutate(value)
                path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    self.call("load_protocol", path)

    def test_failing_process_keeps_stdout_and_stderr_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            stem = Path(temporary) / "case"
            command = [sys.executable, "-c", "import sys;print('partial');print('failure',file=sys.stderr);sys.exit(7)"]
            with self.assertRaises(subprocess.CalledProcessError):
                self.call("run_case", command, stem, {})
            self.assertEqual(stem.with_suffix(".stdout.log").read_text(), "partial\n")
            self.assertEqual(stem.with_suffix(".stderr.log").read_text(), "failure\n")

    def test_hash_failure_leaves_no_claimed_experiment_or_model_changes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = {}
            for name in ("ten", "twenty", "phh"):
                paths[name] = root / f"{name}.bin"
                paths[name].write_bytes(name.encode())
            specification = root / "protocol.json"
            specification.write_text(json.dumps(protocol()))
            output = root / "results"
            with self.assertRaises(ValueError):
                self.call("run_experiment", Path(sys.executable), paths, specification, output, smoke=True)
            self.assertFalse(output.exists())
            self.assertEqual(paths["ten"].read_bytes(), b"ten")

    def test_runner_freezes_manifest_before_processes_and_retains_all_cases(self):
        # A real subprocess exercises the parser/orchestrator boundary. It is a
        # protocol fixture, not a simulated poker engine or accuracy assertion.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "results"
            paths, spec = {}, protocol()
            for name in ("ten", "twenty", "phh"):
                paths[name] = root / f"{name}.bin"
                paths[name].write_bytes(name.encode())
                spec["models"][name]["sha256"] = hashlib.sha256(name.encode()).hexdigest()
            specification = root / "protocol.json"
            specification.write_text(json.dumps(spec))
            fixture = root / "engine_fixture"
            fixture.write_text(f"#!{sys.executable}\n" + r'''
import json, pathlib, sys
assert pathlib.Path(__file__).with_name('results').joinpath('manifest.json').exists()
args = sys.argv[1:]
def get(flag): return args[args.index(flag) + 1]
n, h, seed, stack = (int(get(f)) for f in ('--sessions','--hands','--seed','--stack-bb'))
for i in range(n):
    row = dict(session=i, seed=seed, hand_offset=i*h, start_button=i%6,
        hands_requested=h, hands_played=h, initial_units=10000000, final_units=10000000,
        profit_units=0, buyin_units=stack*20000, stop_loss_units=10000000,
        max_wager_units=2000000, max_drawdown_units=0, rake_table_units=0,
        hero_allin_actions=0, hero_max_single_action_units=0, hero_max_hand_exposure_units=0,
        reached_horizon='true', can_continue='true', stop_reason='hands_limit')
    print('SIXMAX_RISK_SESSION ' + ' '.join(f'{k}={v}' for k,v in row.items()))
print(f'SIXMAX_RISK_SUMMARY sessions={n} hands_played={n*h} profit_units=0')
''')
            fixture.chmod(0o755)
            report = self.call("run_experiment", fixture, paths, specification, output, smoke=True)
            self.assertEqual(report["mode"], "smoke")
            self.assertEqual(len(report["cases"]), 20)
            self.assertEqual(len(report["paired_comparisons"]), 10)
            self.assertEqual(sum(len(case["sessions"]) for case in report["cases"]), 40)
            self.assertEqual(report["verification"]["models_before"], report["verification"]["models_after"])
            self.assertEqual(json.loads((output / "manifest.json").read_text())["effective_seed"], 2026112099)
            self.assertTrue((output / "results.json").is_file())
            with self.assertRaises(FileExistsError):
                self.call("run_experiment", fixture, paths, specification, output, smoke=True)


if __name__ == "__main__":
    unittest.main()

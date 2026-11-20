#!/usr/bin/env python3
"""Frozen, manifest-bound bankroll benchmark. Python standard library only.

All requested sessions are retained. Rates divide total net by total actual
hands; whole-session paired bootstrap preserves the outcome-dependent lengths.
The result estimates this stopping protocol, not unlimited-bankroll policy EV.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import random
import re
import subprocess
import sys
import time


BB_UNITS = 20_000
USD_UNITS = 200_000
BOOTSTRAP_SEED = 2026112011
REPO = Path(__file__).resolve().parents[1]
DEFAULT_PROTOCOL = REPO / "audit/2026-11-20/board-aware/protocol.json"
SMOKE_SEED = 2026112099
INTEGER_FIELDS = (
    "session", "seed", "hand_offset", "hands_requested", "hands_played",
    "initial_units", "final_units", "profit_units", "buyin_units",
    "stop_loss_units", "max_wager_units", "max_drawdown_units", "rake_table_units",
    "hero_allin_actions", "hero_max_single_action_units", "hero_max_hand_exposure_units",
)
BOOLEAN_FIELDS = ("reached_horizon", "can_continue")
PAIR_FIELDS = ("session", "seed", "hand_offset", "hands_requested", "initial_units",
               "buyin_units", "stop_loss_units", "max_wager_units")


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    # Encode first: NaN/Infinity cannot leave a partially written valid artifact.
    encoded = json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n"
    with Path(path).open("x", encoding="utf-8") as handle:
        handle.write(encoded)


def verify_models(paths, specifications):
    verified = {}
    for name, path in paths.items():
        expected = specifications[name].get("sha256", "")
        if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
            raise ValueError(f"missing or invalid expected SHA-256 for {name}")
        actual = sha256(path)
        if actual != expected:
            raise ValueError(f"unexpected {name} checkpoint SHA-256: {actual}; expected {expected}")
        verified[name] = {"path": str(Path(path).resolve()), "sha256": actual,
                          "kind": specifications[name]["kind"]}
    return verified


def percentile(values, probability):
    ordered = sorted(values)
    if not ordered:
        return None
    position = (len(ordered) - 1) * probability
    left = int(position)
    right = min(left + 1, len(ordered) - 1)
    return ordered[left] + (ordered[right] - ordered[left]) * (position - left)


def wilson95(successes, total):
    if not 0 <= successes <= total or total <= 0:
        raise ValueError("invalid binomial counts")
    z = 1.959963984540054
    fraction = successes / total
    denominator = 1 + z * z / total
    center = (fraction + z * z / (2 * total)) / denominator
    half = z * math.sqrt(fraction * (1 - fraction) / total + z * z / (4 * total * total)) / denominator
    return [max(0.0, center - half), min(1.0, center + half)]


def point_estimates(rows):
    if not rows:
        raise ValueError("at least one scheduled session is required")
    profit = sum(row["profit_units"] for row in rows)
    hands = sum(row["hands_played"] for row in rows)
    requested = sum(row["hands_requested"] for row in rows)
    return {"bb100": profit * 100 / (BB_UNITS * hands) if hands else None,
            "usd100": profit * 100 / (USD_UNITS * hands) if hands else None,
            "scheduled_usd100": profit * 100 / (USD_UNITS * requested) if requested else None,
            "mean_profit_usd": profit / (USD_UNITS * len(rows)),
            "stop_fraction": sum(not row["can_continue"] for row in rows) / len(rows)}


def bootstrap(rows, other=None, *, resamples=5000, bootstrap_seed=BOOTSTRAP_SEED):
    if resamples < 1 or len(rows) < 1:
        raise ValueError("bootstrap needs positive resamples and nonempty sessions")
    keys = tuple(point_estimates(rows))
    sampled_values = {key: [] for key in keys}
    undefined = dict.fromkeys(keys, 0)
    rng = random.Random(bootstrap_seed)
    for _ in range(resamples):
        indices = [rng.randrange(len(rows)) for _ in rows]
        values = point_estimates([rows[index] for index in indices])
        if other is not None:
            right = point_estimates([other[index] for index in indices])
            values = {key: right[key] - values[key]
                      if right[key] is not None and values[key] is not None else None
                      for key in keys}
        for key, value in values.items():
            if value is None:
                undefined[key] += 1
            else:
                sampled_values[key].append(value)
    # Do not silently discard all-zero-denominator bootstrap replicates.
    intervals = {key: [percentile(values, 0.025), percentile(values, 0.975)]
                 if len(rows) >= 2 and not undefined[key] else None
                 for key, values in sampled_values.items()}
    return intervals, undefined


def summarize(rows, *, resamples=5000, bootstrap_seed=BOOTSTRAP_SEED):
    point = point_estimates(rows)
    ci, undefined = bootstrap(rows, resamples=resamples, bootstrap_seed=bootstrap_seed)
    stops = sum(not row["can_continue"] for row in rows)
    quantiles = (0, 0.1, 0.25, 0.5, 0.75, 0.9, 1)
    return {**point, "sessions": len(rows),
            "hands_requested": sum(row["hands_requested"] for row in rows),
            "hands_played": sum(row["hands_played"] for row in rows),
            "profit_units": sum(row["profit_units"] for row in rows),
            "profit_usd": sum(row["profit_units"] for row in rows) / USD_UNITS,
            "zero_hand_sessions": sum(row["hands_played"] == 0 for row in rows),
            "reached_horizon": sum(row["reached_horizon"] for row in rows),
            "unable_to_continue": stops,
            "stop_counts": dict(sorted(Counter(row["stop_reason"] for row in rows).items())),
            "stop_fraction_wilson95": wilson95(stops, len(rows)),
            "final_balance_usd_quantiles": {
                str(q): percentile([row["final_units"] / USD_UNITS for row in rows], q)
                for q in quantiles},
            "max_drawdown_usd_quantiles": {
                str(q): percentile([row["max_drawdown_units"] / USD_UNITS for row in rows], q)
                for q in quantiles},
            "hero_allin_actions": sum(row["hero_allin_actions"] for row in rows),
            "hero_max_single_action_usd": max(row["hero_max_single_action_units"] for row in rows) / USD_UNITS,
            "hero_max_hand_exposure_usd": max(row["hero_max_hand_exposure_units"] for row in rows) / USD_UNITS,
            "rake_table_usd": sum(row["rake_table_units"] for row in rows) / USD_UNITS,
            "ci95": ci, "bootstrap_undefined": undefined,
            "bootstrap_resamples": resamples, "bootstrap_seed": bootstrap_seed,
            "ci_method": "whole_session_percentile_bootstrap_approximate_unadjusted",
            "rake_scope": "whole_table_six_players_not_hero"}


def paired_comparison(ten, twenty, *, resamples=5000, bootstrap_seed=BOOTSTRAP_SEED):
    ten = sorted(ten, key=lambda row: row["session"])
    twenty = sorted(twenty, key=lambda row: row["session"])
    if not ten or len(ten) != len(twenty):
        raise ValueError("paired candidates must contain the same nonempty sessions")
    if len({row["session"] for row in ten}) != len(ten):
        raise ValueError("duplicate paired session identifiers")
    for left, right in zip(ten, twenty):
        if any(left[key] != right[key] for key in PAIR_FIELDS):
            raise ValueError("paired session identity or financial conditions differ")
    left, right = point_estimates(ten), point_estimates(twenty)
    point = {f"delta_{key}": right[key] - left[key]
             if right[key] is not None and left[key] is not None else None for key in left}
    ci, undefined = bootstrap(ten, twenty, resamples=resamples, bootstrap_seed=bootstrap_seed)
    return {**point, "comparison": "twenty_minus_ten", "paired_sessions": len(ten),
            "ci95": {f"delta_{key}": value for key, value in ci.items()},
            "bootstrap_undefined": {f"delta_{key}": value for key, value in undefined.items()},
            "bootstrap_resamples": resamples, "bootstrap_seed": bootstrap_seed,
            "ci_method": "paired_whole_session_percentile_bootstrap_approximate_unadjusted"}


def fields(line, prefix):
    result = {}
    for token in line[len(prefix):].split():
        if "=" not in token:
            raise ValueError(f"malformed {prefix} field")
        name, value = token.split("=", 1)
        if name in result:
            raise ValueError(f"duplicate {prefix} field {name}")
        result[name] = value
    return result


def integer(value, name):
    if not isinstance(value, str) or not re.fullmatch(r"[+-]?[0-9]+", value):
        raise ValueError(f"{name} must be an exact integer")
    return int(value)


def parse_sessions(output, *, sessions, hands, seed, stack_bb, bankroll_usd,
                   stop_loss_usd, max_bet_usd, rake_bps, rake_cap_bb):
    rows, summaries = [], []
    for line in output.splitlines():
        if line.startswith("SIXMAX_RISK_SESSION "):
            raw = fields(line, "SIXMAX_RISK_SESSION")
            missing = set((*INTEGER_FIELDS, *BOOLEAN_FIELDS, "stop_reason")) - raw.keys()
            if missing:
                raise ValueError(f"session missing fields: {sorted(missing)}")
            row = {key: integer(raw[key], key) for key in INTEGER_FIELDS}
            for key in BOOLEAN_FIELDS:
                if raw[key] not in ("true", "false"):
                    raise ValueError(f"{key} must be true or false")
                row[key] = raw[key] == "true"
            row["stop_reason"] = raw["stop_reason"]
            if "start_button" in raw:
                row["start_button"] = integer(raw["start_button"], "start_button")
            rows.append(row)
        elif line.startswith("SIXMAX_RISK_SUMMARY "):
            summaries.append(fields(line, "SIXMAX_RISK_SUMMARY"))
    if len(rows) != sessions or len(summaries) != 1:
        raise ValueError("incomplete session output or missing/duplicate summary")
    rows.sort(key=lambda row: row["session"])
    for index, row in enumerate(rows):
        expected = {"session": index, "seed": seed, "hand_offset": index * hands,
                    "hands_requested": hands, "initial_units": bankroll_usd * USD_UNITS,
                    "buyin_units": stack_bb * BB_UNITS,
                    "stop_loss_units": stop_loss_usd * USD_UNITS,
                    "max_wager_units": max_bet_usd * USD_UNITS}
        if any(row[key] != value for key, value in expected.items()):
            raise ValueError(f"session {index} does not match its requested identity/configuration")
        if row.get("start_button", index % 6) != index % 6:
            raise ValueError("unexpected starting button")
        played, buyin = row["hands_played"], row["buyin_units"]
        floor = row["initial_units"] - row["stop_loss_units"]
        final = row["final_units"]
        if not 0 <= played <= hands or final - row["initial_units"] != row["profit_units"]:
            raise ValueError("invalid hand count or net/final balance reconciliation")
        if final < floor or final < 0 or not -buyin * played <= row["profit_units"] <= 5 * buyin * played:
            raise ValueError("bankroll floor or maximum possible profit violated")
        if not 0 <= row["hero_max_single_action_units"] <= row["hero_max_hand_exposure_units"] <= buyin <= row["max_wager_units"]:
            raise ValueError("invalid monetary exposure")
        if not 0 <= row["hero_allin_actions"] <= played or row["max_drawdown_units"] < max(0, -row["profit_units"]):
            raise ValueError("invalid all-in or drawdown telemetry")
        rake_limit = rake_cap_bb * BB_UNITS * played if rake_bps else 0
        if not 0 <= row["rake_table_units"] <= rake_limit:
            raise ValueError("table rake exceeds its declared cap")
        if final <= floor:
            reason = "stop_loss"
        elif final < buyin:
            reason = "insufficient_buyin"
        elif final - buyin < floor:
            reason = "stop_loss_budget"
        else:
            reason = "hands_limit"
        can_continue = reason == "hands_limit"
        if row["can_continue"] != can_continue or row["stop_reason"] != reason:
            raise ValueError("terminal funding state is inconsistent with bankroll")
        if row["reached_horizon"] != (played == hands) or (can_continue and played != hands):
            raise ValueError("terminal horizon state is inconsistent with hand count")
    summary = summaries[0]
    for key, expected in (("sessions", sessions),
                          ("hands_played", sum(row["hands_played"] for row in rows)),
                          ("profit_units", sum(row["profit_units"] for row in rows))):
        if key not in summary or integer(summary[key], key) != expected:
            raise ValueError(f"summary {key} does not reconcile with exact session rows")
    return rows


def load_protocol(path):
    protocol = json.loads(Path(path).read_text(encoding="utf-8"))
    fixed = {"protocol_version": 1, "experiment": "board-aware-risk-v1",
             "sessions": 64, "hands_per_session": 2000, "master_seed": 2026112007,
             "equity_samples": 96, "bankroll_usd": 50, "stop_loss_usd": 50, "max_bet_usd": 10}
    if not isinstance(protocol, dict) or any(protocol.get(key) != value for key, value in fixed.items()):
        raise ValueError("protocol does not match the predeclared full experiment")
    if protocol.get("bootstrap") != {"resamples": 5000, "seed": BOOTSTRAP_SEED}:
        raise ValueError("unexpected bootstrap protocol")
    if protocol.get("no_flop_no_drop", True) is not True:
        raise ValueError("benchmark requires no-flop-no-drop")
    if protocol.get("opponents") != ["equity_multiway", "tight_aggressive", "loose_aggressive"]:
        raise ValueError("unexpected primary opponent set")
    conditions = protocol.get("conditions", [])
    expected = {"100bb_rake5": (100, 500, 3), "50bb_rake5": (50, 500, 3),
                "100bb_norake": (100, 0, 0)}
    if len(conditions) != len(expected) or {condition.get("id") for condition in conditions} != set(expected):
        raise ValueError("missing or duplicate stack/rake condition")
    for condition in conditions:
        if tuple(condition.get(key) for key in ("stack_bb", "rake_bps", "rake_cap_bb")) != expected[condition["id"]]:
            raise ValueError("stack/rake condition differs from declared experiment")
    if protocol.get("phh_condition") != "100bb_rake5":
        raise ValueError("PHH baseline must use the principal 100 BB/raked condition")
    models = protocol.get("models", {})
    for name, kind in (("ten", "tabular"), ("twenty", "tabular"), ("phh", "phh")):
        specification = models.get(name, {})
        digest = specification.get("sha256", "")
        if specification.get("kind") != kind or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError(f"missing pinned model identity for {name}")
    exclusions = protocol.get("exclude_streams")
    if not isinstance(exclusions, list) or not exclusions:
        raise ValueError("known training/evaluation stream exclusions are required")
    for stream in exclusions:
        for key in ("seed", "start", "count"):
            if type(stream.get(key)) is not int or not 0 <= stream[key] < 2 ** 64:
                raise ValueError("invalid excluded stream interval")
        if stream["count"] == 0 or stream["start"] + stream["count"] >= 2 ** 64:
            raise ValueError("empty or overflowing excluded stream interval")
    return protocol


def effective_streams(protocol, smoke):
    # Guard the *planned* full stream in smoke and the separate smoke stream in
    # full. No observed smoke outcome can consume a later full-test deal.
    other = {"seed": protocol["master_seed"] if smoke else SMOKE_SEED,
             "start": 0, "count": protocol["sessions"] * protocol["hands_per_session"] if smoke else 24,
             "reason": "separate_full_stream" if smoke else "separate_smoke_stream"}
    streams = [*protocol["exclude_streams"], other]
    unique = {}
    for stream in streams:
        if smoke and (stream["seed"], stream["start"], stream["count"]) == (SMOKE_SEED, 0, 24):
            continue
        unique[(stream["seed"], stream["start"], stream["count"])] = stream
    return list(unique.values())


def build_cases(protocol, binary, paths, *, smoke=False):
    cases = []
    seed = SMOKE_SEED if smoke else protocol["master_seed"]
    sessions = 2 if smoke else protocol["sessions"]
    hands = 12 if smoke else protocol["hands_per_session"]
    streams = effective_streams(protocol, smoke)
    for condition in protocol["conditions"]:
        opponents = list(protocol["opponents"])
        if condition["id"] == protocol["phh_condition"]:
            opponents.append("phh")
        for opponent in opponents:
            for candidate in ("ten", "twenty"):
                command = [str(Path(binary).resolve()), "--sixmax-risk-benchmark",
                           "--candidate-kind", protocol["models"][candidate]["kind"],
                           "--checkpoint", str(Path(paths[candidate]).resolve()),
                           "--opponent", opponent, "--sessions", str(sessions),
                           "--hands", str(hands), "--seed", str(seed),
                           "--stack-bb", str(condition["stack_bb"]),
                           "--equity-samples", str(protocol["equity_samples"]),
                           "--bankroll-usd", str(protocol["bankroll_usd"]),
                           "--stop-loss-usd", str(protocol["stop_loss_usd"]),
                           "--max-bet-usd", str(protocol["max_bet_usd"]),
                           "--rake-bps", str(condition["rake_bps"])]
                if condition["rake_bps"]:
                    command += ["--rake-cap-bb", str(condition["rake_cap_bb"])]
                if opponent == "phh":
                    command += ["--phh-model", str(Path(paths["phh"]).resolve())]
                for stream in streams:
                    command += ["--exclude-stream", f"{stream['seed']}:{stream['start']}:{stream['count']}"]
                expected = {"sessions": sessions, "hands": hands, "seed": seed,
                            "stack_bb": condition["stack_bb"], "rake_bps": condition["rake_bps"],
                            "rake_cap_bb": condition["rake_cap_bb"],
                            **{key: protocol[key] for key in ("bankroll_usd", "stop_loss_usd", "max_bet_usd")}}
                cases.append({"id": f"{candidate}-{opponent}-{condition['id']}",
                              "candidate": candidate, "opponent": opponent,
                              "condition": condition["id"], "configuration": condition,
                              "command": command, "expected": expected})
    return cases


def run_case(command, log_stem, expected):
    started = time.monotonic()
    process = subprocess.run(command, capture_output=True, text=True, check=False)
    for suffix, value in ((".stdout.log", process.stdout), (".stderr.log", process.stderr)):
        with Path(log_stem).with_suffix(suffix).open("x", encoding="utf-8") as handle:
            handle.write(value)
    if process.returncode:
        raise subprocess.CalledProcessError(process.returncode, command, process.stdout, process.stderr)
    return {"sessions": parse_sessions(process.stdout, **expected),
            "elapsed_seconds": time.monotonic() - started,
            "stdout_sha256": hashlib.sha256(process.stdout.encode()).hexdigest()}


def source_identity():
    def git(*args):
        result = subprocess.run(["git", *args], cwd=REPO, capture_output=True, text=True, check=True)
        return result.stdout.strip()
    return {"commit": git("rev-parse", "HEAD"),
            "tracked_worktree_dirty": bool(git("status", "--porcelain", "--untracked-files=no")),
            "runner_sha256": sha256(__file__),
            "tests_sha256": sha256(Path(__file__).with_name("board_benchmark_tests.py"))}


def run_experiment(binary, paths, protocol_path, output, *, smoke=False):
    output = Path(output).resolve()
    if output.exists():
        raise FileExistsError(f"output directory already exists: {output}")
    protocol_path, binary = Path(protocol_path).resolve(), Path(binary).resolve()
    protocol = load_protocol(protocol_path)
    if set(paths) != {"ten", "twenty", "phh"}:
        raise ValueError("exactly ten, twenty, and phh model paths are required")
    before = verify_models(paths, protocol["models"])
    binary_before, protocol_before = sha256(binary), sha256(protocol_path)
    cases = build_cases(protocol, binary, paths, smoke=smoke)
    manifest = {"protocol": protocol, "protocol_sha256": protocol_before,
                "protocol_path": str(protocol_path), "source": source_identity(),
                "binary_path": str(binary), "binary_sha256": binary_before,
                "models": before, "mode": "smoke" if smoke else "full",
                "effective_sessions": 2 if smoke else protocol["sessions"],
                "effective_hands_per_session": 12 if smoke else protocol["hands_per_session"],
                "effective_seed": SMOKE_SEED if smoke else protocol["master_seed"],
                "effective_exclude_streams": effective_streams(protocol, smoke),
                "cases": cases, "created_utc": datetime.now(timezone.utc).isoformat(),
                "split": "known_manifest_streams_disjoint",
                "scope": "known recorded streams only; historical unrecorded exposure is unverified",
                "shared_deals_across_candidates_and_conditions": True,
                "independent_sampling_unit": "session_global_index_block",
                "bb_units": BB_UNITS, "usd_units": USD_UNITS, "bb_usd": 0.10,
                "model_selection": "none", "hyperparameter_search": False}
    output.mkdir(parents=True)
    (output / "logs").mkdir()
    (output / "cases").mkdir()
    write_json(output / "manifest.json", manifest)
    completed, failure = [], None
    try:
        for index, case in enumerate(cases):
            print(f"BOARD_BENCHMARK_CASE mode={manifest['mode']} case={index + 1}/{len(cases)} id={case['id']}", flush=True)
            result = {**case, **run_case(case["command"], output / "logs" / case["id"], case["expected"])}
            result["summary"] = summarize(result["sessions"],
                                           resamples=protocol["bootstrap"]["resamples"],
                                           bootstrap_seed=protocol["bootstrap"]["seed"])
            write_json(output / "cases" / f"{case['id']}.json", result)
            completed.append(result)
            print(f"BOARD_BENCHMARK_RESULT id={case['id']} hands={result['summary']['hands_played']} "
                  f"profit_usd={result['summary']['profit_usd']:.6f} bb100={result['summary']['bb100']}", flush=True)
    except BaseException as error:
        failure = error
    try:
        after = {name: {**record, "sha256": sha256(record["path"])} for name, record in before.items()}
        binary_after, protocol_after = sha256(binary), sha256(protocol_path)
        verification = {"models_before": before, "models_after": after,
                        "binary_sha256_before": binary_before, "binary_sha256_after": binary_after,
                        "protocol_sha256_before": protocol_before, "protocol_sha256_after": protocol_after,
                        "unchanged": before == after and binary_before == binary_after and protocol_before == protocol_after}
        if not verification["unchanged"] and failure is None:
            failure = ValueError("frozen model, binary, or protocol changed during evaluation")
    except BaseException as error:
        verification = {"models_before": before, "unchanged": False, "verification_error": str(error)}
        failure = failure or error
    write_json(output / "verification.json", verification)
    if failure is not None:
        write_json(output / "failure.json", {"status": "failed", "error": str(failure),
                                            "completed_cases": [case["id"] for case in completed],
                                            "expected_cases": len(cases), "verification": verification})
        raise failure
    comparisons = []
    indexed = {(case["candidate"], case["opponent"], case["condition"]): case for case in completed}
    for case in completed:
        if case["candidate"] == "ten":
            other = indexed[("twenty", case["opponent"], case["condition"])]
            comparisons.append({"opponent": case["opponent"], "condition": case["condition"],
                                **paired_comparison(case["sessions"], other["sessions"],
                                                    resamples=protocol["bootstrap"]["resamples"],
                                                    bootstrap_seed=protocol["bootstrap"]["seed"])})
    report = {"status": "complete", "mode": manifest["mode"], "manifest": "manifest.json",
              "manifest_sha256": sha256(output / "manifest.json"), "protocol_sha256": protocol_before,
              "cases": completed, "paired_comparisons": comparisons, "verification": verification,
              "total_played_hands": sum(case["summary"]["hands_played"] for case in completed),
              "total_requested_hands": sum(case["summary"]["hands_requested"] for case in completed),
              "independent_sessions_per_condition": manifest["effective_sessions"],
              "rows_sharing_session_blocks_are_paired_not_independent": True,
              "ci_scope": "per_condition_unadjusted_approximate_no_automatic_model_promotion",
              "interpretation": "net return under stated bankroll stopping rule, not unlimited-bankroll EV or human-game returns",
              "rake_scope": "table rake includes all six seats; it is not subtracted again from net profit",
              "model_selection": "none"}
    write_json(output / "results.json", report)
    print(f"BOARD_BENCHMARK_COMPLETE mode={manifest['mode']} cases={len(completed)} "
          f"hands={report['total_played_hands']} frozen_unchanged=true results={output / 'results.json'}", flush=True)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--ten", type=Path, required=True)
    parser.add_argument("--twenty", type=Path, required=True)
    parser.add_argument("--phh", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new directory for immutable evidence")
    parser.add_argument("--protocol", type=Path, default=DEFAULT_PROTOCOL)
    parser.add_argument("--smoke", action="store_true", help="separate seed; 2 sessions x 12 hands; never pooled with full")
    args = parser.parse_args()
    try:
        run_experiment(args.binary, {"ten": args.ten, "twenty": args.twenty, "phh": args.phh},
                       args.protocol, args.output, smoke=args.smoke)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"board benchmark failed: {error}\n")


if __name__ == "__main__":
    main()

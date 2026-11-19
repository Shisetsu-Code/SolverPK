#!/usr/bin/env python3
"""Bounded, predeclared neural-v2 pilot; no automatic checkpoint selection.

Rust performs all poker, inference and learning. Python stdlib only orchestrates
commands and records immutable evidence. Every evaluation is frozen.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import subprocess

from audit_evaluate import EXPECTED_MODELS, fields, sha256

TRAIN_SEEDS = (73001, 73002, 73003)
EVAL_SEEDS = (990071, 990073)
RAKE = ["--rake-bps", "500", "--rake-cap-bb", "3"]


def run(command, log_path):
    process = subprocess.run([str(x) for x in command], capture_output=True, text=True)
    log_path.write_text(process.stdout + process.stderr, encoding="utf-8")
    print(process.stdout, end="", flush=True)
    if process.returncode:
        print(process.stderr, end="", flush=True)
        raise RuntimeError(f"command failed with status {process.returncode}: {command}")
    return process.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/solverpk"))
    parser.add_argument("--models", type=Path, default=Path("audit-models"))
    parser.add_argument("--output", type=Path, default=Path("neural-v2-pilot"))
    parser.add_argument("--train-hands", type=int, default=100000)
    parser.add_argument("--eval-hands", type=int, default=12000)
    parser.add_argument("--learning-rate", type=float, default=0.0005)
    args = parser.parse_args()
    if args.train_hands <= 0 or args.eval_hands < 12 or args.eval_hands % 6:
        parser.error("positive training hands and at least two six-hand evaluation clusters required")
    if not math.isfinite(args.learning_rate) or not 0 < args.learning_rate <= 0.05:
        parser.error("learning rate must be finite and in (0,0.05]")
    binary = args.binary.resolve()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "models").mkdir()
    (out / "logs").mkdir()
    historical = {name: (args.models / spec[0]).resolve() for name, spec in EXPECTED_MODELS.items()}
    for name, path in historical.items():
        if sha256(path) != EXPECTED_MODELS[name][1]:
            raise ValueError(f"unexpected historical {name} checkpoint")

    # Compatibility is asserted against the previous audited executable's
    # published results, not recomputed by the code being tested.
    reference = json.loads(Path("audit/2026-11-19/evaluation.json").read_text())
    compatibility = []
    for opponent in ("tabular", "phh"):
        command = [binary, "--sixmax-neural-eval", "--checkpoint", historical["neural"],
                   "--tabular", historical["tabular"], "--hands", "120000", "--seed", "944104", *RAKE]
        if opponent == "phh":
            command += ["--phh-model", historical["phh"]]
        result = fields(run(command, out / "logs" / f"legacy-{opponent}.log"), "SIXMAX_NEURAL_EVAL")
        prior = next(row for row in reference["results"] if row["seed"] == 944104
                     and row["opponent"] == opponent and row["rake_profile"] == "five_percent_cap_three_bb")
        for field, prior_key in [("bb100", "net_bb100"), ("ci95_cluster_half", "approx_ci95_half_bb100"),
                                 ("table_rake_bb100", "table_rake_bb100")]:
            if abs(float(result[field]) - prior[prior_key]) > 0.000001:
                raise ValueError(f"legacy inference changed: {opponent} {field}")
        compatibility.append({"opponent": opponent, "hands": 120000, "matches_published_result": True,
                              "net_bb100": float(result["bb100"]), "ci95_half": float(result["ci95_cluster_half"])})

    models = []
    training = []
    for seed in TRAIN_SEEDS:
        for phase, hands, rate in [("initial", 1, 0.0), ("trained", args.train_hands, args.learning_rate)]:
            path = out / "models" / f"neural-v2-{seed}-{phase}.bin"
            command = [binary, "--sixmax-neural-train", "--checkpoint", path,
                       "--hands", str(hands), "--seed", str(seed), "--learning-rate", str(rate), *RAKE]
            stdout = run(command, out / "logs" / f"train-{seed}-{phase}.log")
            result = fields(stdout, "SIXMAX_NEURAL_TRAIN")
            if path.read_bytes()[:8] != b"SPKNN002":
                raise ValueError("pilot did not create a v2 checkpoint")
            if int(result["updates"]) != (0 if phase == "initial" else hands):
                raise ValueError("expected exactly one update per trained hand")
            models.append((seed, phase, path))
            training.append({"seed": seed, "phase": phase, "hands_simulated": hands,
                             "learning_rate": rate, "policy_updates": int(result["updates"]),
                             "decisions_trained": int(result["steps"]),
                             "hands_per_second": float(result["hands_per_sec"]), "sha256": sha256(path)})

    frozen_paths = {**historical, **{f"{seed}-{phase}": path for seed, phase, path in models}}
    before = {name: sha256(path) for name, path in frozen_paths.items()}
    rows = []
    for training_seed, phase, path in models:
        for evaluation_seed in EVAL_SEEDS:
            command = [binary, "--sixmax-neural-league", "--checkpoint", path,
                       "--hands", str(args.eval_hands), "--seed", str(evaluation_seed), *RAKE,
                       "--rival-checkpoint", historical["neural"], "--tabular", historical["tabular"],
                       "--phh-model", historical["phh"]]
            stdout = run(command, out / "logs" / f"league-{training_seed}-{phase}-{evaluation_seed}.log")
            result_lines = [line for line in stdout.splitlines() if line.startswith("SIXMAX_LEAGUE_ROW ")]
            if len(result_lines) != 8:
                raise ValueError(f"expected eight rivals, got {len(result_lines)}")
            for line in result_lines:
                result = dict(part.split("=", 1) for part in line.split()[1:])
                row = {"training_seed": training_seed, "phase": phase, "evaluation_seed": evaluation_seed,
                       "opponent": result["opponent"], "policy": result["policy"],
                       "hands": int(result["hands"]), "clusters": int(result["clusters"]),
                       "net_bb100": float(result["bb100"]),
                       "approx_ci95_half_bb100": float(result["ci95_cluster_half"]),
                       "table_rake_bb100": float(result["table_rake_bb100"]), "split": result["split"]}
                if row["hands"] != args.eval_hands or row["clusters"] != args.eval_hands // 6:
                    raise ValueError("league sample count mismatch")
                if not 0 <= row["table_rake_bb100"] <= 300 or row["approx_ci95_half_bb100"] < 0:
                    raise ValueError("invalid league metric")
                rows.append(row)
    after = {name: sha256(path) for name, path in frozen_paths.items()}
    if before != after:
        raise ValueError("frozen evaluation modified checkpoint bytes")
    descriptive = []
    for opponent in sorted({r["opponent"] for r in rows}):
        means = {phase: statistics.mean(r["net_bb100"] for r in rows
                 if r["opponent"] == opponent and r["phase"] == phase) for phase in ("initial", "trained")}
        descriptive.append({"opponent": opponent, "initial_mean_bb100": means["initial"],
                            "trained_mean_bb100": means["trained"],
                            "difference_of_means_bb100": means["trained"] - means["initial"]})
    report = {"source_commit": os.environ.get("GITHUB_SHA", "local"), "binary_sha256": sha256(binary),
              "engine": "accounting-v2", "model_format": "SPKNN002", "architecture": [128, 32, 6],
              "parameter_count": 4326, "reward_scale_bb": 100, "rake_bps": 500, "rake_cap_bb": 3,
              "no_flop_no_drop": True, "training_seeds": TRAIN_SEEDS, "evaluation_seeds": EVAL_SEEDS,
              "train_hands_per_seed": args.train_hands, "eval_hands_per_rival": args.eval_hands,
              "training_hands_with_updates": len(TRAIN_SEEDS) * args.train_hands,
              "league_hands": sum(r["hands"] for r in rows), "legacy_compatibility": compatibility,
              "training": training, "results": rows, "descriptive_comparison": descriptive,
              "checkpoint_sha256_before_evaluation": before, "checkpoint_sha256_after_evaluation": after,
              "model_selection": "none", "hyperparameter_search": False,
              "limits": ["Pilot with three initializations, not proof of competitive poker strength",
                         "Reference heuristics are unvalidated; historical rivals have incomplete data provenance",
                         "Intervals are approximate, per-rival and unadjusted for multiple comparisons",
                         "Difference of means is descriptive; no paired significance test or champion selection",
                         "Per-deal random streams are disjoint where recorded; historical exposures cannot be certified"]}
    (out / "results.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(f"NEURAL_V2_PILOT complete=true trained_hands={report['training_hands_with_updates']} "
          f"league_hands={report['league_hands']} frozen_checkpoints_unchanged=true model_selection=none", flush=True)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Reevaluate immutable historical policies on explicit sixmax rake scenarios.

Python standard library only. This runs inference and validates provenance;
it never trains or overwrites a model. All seeds are fixed before the run.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


EXPECTED_MODELS = {
    "neural": ("checkpoints/neural-100m.bin", "a712525d815872286eb960f330b5f585e4218fef627f7276b2e2e1adb08a8ac4"),
    "tabular": ("scratch.bin", "899482efff86724f106abfbc478392a0128f4416b0a731ba390eaf7eaefc6a08"),
    "phh": ("pluribus-imitation.bin", "dd032a79192bf48a98e42b536b77588c5f59ec731da1fafa75d495a727b0d327"),
}
SEEDS = (944104, 2026111901, 2026111902)
PROFILES = {"no_rake": ["--rake-bps", "0"],
            "five_percent_cap_three_bb": ["--rake-bps", "500", "--rake-cap-bb", "3"]}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fields(output, prefix):
    rows = [line for line in output.splitlines() if line.startswith(prefix + " ")]
    if len(rows) != 1:
        raise ValueError(f"expected one {prefix} result; got {len(rows)}")
    return dict(field.split("=", 1) for field in rows[0].split()[1:])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/solverpk"))
    parser.add_argument("--models", type=Path, default=Path("audit-models"))
    parser.add_argument("--output", type=Path, default=Path("audit-evaluation.json"))
    parser.add_argument("--hands", type=int, default=120000)
    args = parser.parse_args()
    if args.hands < 12 or args.hands % 6:
        parser.error("hands must be a multiple of six and at least 12")
    binary = args.binary.resolve()
    paths = {name: (args.models / spec[0]).resolve() for name, spec in EXPECTED_MODELS.items()}
    before = {name: sha256(path) for name, path in paths.items()}
    for name, (_, expected) in EXPECTED_MODELS.items():
        if before[name] != expected:
            raise ValueError(f"unexpected {name} checkpoint SHA256: {before[name]}")
    results = []
    for seed in SEEDS:
        for opponent in ("tabular", "phh"):
            for profile, rake_args in PROFILES.items():
                command = [str(binary), "--sixmax-neural-eval", "--hands", str(args.hands),
                           "--seed", str(seed), "--checkpoint", str(paths["neural"]),
                           "--tabular", str(paths["tabular"]), *rake_args]
                if opponent == "phh":
                    command += ["--phh-model", str(paths["phh"])]
                process = subprocess.run(command, capture_output=True, text=True, check=True)
                print(f"AUDIT_CASE seed={seed} opponent={opponent} profile={profile}", flush=True)
                print(process.stdout, end="", flush=True)
                rules = fields(process.stdout, "SIXMAX_RULES")
                result = fields(process.stdout, "SIXMAX_NEURAL_EVAL")
                if rules["engine"] != "accounting-v2" or rules["metric"] != "net_after_rake":
                    raise ValueError("evaluation did not use corrected accounting")
                if int(result["hands"]) != args.hands or int(result["eval_seed"]) != seed:
                    raise ValueError("evaluation configuration mismatch")
                row = {"seed": seed, "seed_role": "historical_recheck" if seed == 944104 else "new_test_seed",
                       "opponent": opponent, "rake_profile": profile, "hands": args.hands,
                       "independent_deal_clusters": int(result["clusters"]),
                       "net_bb100": float(result["bb100"]),
                       "approx_ci95_half_bb100": float(result["ci95_cluster_half"]),
                       "table_rake_bb100": float(result["table_rake_bb100"])}
                if profile == "no_rake" and row["table_rake_bb100"] != 0:
                    raise ValueError("zero-rake run charged money")
                if not 0 <= row["table_rake_bb100"] <= 300:
                    raise ValueError("rake exceeded the configured 3 BB per-hand cap")
                results.append(row)
    for seed in SEEDS:
        for opponent in ("tabular", "phh"):
            matching = [r for r in results if r["seed"] == seed and r["opponent"] == opponent]
            if matching[1]["net_bb100"] > matching[0]["net_bb100"] + 0.001:
                raise ValueError("rake increased frozen candidate's net on identical trajectories")
    after = {name: sha256(path) for name, path in paths.items()}
    if before != after:
        raise ValueError("evaluation modified a frozen checkpoint")
    report = {"engine": "accounting-v2", "model_training_environment": "historical pre-audit, no rake",
              "binary_sha256": sha256(binary), "checkpoint_sha256_before": before,
              "checkpoint_sha256_after": after, "rake_scenario_is_operator_specific": False,
              "results": results,
              "limits": ["Frozen weak internal opponents, not the original Pluribus agent or humans",
                         "Intervals reflect sampled deals, not model quality or simulator correctness",
                         "The historical checkpoint was not retrained after the rule changes"]}
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"AUDIT_FROZEN_CHECKPOINTS unchanged=true cases={len(results)} hands={args.hands * len(results)}")


if __name__ == "__main__":
    main()

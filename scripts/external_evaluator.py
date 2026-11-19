#!/usr/bin/env python3
"""Generate evaluator fixtures independently with pinned PokerKit and Treys.

The corpus covers every one of the 7,462 five-card strengths, 10,000 random
hands of each size (5/6/7), and targeted edges under all suit permutations
and reversed card order. No SolverPK evaluator code is imported or copied.

Install/check out the two revisions below, then run:
    python scripts/external_evaluator.py --output /tmp/evaluator-external.tsv
    SOLVERPK_EXTERNAL_EVALUATOR_TSV=/tmp/evaluator-external.tsv \
        cargo test --release external_evaluator_matches -- --ignored --nocapture

The generated JSON records exact source and corpus hashes. Rust establishes
the ordinal mapping from canonical representatives, so numeric score formats
need not match across the three engines.
"""

import argparse
from collections import Counter
import hashlib
import importlib
from itertools import permutations
import json
from pathlib import Path
import platform
import random

from pokerkit.hands import StandardHighHand
from treys import Card, Evaluator


ENGINE_PINS = {
    "PokerKit": {
        "repository": "https://github.com/uoftcprg/pokerkit",
        "revision": "abf795299eaac41183495a0956ce7136ce38ed64",
    },
    "Treys": {
        "repository": "https://github.com/ihendley/treys",
        "revision": "70fbaade2f9b63ee3ea41b54aed7be2c921add7e",
    },
}

# Only evaluation dependencies are fingerprinted. This also works with clean
# source checkouts without installed distribution metadata or a .git folder.
SOURCE_SHA256 = {
    "pokerkit.hands": "8bd1877b4b14c8ec1162742723e5daf9efb52e3d43d99fe871af9c5fed931fa9",
    "pokerkit.lookups": "ab82def3592d51c2c4a644ccd8610634e14f823f13c714d0d54f5009c19fac66",
    "pokerkit.utilities": "96a88973c7b90b820905a44955613bc82b4e2a065d91b26cb0bc4878c84d73e2",
    "treys.evaluator": "c937a9bac94923b08b950fd148a259702faaba188952b151a1ee9552b535d166",
    "treys.lookup": "10b3d5ae9f16de5f900443efaf77e32b87de0284f69e3fc6fb3dae1621a4cdc0",
    "treys.card": "1c9fca60c83b43161787b46654158ea9053b518105fb7f8fbff0e50542f3deba",
}

RANKS = "23456789TJQKA"
SUITS = "cdhs"
STRENGTHS = 7_462

EDGE_HANDS = {
    "wheel_five": "As2c3d4h5s",
    "false_ace_wrap": "QcKhAs2d3c",
    "six_high_over_wheel": "As2c3d4h5s6c",
    "seven_high_over_wheel": "As2c3d4h5s6c7d",
    "wheel_straight_flush": "Ac2c3c4c5c6d7h",
    "highest_straight_flush": "8h9hThJhQhKhAh",
    "flush_and_mixed_straight": "2c3c4c5c9c6d7h",
    "double_trips_six": "AcAdAhKcKdKh",
    "double_trips_seven": "AcAdAhKcKdKh2s",
    "trips_and_two_pairs": "8c8d8hAcAdKcKd",
    "three_pairs_six": "AcAdKcKdQcQd",
    "third_pair_as_kicker": "AcAdKcKdQcQd2s",
    "unpaired_ace_over_third_pair": "5c5d4c4d3c3dAs",
    "quads_board_king_queen_hole": "9c9d9h9sAcKhQd",
    "quads_board_jack_ten_hole": "9c9d9h9sAcJhTd",
    "royal_board_ace_nine_hole": "AhKhQhJhThAc9c",
    "royal_board_king_deuce_hole": "AhKhQhJhThKd2s",
    "six_card_flush": "AcKcQc9c6c3c2s",
    "seven_card_flush": "AcKcJc8c5c3c2c",
    "flush_with_offsuit_king": "AcQcJc9c4c2cKd",
    "quads_and_trips": "TcTdThTsAcAdAh",
    "quads_and_pair_six": "TcTdThTsAcAd",
    "pair_three_kickers": "AcAdKhQsJc9h2s",
    "trips_two_kickers": "3c3d3hAcKd9s2c",
    "pair_six": "2c2dAcKhQs9s",
    "board_straight_beats_hole_pair": "4c5d6h7s8cAsAd",
    "duplicate_rank_in_straight": "5c6c7d7h8s9hAc",
    "high_card_seven": "AcKdJh9s7c5d3h",
}


def card_text(card):
    return RANKS[card % 13] + SUITS[card // 13]


def parse_cards(text):
    if len(text) % 2:
        raise ValueError(f"incomplete card: {text!r}")
    return tuple(
        SUITS.index(text[index + 1]) * 13 + RANKS.index(text[index])
        for index in range(0, len(text), 2)
    )


def check_source_hashes():
    actual = {}
    for name, expected in SOURCE_SHA256.items():
        source = Path(importlib.import_module(name).__file__)
        actual[name] = hashlib.sha256(source.read_bytes()).hexdigest()
        if actual[name] != expected:
            raise ValueError(
                f"{name} does not match the pinned source: {actual[name]}; "
                f"expected {expected}. Use the revisions in ENGINE_PINS."
            )
    return actual


def canonical_hands(evaluator):
    """Decode external lookup keys into cards; do not implement poker ranking."""
    result = {}
    for suited, table in (
        (True, evaluator.table.flush_lookup),
        (False, evaluator.table.unsuited_lookup),
    ):
        for product, external_rank in table.items():
            ranks = []
            for rank, prime in enumerate(Card.PRIMES):
                while product % prime == 0:
                    ranks.append(rank)
                    product //= prime
            if product != 1 or len(ranks) != 5:
                raise ValueError("unexpected external prime-product representation")
            # Cyclic suits keep duplicate ranks distinct and prevent an
            # accidental flush in the unsuited lookup representatives.
            cards = tuple(rank + 13 * (0 if suited else i % 4)
                          for i, rank in enumerate(ranks))
            if external_rank in result:
                raise ValueError(f"duplicate canonical rank {external_rank}")
            result[external_rank] = cards
    if set(result) != set(range(1, STRENGTHS + 1)):
        raise ValueError("external lookup does not cover all 7,462 strengths")
    return result


def generate(samples_per_size, seed):
    evaluator = Evaluator()
    rows = []
    counts = Counter()

    def append(kind, name, cards, expected_treys_rank=None):
        if not 5 <= len(cards) <= 7 or len(set(cards)) != len(cards):
            raise ValueError(f"invalid fixture {name}: {cards}")
        if not all(0 <= card < 52 for card in cards):
            raise ValueError(f"out-of-deck fixture {name}: {cards}")
        names = [card_text(card) for card in cards]
        treys_rank = evaluator.evaluate([Card.new(card) for card in names], [])
        pokerkit_index = StandardHighHand.from_game("".join(names)).entry.index
        if STRENGTHS - treys_rank != pokerkit_index:
            raise AssertionError(
                f"external evaluator disagreement: {name} cards={names} "
                f"Treys={treys_rank} PokerKit={pokerkit_index}"
            )
        if expected_treys_rank is not None and treys_rank != expected_treys_rank:
            raise AssertionError(f"incorrect canonical card decoding at {name}")
        rows.append("\t".join((kind, name, str(treys_rank), str(pokerkit_index),
                               ",".join(map(str, cards)))))
        counts[f"{kind}_{len(cards)}"] += 1

    canonical = canonical_hands(evaluator)
    for external_rank in range(STRENGTHS, 0, -1):
        append("canonical", f"rank_{external_rank:04d}", canonical[external_rank], external_rank)

    # Separate from SolverPK's RNG/deck implementation; no shared random code.
    rng = random.Random(seed)
    for size in (5, 6, 7):
        for sample in range(samples_per_size):
            append("random", f"size{size}_{sample:05d}", tuple(rng.sample(range(52), size)))

    for name, text in EDGE_HANDS.items():
        base = parse_cards(text)
        for suits in permutations(range(4)):
            cards = tuple(13 * suits[card // 13] + card % 13 for card in base)
            suffix = "".join(map(str, suits))
            append("edge", f"{name}_suits{suffix}_forward", cards)
            append("edge", f"{name}_suits{suffix}_reverse", cards[::-1])

    header = ["# solverpk-external-evaluator-v1"]
    header += [f"# engine\t{name}\t{info['revision']}" for name, info in ENGINE_PINS.items()]
    header += [f"# random_per_size\t{samples_per_size}", f"# seed\t{seed}",
               f"# rows\t{len(rows)}", "# columns\tkind\tname\ttreys_rank\tpokerkit_index\tcards"]
    return ("\n".join(header + rows) + "\n").encode("ascii"), dict(sorted(counts.items()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, help="default: output path with .json suffix")
    parser.add_argument("--samples-per-size", type=int, default=10_000)
    parser.add_argument("--seed", type=lambda value: int(value, 0), default=20_261_119)
    args = parser.parse_args()
    if args.samples_per_size < 1:
        parser.error("--samples-per-size must be positive (Rust audit requires >= 10000)")
    metadata_path = args.metadata or args.output.with_suffix(".json")
    if metadata_path.resolve() == args.output.resolve():
        parser.error("metadata and TSV output must use distinct paths")
    source_hashes = check_source_hashes()
    data, counts = generate(args.samples_per_size, args.seed)
    report = {
        "schema": "solverpk-external-evaluator-v1",
        "engines": ENGINE_PINS,
        "engine_source_sha256": source_hashes,
        "fixture_sha256": hashlib.sha256(data).hexdigest(),
        "python": platform.python_version(),
        "seed": args.seed,
        "random_samples_per_size": args.samples_per_size,
        "canonical_strengths": STRENGTHS,
        "edge_fixtures": len(EDGE_HANDS),
        "counts": counts,
        "total_hands": sum(counts.values()),
        "external_disagreements": 0,
        "solverpk_verification": "pending Rust external_evaluator_matches_treys_and_pokerkit",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    metadata_path.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
    metadata_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("EXTERNAL_EVALUATOR_FIXTURE " + json.dumps(report, sort_keys=True), flush=True)


if __name__ == "__main__":
    main()

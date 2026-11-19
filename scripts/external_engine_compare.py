#!/usr/bin/env python3
"""Replay real SolverPK JSONL histories in an immutable PokerKit engine.

All PokerKit monetary inputs are Fraction objects. Native rake is disabled;
fees are independently allocated using PokerKit's ChipsPushing operations.
No upstream source/state is patched. Known differences are counted individually
and rejected hands remain explicitly incomplete. Unknown differences fail CLI.
"""
import argparse
from collections import Counter, defaultdict
from fractions import Fraction
import hashlib
import importlib
import json
from pathlib import Path
import platform
import sys
import time
import warnings

N = 6
SCALE = 10000
RAKE_BPS = 500
RAKE_CAP = 60000
PIN = "abf795299eaac41183495a0956ce7136ce38ed64"
SOURCE_SHA256 = {
    "pokerkit.state": "8c06107cc671a5debcc1cede731ccc659364bb5769b953e1ad60ba3194df40d5",
    "pokerkit.games": "c7d1aee031f6a26ba540f95c7c07d056510dbde00167b83a4b9423a92a375fd3",
    "pokerkit.utilities": "96a88973c7b90b820905a44955613bc82b4e2a065d91b26cb0bc4878c84d73e2",
    "pokerkit.hands": "8bd1877b4b14c8ec1162742723e5daf9efb52e3d43d99fe871af9c5fed931fa9",
    "pokerkit.lookups": "ab82def3592d51c2c4a644ccd8610634e14f823f13c714d0d54f5009c19fac66",
}


def exact(value):
    value = Fraction(value)
    return value.numerator if value.denominator == 1 else str(value)


def card_text(card):
    if not isinstance(card, int) or not 0 <= card < 52:
        raise ValueError(f"invalid card id: {card!r}")
    return "23456789TJQKA"[card % 13] + "cdhs"[card // 13]


def load_engine(root=None):
    if root:
        sys.path.insert(0, str(Path(root).resolve()))
    modules = {name: importlib.import_module(name) for name in SOURCE_SHA256}
    hashes = {}
    for name, module in modules.items():
        digest = hashlib.sha256(Path(module.__file__).read_bytes()).hexdigest()
        hashes[name] = digest
        if digest != SOURCE_SHA256[name]:
            raise ValueError(f"source pin mismatch for {name}: {digest}")
    return modules, hashes


class Difference(Exception):
    def __init__(self, category, detail, known=False):
        super().__init__(detail)
        self.category, self.known = category, known


class Audit:
    def __init__(self, modules):
        self.games = modules["pokerkit.games"]
        self.pk = modules["pokerkit.state"]
        self.counts = Counter()
        self.groups = defaultdict(Counter)
        self.mask_counts = Counter()
        self.errors = []
        self.mask_differences = []
        self.rounding = []
        self.examples = defaultdict(list)
        self.buttons = defaultdict(Counter)

    def count(self, key, group, amount=1):
        self.counts[key] += amount
        self.groups[group][key] += amount

    def example(self, category, record):
        if len(self.examples[category]) < 5:
            self.examples[category].append(record)

    def error(self, hand, group, category, detail, known=False, step=None):
        record = {"hand": hand.get("id"), "group": group, "category": category,
                  "known": known, "step": step, "detail": detail}
        self.errors.append(record)
        self.count("known_errors" if known else "unknown_errors", group)
        self.example(category, record)

    def ledgers(self, hand, group):
        total = hand["total"]
        if len(total) != N or any(not isinstance(x, int) or x < 0 for x in total):
            raise Difference("invalid_ledger", "invalid final contributions")
        ordered = sorted(total)
        uncalled = ordered[-1] - ordered[-2]
        refunds = [0] * N
        refunds[total.index(ordered[-1])] = uncalled * SCALE
        contested = (sum(total) - uncalled) * SCALE
        expected_rake = min(contested * RAKE_BPS // 10000, RAKE_CAP) if hand["visible"] >= 3 else 0
        for name, expected in (("gross", 0), ("raked", expected_rake)):
            ledger = hand[name]
            if any(len(ledger[key]) != N for key in ("payouts", "net", "refunds")):
                raise Difference("invalid_ledger", f"{name}: incorrect vector length")
            checks = {
                "rake": ledger["rake"] == expected,
                "refunds": ledger["refunds"] == refunds,
                "payout_conservation": sum(ledger["payouts"]) + expected == sum(total) * SCALE,
                "net_conservation": sum(ledger["net"]) == -expected,
                "net_definition": ledger["net"] == [ledger["payouts"][i] - total[i] * SCALE for i in range(N)],
                "nonnegative_payouts": all(x >= 0 for x in ledger["payouts"]),
                "folded_no_winnings": all(not hand["folded"][i] or ledger["payouts"][i] == refunds[i] for i in range(N)),
            }
            failed = [key for key, passed in checks.items() if not passed]
            if failed:
                raise Difference("ledger_mismatch", f"{name}: {failed}; expected rake={expected}, refunds={refunds}")
        self.count("ledger_hands_checked", group)
        return contested, expected_rake

    def mask_kind(self, step, state, actor, index, ours, theirs, reason, acted):
        due = step["h"] - step["b"][step["p"]]
        if index == 0 and not ours and theirs and due == 0:
            return "cash_free_fold_policy"
        if index < 2:
            return None
        increment = state.completion_betting_or_raising_amount
        if (not ours and theirs and step["p"] in acted and 0 < due < 2
                and step["m"] == 2 and 0 < increment < 2):
            return "pokerkit_first_short_bet_reopens_prior_actor"
        covered = all(i == actor or not state.statuses[i]
                      or state.stacks[i] + state.bets[i] <= max(state.bets)
                      for i in range(N))
        if (ours and not theirs and covered and reason
                and "There is no reason to complete, bet, or raise" in reason):
            return "solver_covered_opponent_raise"
        if (ours and not theirs and step["p"] in acted and due >= step["m"]
                and increment == step["m"] and reason
                and "already acted and hence cannot raise" in reason
                and "non-full all-in" in reason):
            return "pokerkit_cumulative_reopening_reset"
        return None

    def replay(self, hand, group, contested, expected_rake):
        button = hand["button"]
        seats = [(button + 1 + i) % N for i in range(N)]
        inverse = {seat: index for index, seat in enumerate(seats)}
        cards = [c for pair in hand["hole"] for c in pair] + hand["board"]
        if len(cards) != 17 or len(set(cards)) != 17:
            raise Difference("invalid_cards", "expected 17 distinct cards")
        if len(hand["initial"]) != N or any(x < 2 for x in hand["initial"]):
            raise Difference("invalid_stacks", "six initial stacks >= big blind required")
        automations = tuple(a for a in self.pk.Automation if a not in (
            self.pk.Automation.HOLE_DEALING, self.pk.Automation.BOARD_DEALING,
            self.pk.Automation.CARD_BURNING))
        state = self.games.NoLimitTexasHoldem.create_state(
            automations, True, Fraction(0), (Fraction(1), Fraction(2)), Fraction(2),
            tuple(Fraction(hand["initial"][i]) for i in seats), N,
            mode=self.pk.Mode.CASH_GAME,
            rake=lambda amount, current: (Fraction(0), amount))
        for i, seat in enumerate(seats):
            state.deal_hole("".join(card_text(c) for c in hand["hole"][seat]), i)
        board_offset = 0

        def advance():
            nonlocal board_offset
            for _ in range(16):
                if not state.status or state.actor_index is not None:
                    return
                if state.can_burn_card("??"):
                    state.burn_card("??")
                elif state.can_deal_board():
                    count = state.board_dealing_count
                    if count not in (1, 3) or board_offset + count > 5:
                        raise Difference("board_protocol", f"invalid board count={count}, offset={board_offset}")
                    state.deal_board("".join(card_text(c) for c in hand["board"][board_offset:board_offset + count]))
                    board_offset += count
                else:
                    raise Difference("blocked_reference", "PokerKit has neither action nor pending board/burn")
            raise Difference("board_protocol", "too many non-betting transitions")

        acted = set()
        current_round = 0
        final_folded = [False] * N
        for step_index, step in enumerate(hand["steps"]):
            self.count("snapshots_attempted", group)
            advance()
            if step["r"] != current_round:
                acted.clear()
                current_round = step["r"]
            actor = inverse[step["p"]]
            actual = {
                "actor": None if state.actor_index is None else seats[state.actor_index],
                "street": state.street_index,
                "stack": [state.stacks[inverse[i]] for i in range(N)],
                "bet": [state.bets[inverse[i]] for i in range(N)],
                "folded": [not state.statuses[inverse[i]] for i in range(N)],
                "high": max(state.bets),
                "full_raise": max(Fraction(2), state.completion_betting_or_raising_amount),
                "contributions": [-state.payoffs[inverse[i]] for i in range(N)],
            }
            expected = {"actor": step["p"], "street": step["r"], "stack": step["s"],
                        "bet": step["b"], "folded": step["f"], "high": step["h"],
                        "full_raise": step["m"], "contributions": step["t"]}
            differences = {key: {"solver": expected[key], "pokerkit": actual[key]}
                           for key in expected if expected[key] != actual[key]}
            if differences:
                raise Difference("snapshot_mismatch", json.dumps({"step": step_index, "fields": differences}, default=exact))
            self.count("snapshots_matched", group)
            targets = [None, None] + step["to"]
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                reference_mask = [state.can_fold(), state.can_check_or_call()] + [
                    state.can_complete_bet_or_raise_to(Fraction(to)) for to in step["to"]]
            classifications = {}
            for index, (ours, theirs) in enumerate(zip(step["l"], reference_mask)):
                if ours == theirs:
                    continue
                reason = None
                if index >= 2 and not theirs:
                    try:
                        state.verify_completion_betting_or_raising_to(Fraction(targets[index]))
                    except (ValueError, UserWarning) as exc:
                        reason = str(exc)
                category = self.mask_kind(step, state, actor, index, ours, theirs, reason, acted)
                classifications[index] = category
                category = category or "unknown_mask_difference"
                record = {"hand": hand["id"], "group": group, "step": step_index,
                          "street": step["r"], "actor": step["p"], "action": index,
                          "target": targets[index], "solver": ours, "pokerkit": theirs,
                          "category": category, "reason": reason,
                          "due": step["h"] - step["b"][step["p"]],
                          "solver_full_raise": step["m"], "pokerkit_full_raise": exact(state.completion_betting_or_raising_amount)}
                self.mask_differences.append(record)
                self.mask_counts[category] += 1
                self.count("known_mask_differences" if category != "unknown_mask_difference" else "unknown_mask_differences", group)
                self.example(category, record)
            choice = step["a"]
            if not 0 <= choice < 6 or not step["l"][choice]:
                raise Difference("invalid_exported_action", f"step={step_index}, choice={choice}")
            if not reference_mask[choice]:
                category = classifications.get(choice)
                raise Difference(category or "reference_rejected_action",
                                 f"step={step_index}, action={choice}, target={targets[choice]}", bool(category))
            try:
                if choice == 0:
                    state.fold()
                    final_folded[step["p"]] = True
                elif choice == 1:
                    state.check_or_call()
                else:
                    state.complete_bet_or_raise_to(Fraction(targets[choice]))
            except (ValueError, AssertionError) as exc:
                raise Difference("reference_action_exception", f"step={step_index}: {type(exc).__name__}: {exc}") from exc
            acted.add(step["p"])
            self.count("actions_replayed", group)
        advance()
        if state.status or state.actor_index is not None:
            raise Difference("unfinished_reference", "exported actions ended while PokerKit still needs action")
        if board_offset != hand["visible"] or final_folded != hand["folded"]:
            raise Difference("final_state_mismatch", f"board={board_offset}/{hand['visible']}; folds={final_folded}/{hand['folded']}")
        external = [Fraction(state.payoffs[inverse[i]]) * SCALE for i in range(N)]
        if sum(external) != 0:
            raise Difference("reference_conservation", "PokerKit no-rake payoffs do not sum exactly to zero")
        pushes = [op for op in state.operations if isinstance(op, self.pk.ChipsPushing)]
        positive_pushes = [op for op in pushes if sum(op.amounts) > 0]
        second_total = sorted(hand["total"])[-2]
        live_caps = {min(hand["total"][i], second_total) for i in range(N)
                     if not hand["folded"][i] and hand["total"][i] > 0}
        killed_caps = {min(hand["total"][seats[op.player_index]], second_total)
                       for op in state.operations if isinstance(op, self.pk.HandKilling)
                       and not hand["folded"][seats[op.player_index]]}
        # PokerKit constructs final pots after killing losing showdown hands.
        # A killed short stack can remove a genuine earlier pot boundary. The
        # rational award/fee is unchanged, but independent per-pot integer
        # rounding may differ even when the merged rational result is integral.
        merged_after_killing = (len(live_caps) > len(positive_pushes)
                                and bool(killed_caps.intersection(live_caps - {max(live_caps)})))
        if merged_after_killing:
            self.count("pokerkit_post_killing_merged_pot_hands", group)
            self.example("pokerkit_post_killing_pot_merge", {
                "hand": hand["id"], "live_contribution_caps": sorted(live_caps),
                "native_positive_pots": len(positive_pushes), "native_killed_caps": sorted(killed_caps)})
        fractional_pots = any((Fraction(amount) * SCALE).denominator != 1
                              for op in pushes for amount in op.amounts)
        if fractional_pots:
            self.count("fractional_pot_hands", group)
        fees = [Fraction(0) for _ in range(N)]
        fee_pots = []
        fractional_fee_pots = False
        live = [i for i, folded in enumerate(hand["folded"]) if not folded]
        if len(live) == 1:
            fees[live[0]] = Fraction(expected_rake)
        else:
            pushed = sum((sum(Fraction(a) for a in op.amounts) for op in pushes), Fraction(0))
            if pushed * SCALE != contested:
                raise Difference("reference_pot_amount", f"pushed={exact(pushed*SCALE)}, contested={contested}")
            for op in pushes:
                amount = sum(Fraction(a) for a in op.amounts) * SCALE
                winners = [i for i, a in enumerate(op.amounts) if a > 0]
                if not amount:
                    continue
                if not winners:
                    raise Difference("reference_empty_pot", "positive pot has no winner")
                cost = Fraction(expected_rake * amount, contested * len(winners))
                fractional_fee_pots |= cost.denominator != 1
                fee_pots.append({"pot_index": op.pot_index, "amount_units": exact(amount),
                                 "winners": [seats[i] for i in winners],
                                 "fee_per_winner_units": exact(cost)})
                for i in winners:
                    fees[seats[i]] += cost
        if sum(fees) != expected_rake:
            raise Difference("reference_fee_conservation", f"allocated fees={exact(sum(fees))}, expected={expected_rake}")
        pot_bound = max(1, len(live_caps))
        for name, expected_net, bound in (("gross", external, pot_bound),
                                          ("raked", [external[i] - fees[i] for i in range(N)], 2*pot_bound)):
            deltas = [Fraction(hand[name]["net"][i]) - expected_net[i] for i in range(N)]
            if sum(deltas) != 0:
                raise Difference(f"{name}_payoff_conservation", f"differences sum to {exact(sum(deltas))}")
            # Fractions may cancel after multiple side pots: the relevant
            # rounding sites are individual pot awards and fee allocations.
            can_round = (fractional_pots or merged_after_killing
                         or (name == "raked" and fractional_fee_pots))
            if any(deltas) and (not can_round or max(map(abs, deltas)) > bound):
                raise Difference(f"{name}_payoff_mismatch", f"deltas_units={list(map(exact,deltas))}; bound={bound}; rounding_applicable={can_round}")
            if any(deltas):
                self.count(f"{name}_rational_rounding_hands", group)
                record = {"hand": hand["id"], "group": group, "ledger": name,
                          "deltas_units": list(map(exact, deltas)), "max_allowed_per_player_units": bound,
                          "post_killing_pot_merge": merged_after_killing,
                          "live_contribution_caps": sorted(live_caps), "native_positive_pots": len(positive_pushes)}
                if name == "raked":
                    record["external_fee_pots"] = fee_pots
                self.rounding.append(record)
                self.example(f"{name}_rational_rounding", record)
            else:
                self.count(f"{name}_exact_hands", group)
        self.count("completed", group)

    def hand(self, hand):
        group = "fixed" if hand.get("initial") == [200] * N else "variable"
        self.count("hands", group)
        self.buttons[group][str(hand.get("button"))] += 1
        self.count("exported_actions", group, len(hand.get("steps", [])))
        try:
            contested, rake = self.ledgers(hand, group)
        except Difference as exc:
            self.error(hand, group, exc.category, str(exc))
            return
        except (KeyError, TypeError, ValueError, IndexError) as exc:
            self.error(hand, group, "invalid_trace", f"{type(exc).__name__}: {exc}")
            return
        try:
            self.replay(hand, group, contested, rake)
        except Difference as exc:
            self.error(hand, group, exc.category, str(exc), exc.known)
        except Exception as exc:
            self.error(hand, group, "unexpected_comparator_exception", f"{type(exc).__name__}: {exc}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path, help="real Rust exporter JSONL")
    parser.add_argument("--output", required=True, type=Path, help="complete JSON audit, including every mask difference")
    parser.add_argument("--pokerkit-root", type=Path, help="directory containing the pinned pokerkit package")
    parser.add_argument("--expected-hands", type=int, help="require this count; counts >=12 also require all buttons in both stack groups")
    parser.add_argument("--require-position-coverage", action="store_true", help="require all six buttons in each stack group; optional for small fixtures")
    args = parser.parse_args()
    started = time.monotonic()
    metadata = {"engine": "PokerKit", "revision": PIN,
                "repository": "https://github.com/uoftcprg/pokerkit", "python": platform.python_version(),
                "input": str(args.input.resolve()), "input_sha256": hashlib.sha256(args.input.read_bytes()).hexdigest(),
                "money_scale": SCALE, "rake_bps": RAKE_BPS, "rake_cap_units": RAKE_CAP,
                "no_flop_no_drop": True,
                "expected_hands": args.expected_hands,
                "position_coverage_required": bool(args.require_position_coverage or
                    (args.expected_hands is not None and args.expected_hands >= 12)),
                "mask_count_unit": "action slots; four raise slots may have the same clipped target",
                "comparison": "exact rational PokerKit payoffs; no native rake; external pot-derived fee allocation",
                "rounding_limits": {"gross_per_player_units": N, "raked_per_player_units": 2*N},
                "known_difference_policy": {
                    "cash_free_fold_policy": "Cash-mode PokerKit allows warning-only free folds; Solver masks them.",
                    "pokerkit_first_short_bet_reopens_prior_actor": "PokerKit uses a first increment below BB as reopening threshold; TDA 2026 rule49 requires full bet/raise.",
                    "solver_covered_opponent_raise": "Solver offers raises despite no opponent able to match more than current highest bet; rejecting chosen actions ends that replay.",
                    "pokerkit_cumulative_reopening_reset": "PokerKit global short-allin list can reset before a previous actor returns; a player facing a full cumulative raise is entitled to raise.",
                }}
    try:
        modules, hashes = load_engine(args.pokerkit_root)
        metadata["source_sha256"] = hashes
    except (ImportError, ValueError) as exc:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps({"metadata": metadata, "fatal_error": str(exc), "strict_checks_passed": False}, indent=2) + "\n")
        print(str(exc), file=sys.stderr)
        return 2
    audit = Audit(modules)
    seen = set()
    with args.input.open() as stream:
        for line_number, line in enumerate(stream, 1):
            if not line.strip():
                continue
            try:
                hand = json.loads(line)
                if hand["id"] in seen:
                    raise ValueError(f"duplicate hand id {hand['id']}")
                seen.add(hand["id"])
                audit.hand(hand)
            except (ValueError, KeyError, TypeError) as exc:
                audit.error({}, "invalid", "invalid_jsonl", f"line={line_number}: {exc}")
    coverage = {"buttons_by_group": {group: {str(i): audit.buttons[group][str(i)] for i in range(N)}
                                     for group in ("fixed", "variable")}}
    coverage["all_positions_by_group"] = {group: all(coverage["buttons_by_group"][group].values())
                                          for group in ("fixed", "variable")}
    coverage["balanced_by_group"] = {group: len(set(coverage["buttons_by_group"][group].values())) == 1
                                     and coverage["all_positions_by_group"][group]
                                     for group in ("fixed", "variable")}
    if args.expected_hands is not None and audit.counts["hands"] != args.expected_hands:
        audit.error({}, "coverage", "hand_count_mismatch", f"actual={audit.counts['hands']}, expected={args.expected_hands}")
    require_positions = (args.require_position_coverage
                         or (args.expected_hands is not None and args.expected_hands >= 12))
    if require_positions and not all(coverage["all_positions_by_group"].values()):
        audit.error({}, "coverage", "missing_position_coverage", str(coverage["buttons_by_group"]))
    unknown = audit.counts["unknown_errors"] + audit.counts["unknown_mask_differences"]
    complete = audit.counts["completed"] == audit.counts["hands"] and audit.counts["hands"] > 0
    passed = (unknown == 0 and complete and not audit.errors
              and audit.mask_counts["solver_covered_opponent_raise"] == 0)
    summary = dict(audit.counts)
    summary.update({"fixed": audit.groups["fixed"]["hands"], "variable": audit.groups["variable"]["hands"],
                    "errors": len(audit.errors), "incomplete": audit.counts["hands"] - audit.counts["completed"],
                    "known_errors": audit.counts["known_errors"], "unknown_errors": audit.counts["unknown_errors"],
                    "unknown_mask_differences": audit.counts["unknown_mask_differences"]})
    report = {"metadata": metadata, "elapsed_seconds": round(time.monotonic()-started, 3),
              "strict_checks_passed": passed, "all_hands_replayed": complete,
              "summary": summary, "groups": {k: dict(v) for k, v in audit.groups.items()}, "coverage": coverage,
              "mask_difference_counts": dict(audit.mask_counts), "examples": dict(audit.examples),
              "errors": audit.errors, "mask_differences": audit.mask_differences,
              "rational_rounding_differences": audit.rounding}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("strict_checks_passed", "all_hands_replayed", "summary", "mask_difference_counts", "elapsed_seconds")}))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())

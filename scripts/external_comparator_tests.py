#!/usr/bin/env python3
"""Regression tests using immutable histories emitted by the real Rust engine.

Fixtures 3/207: GREEN run 44609051377, input sha256
bd5bf25915c86134c8df995e6ff45e7d5b9f3ae42eb155f628c070ddc9bcb12c.
Fixtures 195/5477: balanced run 44610655090.
No simulator or evaluator is mocked.
"""
import argparse
import copy
import json
import random
import unittest
import external_engine_compare as comparator

FIXTURES = json.loads(r'''{"3":{"id":3,"seed":1862931088974303797,"button":3,"initial":[75,25,5,25,2,75],"hole":[[8,11],[2,3],[24,46],[5,0],[1,4],[41,21]],"board":[25,36,44,35,51],"steps":[{"r":0,"p":0,"s":[75,25,5,25,1,73],"b":[0,0,0,0,1,2],"t":[0,0,0,0,1,2],"f":[false,false,false,false,false,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,5,7,75],"a":1},{"r":0,"p":1,"s":[73,25,5,25,1,73],"b":[2,0,0,0,1,2],"t":[2,0,0,0,1,2],"f":[false,false,false,false,false,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,7,9,25],"a":5},{"r":0,"p":2,"s":[73,0,5,25,1,73],"b":[2,25,0,0,1,2],"t":[2,25,0,0,1,2],"f":[false,false,false,false,false,false],"h":25,"m":23,"l":[true,true,false,false,false,false],"to":[5,5,5,5],"a":1},{"r":0,"p":3,"s":[73,0,0,25,1,73],"b":[2,25,5,0,1,2],"t":[2,25,5,0,1,2],"f":[false,false,false,false,false,false],"h":25,"m":23,"l":[true,true,false,false,false,false],"to":[25,25,25,25],"a":1},{"r":0,"p":4,"s":[73,0,0,0,1,73],"b":[2,25,5,25,1,2],"t":[2,25,5,25,1,2],"f":[false,false,false,false,false,false],"h":25,"m":23,"l":[true,true,false,false,false,false],"to":[2,2,2,2],"a":1},{"r":0,"p":5,"s":[73,0,0,0,0,73],"b":[2,25,5,25,2,2],"t":[2,25,5,25,2,2],"f":[false,false,false,false,false,false],"h":25,"m":23,"l":[true,true,true,true,true,true],"to":[53,75,75,75],"a":1},{"r":0,"p":0,"s":[73,0,0,0,0,50],"b":[2,25,5,25,2,25],"t":[2,25,5,25,2,25],"f":[false,false,false,false,false,false],"h":25,"m":23,"l":[true,true,true,true,true,true],"to":[60,75,75,75],"a":1},{"r":1,"p":5,"s":[50,0,0,0,0,50],"b":[0,0,0,0,0,0],"t":[25,25,5,25,2,25],"f":[false,false,false,false,false,false],"h":0,"m":2,"l":[false,true,true,true,true,true],"to":[35,50,50,50],"a":3},{"r":1,"p":0,"s":[50,0,0,0,0,0],"b":[0,0,0,0,0,50],"t":[25,25,5,25,2,75],"f":[false,false,false,false,false,false],"h":50,"m":50,"l":[true,true,false,false,false,false],"to":[50,50,50,50],"a":1}],"total":[75,25,5,25,2,75],"folded":[false,false,false,false,false,false],"visible":5,"gross":{"payouts":[2070000,0,0,0,0,0],"net":[1320000,-250000,-50000,-250000,-20000,-750000],"refunds":[0,0,0,0,0,0],"rake":0},"raked":{"payouts":[2010000,0,0,0,0,0],"net":[1260000,-250000,-50000,-250000,-20000,-750000],"refunds":[0,0,0,0,0,0],"rake":60000}},"207":{"id":207,"seed":8338086110644858677,"button":3,"initial":[50,150,150,400,200,150],"hole":[[46,0],[23,20],[4,29],[44,3],[6,38],[42,17]],"board":[34,40,18,33,15],"steps":[{"r":0,"p":0,"s":[50,150,150,400,199,148],"b":[0,0,0,0,1,2],"t":[0,0,0,0,1,2],"f":[false,false,false,false,false,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,5,7,50],"a":1},{"r":0,"p":1,"s":[48,150,150,400,199,148],"b":[2,0,0,0,1,2],"t":[2,0,0,0,1,2],"f":[false,false,false,false,false,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,7,9,150],"a":2},{"r":0,"p":2,"s":[48,146,150,400,199,148],"b":[2,4,0,0,1,2],"t":[2,4,0,0,1,2],"f":[false,false,false,false,false,false],"h":4,"m":2,"l":[true,true,true,true,true,true],"to":[8,13,17,150],"a":1},{"r":0,"p":3,"s":[48,146,146,400,199,148],"b":[2,4,4,0,1,2],"t":[2,4,4,0,1,2],"f":[false,false,false,false,false,false],"h":4,"m":2,"l":[true,true,true,true,true,true],"to":[9,16,21,400],"a":2},{"r":0,"p":4,"s":[48,146,146,391,199,148],"b":[2,4,4,9,1,2],"t":[2,4,4,9,1,2],"f":[false,false,false,false,false,false],"h":9,"m":5,"l":[true,true,true,true,true,true],"to":[19,31,39,200],"a":4},{"r":0,"p":5,"s":[48,146,146,391,161,148],"b":[2,4,4,9,39,2],"t":[2,4,4,9,39,2],"f":[false,false,false,false,false,false],"h":39,"m":30,"l":[true,true,true,true,true,true],"to":[71,111,136,150],"a":1},{"r":0,"p":0,"s":[48,146,146,391,161,111],"b":[2,4,4,9,39,39],"t":[2,4,4,9,39,39],"f":[false,false,false,false,false,false],"h":39,"m":30,"l":[true,true,true,true,true,true],"to":[50,50,50,50],"a":1},{"r":0,"p":1,"s":[11,146,146,391,161,111],"b":[39,4,4,9,39,39],"t":[39,4,4,9,39,39],"f":[false,false,false,false,false,false],"h":39,"m":30,"l":[true,true,true,true,true,true],"to":[95,150,150,150],"a":5},{"r":0,"p":2,"s":[11,0,146,391,161,111],"b":[39,150,4,9,39,39],"t":[39,150,4,9,39,39],"f":[false,false,false,false,false,false],"h":150,"m":111,"l":[true,true,false,false,false,false],"to":[150,150,150,150],"a":1},{"r":0,"p":3,"s":[11,0,0,391,161,111],"b":[39,150,150,9,39,39],"t":[39,150,150,9,39,39],"f":[false,false,false,false,false,false],"h":150,"m":111,"l":[true,true,true,true,true,true],"to":[339,400,400,400],"a":0},{"r":0,"p":4,"s":[11,0,0,391,161,111],"b":[39,150,150,9,39,39],"t":[39,150,150,9,39,39],"f":[false,false,false,true,false,false],"h":150,"m":111,"l":[true,true,false,false,false,false],"to":[200,200,200,200],"a":1},{"r":0,"p":5,"s":[11,0,0,391,50,111],"b":[39,150,150,9,150,39],"t":[39,150,150,9,150,39],"f":[false,false,false,true,false,false],"h":150,"m":111,"l":[true,true,false,false,false,false],"to":[150,150,150,150],"a":1},{"r":0,"p":0,"s":[11,0,0,391,50,0],"b":[39,150,150,9,150,150],"t":[39,150,150,9,150,150],"f":[false,false,false,true,false,false],"h":150,"m":111,"l":[true,true,false,false,false,false],"to":[50,50,50,50],"a":1}],"total":[50,150,150,9,150,150],"folded":[false,false,false,true,false,false],"visible":5,"gross":{"payouts":[0,0,3295000,0,0,3295000],"net":[-500000,-1500000,1795000,-90000,-1500000,1795000],"refunds":[0,0,0,0,0,0],"rake":0},"raked":{"payouts":[0,0,3264999,0,0,3265001],"net":[-500000,-1500000,1764999,-90000,-1500000,1765001],"refunds":[0,0,0,0,0,0],"rake":60000}},"7":{"id":7,"seed":1880319199112755260,"button":3,"initial":[400,25,150,200,10,400],"hole":[[40,7],[31,0],[39,37],[6,28],[18,10],[13,32]],"board":[41,23,5,50,30],"steps":[{"r":0,"p":0,"s":[400,25,150,200,9,398],"b":[0,0,0,0,1,2],"t":[0,0,0,0,1,2],"f":[false,false,false,false,false,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,5,7,400],"a":2},{"r":0,"p":1,"s":[396,25,150,200,9,398],"b":[4,0,0,0,1,2],"t":[4,0,0,0,1,2],"f":[false,false,false,false,false,false],"h":4,"m":2,"l":[true,true,true,true,true,true],"to":[7,12,15,25],"a":3},{"r":0,"p":2,"s":[396,13,150,200,9,398],"b":[4,12,0,0,1,2],"t":[4,12,0,0,1,2],"f":[false,false,false,false,false,false],"h":12,"m":8,"l":[true,true,true,true,true,true],"to":[22,35,43,150],"a":1},{"r":0,"p":3,"s":[396,13,138,200,9,398],"b":[4,12,12,0,1,2],"t":[4,12,12,0,1,2],"f":[false,false,false,false,false,false],"h":12,"m":8,"l":[true,true,true,true,true,true],"to":[26,44,55,200],"a":4},{"r":0,"p":4,"s":[396,13,138,145,9,398],"b":[4,12,12,55,1,2],"t":[4,12,12,55,1,2],"f":[false,false,false,false,false,false],"h":55,"m":43,"l":[true,true,false,false,false,false],"to":[10,10,10,10],"a":1},{"r":0,"p":5,"s":[396,13,138,145,0,398],"b":[4,12,12,55,10,2],"t":[4,12,12,55,10,2],"f":[false,false,false,false,false,false],"h":55,"m":43,"l":[true,true,true,true,true,true],"to":[104,166,203,400],"a":1},{"r":0,"p":0,"s":[396,13,138,145,0,345],"b":[4,12,12,55,10,55],"t":[4,12,12,55,10,55],"f":[false,false,false,false,false,false],"h":55,"m":43,"l":[true,true,true,true,true,true],"to":[121,204,254,400],"a":1},{"r":0,"p":1,"s":[345,13,138,145,0,345],"b":[55,12,12,55,10,55],"t":[55,12,12,55,10,55],"f":[false,false,false,false,false,false],"h":55,"m":43,"l":[true,true,false,false,false,false],"to":[25,25,25,25],"a":1},{"r":0,"p":2,"s":[345,0,138,145,0,345],"b":[55,25,12,55,10,55],"t":[55,25,12,55,10,55],"f":[false,false,false,false,false,false],"h":55,"m":43,"l":[true,true,true,true,true,true],"to":[140,150,150,150],"a":3},{"r":0,"p":3,"s":[345,0,0,145,0,345],"b":[55,25,150,55,10,55],"t":[55,25,150,55,10,55],"f":[false,false,false,false,false,false],"h":150,"m":95,"l":[true,true,true,true,true,true],"to":[200,200,200,200],"a":0},{"r":0,"p":5,"s":[345,0,0,145,0,345],"b":[55,25,150,55,10,55],"t":[55,25,150,55,10,55],"f":[false,false,false,true,false,false],"h":150,"m":95,"l":[true,true,true,true,true,true],"to":[298,400,400,400],"a":3},{"r":0,"p":0,"s":[345,0,0,145,0,0],"b":[55,25,150,55,10,400],"t":[55,25,150,55,10,400],"f":[false,false,false,true,false,false],"h":400,"m":250,"l":[true,true,false,false,false,false],"to":[400,400,400,400],"a":1}],"total":[400,25,150,55,10,400],"folded":[false,false,false,true,false,false],"visible":5,"gross":{"payouts":[5000000,0,4800000,0,600000,0],"net":[1000000,-250000,3300000,-550000,500000,-4000000],"refunds":[0,0,0,0,0,0],"rake":0},"raked":{"payouts":[4971154,0,4772308,0,596538,0],"net":[971154,-250000,3272308,-550000,496538,-4000000],"refunds":[0,0,0,0,0,0],"rake":60000}},"195":{"id":195,"seed":4912038156907443807,"button":1,"initial":[3,200,2,50,10,75],"hole":[[16,22],[6,28],[49,40],[10,50],[43,11],[17,35]],"board":[7,39,38,34,25],"steps":[{"r":0,"p":4,"s":[3,200,1,48,10,75],"b":[0,0,1,2,0,0],"t":[0,0,1,2,0,0],"f":[false,false,false,false,false,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,5,7,10],"a":0},{"r":0,"p":5,"s":[3,200,1,48,10,75],"b":[0,0,1,2,0,0],"t":[0,0,1,2,0,0],"f":[false,false,false,false,true,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,5,7,75],"a":0},{"r":0,"p":0,"s":[3,200,1,48,10,75],"b":[0,0,1,2,0,0],"t":[0,0,1,2,0,0],"f":[false,false,false,false,true,true],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[3,3,3,3],"a":0},{"r":0,"p":1,"s":[3,200,1,48,10,75],"b":[0,0,1,2,0,0],"t":[0,0,1,2,0,0],"f":[true,false,false,false,true,true],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,5,7,200],"a":0},{"r":0,"p":2,"s":[3,200,1,48,10,75],"b":[0,0,1,2,0,0],"t":[0,0,1,2,0,0],"f":[true,true,false,false,true,true],"h":2,"m":2,"l":[true,true,false,false,false,false],"to":[2,2,2,2],"a":1},{"r":0,"p":3,"s":[3,200,0,48,10,75],"b":[0,0,2,2,0,0],"t":[0,0,2,2,0,0],"f":[true,true,false,false,true,true],"h":2,"m":2,"l":[false,true,false,false,false,false],"to":[4,5,6,50],"a":1}],"total":[0,0,2,2,0,0],"folded":[true,true,false,false,true,true],"visible":5,"gross":{"payouts":[0,0,0,40000,0,0],"net":[0,0,-20000,20000,0,0],"refunds":[0,0,0,0,0,0],"rake":0},"raked":{"payouts":[0,0,0,38000,0,0],"net":[0,0,-20000,18000,0,0],"refunds":[0,0,0,0,0,0],"rake":2000}},"5477":{"id":5477,"seed":10910903445963885259,"button":2,"initial":[50,100,10,5,25,150],"hole":[[34,49],[40,44],[45,23],[11,27],[29,21],[50,10]],"board":[39,46,9,37,8],"steps":[{"r":0,"p":5,"s":[50,100,10,4,23,150],"b":[0,0,0,1,2,0],"t":[0,0,0,1,2,0],"f":[false,false,false,false,false,false],"h":2,"m":2,"l":[true,true,true,true,true,true],"to":[4,5,7,150],"a":5},{"r":0,"p":0,"s":[50,100,10,4,23,0],"b":[0,0,0,1,2,150],"t":[0,0,0,1,2,150],"f":[false,false,false,false,false,false],"h":150,"m":148,"l":[true,true,false,false,false,false],"to":[50,50,50,50],"a":1},{"r":0,"p":1,"s":[0,100,10,4,23,0],"b":[50,0,0,1,2,150],"t":[50,0,0,1,2,150],"f":[false,false,false,false,false,false],"h":150,"m":148,"l":[true,true,false,false,false,false],"to":[100,100,100,100],"a":0},{"r":0,"p":2,"s":[0,100,10,4,23,0],"b":[50,0,0,1,2,150],"t":[50,0,0,1,2,150],"f":[false,true,false,false,false,false],"h":150,"m":148,"l":[true,true,false,false,false,false],"to":[10,10,10,10],"a":1},{"r":0,"p":3,"s":[0,100,0,4,23,0],"b":[50,0,10,1,2,150],"t":[50,0,10,1,2,150],"f":[false,true,false,false,false,false],"h":150,"m":148,"l":[true,true,false,false,false,false],"to":[5,5,5,5],"a":1},{"r":0,"p":4,"s":[0,100,0,0,23,0],"b":[50,0,10,5,2,150],"t":[50,0,10,5,2,150],"f":[false,true,false,false,false,false],"h":150,"m":148,"l":[true,true,false,false,false,false],"to":[25,25,25,25],"a":1}],"total":[50,0,10,5,25,150],"folded":[false,true,false,false,false,false],"visible":5,"gross":{"payouts":[625000,0,149999,0,0,1625001],"net":[125000,0,49999,-50000,-250000,125001],"refunds":[0,0,0,0,0,1000000],"rake":0},"raked":{"payouts":[598214,0,143571,0,0,1598215],"net":[98214,0,43571,-50000,-250000,98215],"refunds":[0,0,0,0,0,1000000],"rake":60000}}}''')
ENGINE_ROOT = None

class ComparatorRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.modules, _ = comparator.load_engine(ENGINE_ROOT)

    def audit(self, hand):
        result = comparator.Audit(self.modules)
        result.hand(hand)
        return result

    def mutate_award(self, hand_id, ledger, source, target, amount=1):
        hand = copy.deepcopy(FIXTURES[str(hand_id)])
        for field in ("payouts", "net"):
            hand[ledger][field][source] -= amount
            hand[ledger][field][target] += amount
        return hand

    def assert_financial_rejection(self, hand):
        result = self.audit(hand)
        self.assertEqual(result.counts["ledger_hands_checked"], 1, result.errors)
        self.assertEqual(result.counts["actions_replayed"], len(hand["steps"]), result.errors)
        self.assertEqual(result.counts["completed"], 0, result.rounding)
        self.assertTrue(any(not e["known"] and "payoff" in e["category"]
                            for e in result.errors), result.errors)

    def test_original_real_histories_are_accepted(self):
        for key, hand in FIXTURES.items():
            with self.subTest(hand=key):
                result = self.audit(hand)
                self.assertEqual(result.counts["completed"], 1, result.errors)
                self.assertEqual(result.counts["unknown_mask_differences"], 0)

    def test_one_gross_unit_cannot_move_to_a_player_who_won_no_pot(self):
        self.assert_financial_rejection(self.mutate_award(3, "gross", 0, 1))

    def test_one_raked_unit_cannot_move_to_a_player_who_won_no_pot(self):
        self.assert_financial_rejection(self.mutate_award(3, "raked", 0, 1))

    def test_a_known_tie_does_not_allow_a_different_odd_unit_assignment(self):
        self.assert_financial_rejection(self.mutate_award(5477, "gross", 5, 2))

    def test_a_known_tie_does_not_allow_a_different_fee_assignment(self):
        self.assert_financial_rejection(self.mutate_award(207, "raked", 5, 2))

    def test_seeded_conserved_wrong_recipient_mutations_are_rejected(self):
        rng = random.Random(20261119)
        for trial in range(12):
            ledger = rng.choice(("gross", "raked"))
            recipient, amount = rng.randrange(1, 6), rng.randrange(1, 4)
            with self.subTest(trial=trial, ledger=ledger, recipient=recipient, amount=amount):
                self.assert_financial_rejection(self.mutate_award(3, ledger, 0, recipient, amount))

    def test_truncated_masks_cannot_skip_all_raise_comparisons(self):
        hand = copy.deepcopy(FIXTURES["195"])
        for step in hand["steps"]:
            step["l"] = step["l"][:2]
            step["to"] = []
        result = self.audit(hand)
        self.assertEqual(result.counts["completed"], 0)
        self.assertGreater(result.counts["unknown_errors"], 0)

    def test_final_contributions_must_match_the_replayed_actions(self):
        hand = copy.deepcopy(FIXTURES["7"])
        hand["total"][1] += 1
        hand["total"][2] -= 1
        for ledger in ("gross", "raked"):
            hand[ledger]["payouts"][1] += comparator.SCALE
            hand[ledger]["payouts"][2] -= comparator.SCALE
        result = self.audit(hand)
        self.assertEqual(result.counts["completed"], 0)
        self.assertGreater(result.counts["unknown_errors"], 0)

    def test_malformed_snapshot_types_lengths_and_ranges_are_rejected(self):
        changes = (("l", [1] * 6), ("to", [True] * 4), ("p", 6), ("r", 4),
                   ("s", [200] * 5), ("f", [0] * 6))
        for field, value in changes:
            with self.subTest(field=field):
                hand = copy.deepcopy(FIXTURES["195"])
                hand["steps"][0][field] = value
                result = self.audit(hand)
                self.assertEqual(result.counts["completed"], 0)
                self.assertGreater(result.counts["unknown_errors"], 0)
        hand = copy.deepcopy(FIXTURES["195"])
        hand["button"] = 6
        result = self.audit(hand)
        self.assertEqual(result.counts["completed"], 0)
        self.assertGreater(result.counts["unknown_errors"], 0)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--pokerkit-root")
    args, remaining = parser.parse_known_args()
    ENGINE_ROOT = args.pokerkit_root
    unittest.main(argv=[__file__, *remaining])

# Frozen Pluribus PHH imitation vs Monte Carlo — 2026-11-19

The benchmark is implemented in `src/phh_eval.rs` and can be reproduced with:

```bash
cargo run --release -- --evaluate-phh --model pluribus-imitation.bin --candidate phh --hands 100000 --samples 32 --seed 74101
cargo run --release -- --evaluate-phh --model pluribus-imitation.bin --candidate phh --hands 100000 --samples 32 --seed 94211
cargo run --release -- --evaluate-phh --model pluribus-imitation.bin --candidate uniform --hands 100000 --samples 32 --seed 74101
cargo run --release -- --evaluate-phh --model pluribus-imitation.bin --candidate mc --hands 100000 --samples 32 --seed 74101
```

Provenance:
- Frozen `SPKIMT01` policy from [full 10K Pluribus import](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44541033820)
- [400K evaluation action](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44542417626)
- All 26 Rust tests passed

| Candidate vs fixed Monte Carlo baseline | Candidate bb/100 | Approx paired-deal 95% CI half-width |
|---|---:|---:|
| Frozen PHH policy, seed 74101 | -85.575 | 3.383 |
| Frozen PHH policy, seed 94211 | -86.513 | 3.417 |
| Uniform legal random candidate | -377.017 | 10.178 |
| Identical Monte Carlo policy control | +0.566 | 6.278 |

Each comparison consists of 100,000 hands with a shared deal per two-hand pair, hole cards swapped across positions, and the small blind alternating. The learned policy never updates during evaluation. The opponent is a non-learning Monte Carlo *heuristic*, not CFR or GTO. Each Monte Carlo decision draws 32 equity outcomes from uniform random opponent holdings.

The PHH policy learned six-player action behavior. This evaluation **pools the number-of-players bucket** to transfer it into Heads-Up, because direct Heads-Up bins are largely out of distribution. All observed decision groups have aggregate training counts, but this does not imply distributional generalization. Its historical labels contain no oracle EV. The PHH model uses coarse preflop hole-card structure, street, pot odds, and position; it does not condition on board texture, detailed betting history, or opponent ranges. Neither the original 6-player policy nor a hypothetical 6-max outcome is tested here.

**Conclusion:** the PHH imitation transfers some exploitable structure relative to a uniform random player, but it is **significantly weaker than the fixed Monte Carlo analysis bot Heads-Up**. This does not establish that playing imitated Pluribus hands is worse in its original six-player game. A correct next evaluation requires a validated playable 6-max engine with comparable rules, legal bet sizing and held-out opponent populations; ideally train an outcome/EV network or adapt with self-play and use frozen head-to-head tests.

Note: a 'decision coverage' percentage of 100% refers only to the pooled, coarse table having nonzero counts, not sufficient support across the original game states.

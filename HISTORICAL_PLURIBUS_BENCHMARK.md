# Historical Pluribus decision benchmark — 2026-11-19

## Provenance

- Historical source: [uoftcprg/phh-dataset](https://github.com/uoftcprg/phh-dataset), all 10,000 Pluribus hands.
- Frozen 10M policy: [Actions 44533415706](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44533415706).
- Frozen 20M continuation: [Actions 44534678539](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44534678539).
- [Evaluation run 44545805131](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44545805131).

## Procedure

PHH records were re-imported using the 96-byte observation format with actor identity and the original minimum-bet scale. Only decisions explicitly attributed to player name `Pluribus` were evaluated, excluding the other computer/human agent names. The model sees its own hole cards, public cards, pot, to-call, action mask and button position; never unseen opponent cards. The historical currency/chip units were normalized to the self-play simulator's 2-chip big blind.

Our 10M and 20M agents were trained Heads-Up using 640 coarse action-state buckets; the original Pluribus historical contexts are **6-player**. We evaluated each frozen bot's distribution over abstract {fold, check/call, bet/raise} using equity Monte Carlo versus **one** random opponent holding with 32 and 64 samples. This is not a valid counterfactual 6-player EV estimate, but is a reproducible *behavior agreement* diagnostic.

| Quantity | 32 equity samples | 64 equity samples |
|---|---:|---:|
| Total historical decisions | 91,356 | 91,356 |
| Pluribus decisions isolated | 15,169 | 15,169 |
| Skipped unknown actor cards | 0 | 0 |
| Self-play 10M agreement | 25.578% | 25.697% |
| Self-play 20M agreement | 36.700% | 36.898% |
| Monte Carlo reference agreement | 31.940% | 30.193% |
| Self-play 10M negative log likelihood | 3.55720 | 3.55584 |
| Self-play 20M negative log likelihood | 3.46967 | 3.45184 |
| Uniform over legal actions log loss | 0.99896 | 0.99896 |

Pluribus historical action frequencies on the examined observations:
fold 8,007; check/call 4,035; bet/raise 3,127.
By street: preflop 10,299, flop 2,317, turn 1,513, river 1,040.

27 Rust tests passed and all 10,000 PHH files imported with zero rejects.

## Interpretation and limitations

- 20M matches Pluribus actions more often than 10M, **despite** losing to 10M in a previous direct Heads-Up duel. These evaluate different capabilities; no contradiction.
- Both frozen agents have much worse action negative log likelihood than uniform legal actions on out-of-domain 6-max histories. This suggests overconfident predictions and limited transfer.
- The Monte Carlo reference is a hand-strength heuristic, not a solver. Its agreement with Pluribus is not an EV metric.
- Action size is discarded in the comparison. Only three broad categories are scored, even though real NLHE has many bet sizes.
- Historical actions are not ground-truth optimal actions. Disagreement is not necessarily an error; even matching Pluribus does not prove GTO quality.
- The PHH acting player's private cards are known because these are fully revealed computer-game records. No opponent private information is used in the model features.
- The current evaluation uses one random-opponent equity estimate, which is mismatched to the 6-player game and could substantially change the resulting model action distribution.
- The evaluation's reported Monte Carlo accuracy is the percentage of exact matches with the deterministic equity-threshold heuristic, **not** a probability-based log loss for that heuristic.
- These historical outcomes do not supply counterfactual opponent responses, so no reliable head-to-head winrate can be inferred.

## Next measurement

Build a validated playable 6-max game engine and train/evaluate policies in the same action/state space, then test independent frozen head-to-head games and, where tractable, CFR-based EV-loss against a separate reference. Keep the Heads-Up benchmark results independently.

# Statistical control evaluation — 2026-11-19

Fresh 10M-hand trained checkpoint from [GitHub Actions 44532239059](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44532239059), evaluated by [GitHub Actions 44532985567](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44532985567).

Each matchup uses 100,000 hands, two seats per shared-deal pair, 16 sampled equity outcomes per decision, fixed policies, with training disabled. The reported 95% bands are approximate paired-cluster uncertainty bands, not exact robust confidence intervals.

| Frozen opponent | Candidate bb/100 | Approximate 95% interval |
|---|---:|---:|
| Uniform random legal actions | +178.657 | +169.129 to +188.185 |
| Monte Carlo equity heuristic, no training | +42.277 | +27.407 to +57.147 |
| Identical clone of trained policy (negative control) | +0.538 | -9.381 to +10.457 |

18 tests passed.

## Interpretation

The 10M policy outperformed both fixed control opponents in this simplified Heads-Up simulation. Its near-zero result against an identical clone is an important null sanity check, meaning the results are not merely an unavoidable first-seat advantage.

This does **not** establish GTO strength, generalization to humans, or absence of all statistical bias. The Monte Carlo opponent is a hand-strength heuristic, not an optimal strategy or equity-based solver; it uses 16 random opponent/board rollouts per action. Its decision thresholds are fixed, so it can be exploited.

The 10M checkpoint contains policies trained with the updated main evaluator, corrected seat rotation, and independently seeded dealing. 640 tabular observation buckets and three coarse action categories remain important limitations. The 10M training run was started before the statistical-control CLI was added; it is evaluated here without further training.

## Next checks

- Test several independent training seeds and a broader league of static opponent types.
- Validate the evaluator against an independent reference and expand betting-rule fixtures.
- Compare to trained checkpoints at 5M and 10M using identical test schedules.
- Distinguish improvements in general quality from specialization against this league.

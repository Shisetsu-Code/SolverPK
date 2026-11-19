# Playable six-player Hold'em simulator and evaluation

Implementation: `src/sixmax.rs` (Rust standard library only). It is **independent** of the original Heads-Up `selfplay.rs` and PHH replay importer.

## Execute

```bash
cargo test --all-targets
cargo run --release -- --sixmax-train --hands 100000 --seed 42 --checkpoint sixmax.bin
cargo run --release -- --sixmax-train --resume --hands 900000 --seed 43 --checkpoint sixmax.bin
cargo run --release -- --sixmax-eval --hands 120000 --seed 92001 --checkpoint sixmax.bin --opponent random
```

To test against the frozen Pluribus *imitation*, first download `pluribus-imitation.bin` from the archived workflow artifact at [PHH import 44541033820](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44541033820):

```bash
cargo run --release -- --sixmax-eval --hands 120000 --seed 92001 --checkpoint sixmax.bin --opponent phh --phh-model pluribus-imitation.bin
cargo run --release -- --sixmax-eval --hands 120000 --seed 92001 --checkpoint sixmax.bin --opponent phh --candidate-phh --phh-model pluribus-imitation.bin
cargo run --release -- --sixmax-eval --hands 120000 --seed 92001 --checkpoint sixmax.bin --opponent clone
```

## Supported table functionality

- Six fixed seats, button/SB/BB rotation, preflop and postflop order.
- 100 BB effective initial stacks (200 chips, 1/2 blinds), independent cash-style deals.
- Fold, check, call and four **abstract** no-limit raise targets: approximately 33%, 75%, 100% pot and all-in, subject to minimum-raise and stack caps. The engine accepts validated numeric raise targets.
- Reopening after full raises; a short all-in does not reopen re-raising for previous actors.
- All-in eligibility, multiway main/side pot calculation, unmatched-bet return through pot tiers and split-pot odd chip allocation.
- Exact seven-card evaluator, independent RNG for each deal versus policy actions.
- No rake and no external poker platform integration.

## Learning

- 6 independently trained tabular softmax actor-critic policies, stored in the `SPK6MAX3` binary checkpoint format. **Not a neural network or GTO/CFR solver.**
- Input bins include street, private-card class, number of live opponents, pot odds, position, public board texture, observed betting aggression and a coarse three-class estimate of opponent strength inferred only from public actions.
- That three-class range estimator is heuristic Bayesian updating using fixed likelihoods, **not an accurate distribution over all 1,326 private-card combinations**.
- All future board cards are masked in observations.
- Each training hand returns terminal chip rewards; the model updates from full-hand reward with per-state baselines.
- The policy does **not** currently track full betting sequences or learn a calibrated player-specific opponent-range posterior.

## Validation and results

The [six-max CI](https://github.com/Shisetsu-Code/SolverPK/actions/workflows/sixmax.yml) runs unit/regression tests, 100k self-play hands and fixed-policy comparisons against random actions, the Pluribus imitation and a same-policy clone.

The [generation comparison](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44548708132) trained a 100k generation from scratch, continued to 1M and evaluated each against the frozen Pluribus imitation on the same held-out deal seeds with 120k hands per comparison:

| Frozen agent | Against five PHH imitation bots | Approx. six-hand-cluster CI half-width |
|---|---:|---:|
| 100k hands | +138.348 bb/100 | 20.886 bb/100 |
| 1M hands | +116.285 bb/100 | 15.324 bb/100 |

These intervals overlap. The effect of training 900k additional hands is **not conclusively positive** in this matchup. The opponent is a coarse imitation of six-handed Pluribus historical actions, not Pluribus itself.

The six-handed PHH imitation versus itself and trained-model versus itself are sanity checks; even nearly zero same-policy EV is not proof that the engine is a faithful implementation of every NLHE betting edge case.

## Outstanding work for scientific validity

1. Cross-validate rules and side-pot awards against a fully independent six-player NLHE implementation, including incomplete raises, odd chips and sit-out/busted-seat permutations.
2. Calibrate and expand opponent range beliefs, or implement sampled/bucketed 169/1,326-combination distributions; add complete betting history rather than summary bins.
3. Incorporate PHH demonstrations into the **same six-max trainer**, rather than treating the frozen imitation only as an opponent.
4. Evaluate against multiple opponents and seeds with cluster-aware confidence intervals and compare generations in direct head-to-head games.
5. Build genuinely parallel arenas on CPU/GPU. The current simulator loops hands sequentially.
6. Add exploitability/EV-loss evaluation against a validated multiplayer search approximation. Exact multiplayer GTO is not provided.

Earlier Heads-Up datasets/checkpoints remain separate. Do not interpret these simulated bb/100 numbers as expected financial returns against human players.

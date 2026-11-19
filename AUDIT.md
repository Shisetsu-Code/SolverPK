# Audit findings — 2026-11-19

## Scope

Reviewed original Heads-Up simulation, tabular self-play, RNG, and frozen generation duel.
Historical 1M/11M checkpoints are retained unchanged. Those policies were trained using
the **old** environment; correct evaluation does not retroactively fix training data.

## Confirmed defects and fixes

1. **Training blind-position bias (critical)**: self-play passed `hand + hand % rooms` to
   the game. With 64 rooms, this is always even. Physical player 0 always paid SB,
   so the population never experienced independently alternating seat positions.
   Fixed: pass `hand` directly. Logical rooms remain sequential, not actual threads.

2. **Evaluation blind-position bias (critical)**: frozen duels swapped both the agent
   seat mapping and the blind index, unintentionally keeping the first policy on
   small blind/button every hand. Baseline evaluation used the same pattern.
   Fixed: leave agent-to-seat mapping fixed and alternate only the blind.

3. **All-in unresolved call (critical)**: betting loops terminated when either stack
   reached zero, without allowing the still-funded opponent to respond to an
   outstanding all-in bet. This could force a showdown without a legal call/fold.
   Fixed: permit an outstanding response, then close the betting round when
   no further live opponent action exists. Added regression coverage for base engine.

4. **Pseudo-independent evaluation seeds (high)**: `seed + hand_index` made seeds
   77101 and 77102 produce nearly identical streams offset by exactly one hand
   (and therefore misleadingly similar net outcomes/showdown counts).
   Fixed: combine seed with a multiplicatively scrambled hand counter by XOR
   before SplitMix64 seed mixing. Added cross-seed regression tests.

5. **RNG seed quality and bounded integer sampling (medium)**: nearby integer
   seeds were fed directly into xorshift64* state; bounds used modulo.
   Fixed: SplitMix64 initial state whitening, rejection sampling for unbiased bounds.
   This is a deterministic simulation PRNG, **not a cryptographic RNG**.

## Other notable limitations (not yet fixed)

- Policies are tabular actor-critic (640 coarse buckets), not neural nets or GTO.
  The reward uses whole-hand chip outcomes for every previous decision.
- Monte Carlo equities use just 4 samples/action in archived training; high noise.
- Betting size abstraction is one half-pot raise; no complete NLHE action space.
- No multiway/side-pot engine; cash-style independent heads-up hands only.
- Different bet sequences may consume different amounts of RNG; even with
  common seeds, two trajectories are not strict paired controlled evaluations.
- All-in rule edge cases require additional scripted fixtures beyond this pass.
- The 1M and 11M checkpoint policies are trained on biased historical hands;
  compare for diagnosis only, not as unbiased representations of learning quality.

## Validation

GitHub Actions `duel.yml` loads the historical checkpoints and runs frozen
head-to-head tests using two independent seeds. CI executes Rust unit tests.
See GitHub Actions artifacts and logs for results; do not treat historical
published bb/100 figures as independently validated estimates.

## Recommended next steps

- Build a betting state machine with action legalization and scripted fixture
  tests (blind rotation, min raise, short all-in, uncalled refund, split pot).
- Isolate per-hand deal RNG and policy RNG, add sampled-hand fingerprints,
  and implement truly paired/mirrored deals.
- Cross-check 5/7-card ranks versus an independent evaluator, including
  enumerated edge-case vectors.
- Retrain from scratch with unbiased seat rotation and store periodic frozen
  checkpoints; evaluate against held-out agent populations.
- Increase and benchmark equity samples; measure policy robustness across
  multiple seeds and opponents.

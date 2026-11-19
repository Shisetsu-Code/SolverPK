# Real neural six-max policy (prototype)

This repository previously used **tabular actor-critic**, not a neural network. That remains available in `sixmax.rs` as a reference baseline. The new `src/neural.rs` contains an actual trainable multilayer perceptron using Rust's standard library.

## Architecture

- MLP input: 64 numeric features; hidden: 32 ReLU; output: 6 masked-softmax poker actions.
- Six actions: fold, check/call, 33% pot, 75% pot, pot and all-in abstract raise targets.
- Backpropagation implemented in Rust, with policy-gradient REINFORCE and a moving reward baseline.
- Alternative supervised pretraining: minimize policy cross-entropy to a frozen tabular teacher.
- Inputs use only private hole cards owned by the actor plus publicly observed street/board, pot, call, stack, position, public betting pressure, remaining players and **coarse** public-action opponent beliefs.
- Unrevealed board cards and other players' hole cards are not provided.
- Binary checkpoint `SPKNN001` is versioned and different from the old tabular checkpoint.

### Commands

```bash
cargo run --release -- --sixmax-train --hands 100000 --seed 4101 --checkpoint tabular.bin

# Train from scratch using neural REINFORCE:
cargo run --release -- --sixmax-neural-train --hands 20000 --seed 4101 --checkpoint neural.bin

# Optionally initialize from the frozen teacher strategy:
cargo run --release -- --sixmax-neural-distill --hands 100000 --teacher tabular.bin --checkpoint distilled.bin

# Compare frozen network against five copies of the tabular baseline:
cargo run --release -- --sixmax-neural-eval --hands 12000 --seed 9401 --checkpoint distilled.bin --tabular tabular.bin

# Resume an existing network:
cargo run --release -- --sixmax-neural-train --resume --hands 5000 --seed 4102 --checkpoint distilled.bin
```

## Measured first results

[GitHub Actions neural training benchmark](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44562105608)

| Measurement | Result |
|---|---:|
| Rust tests | 44/44 passed |
| 20,000-hand neural REINFORCE from scratch | ~52,850 hands/s (GitHub Actions CPU) |
| Neural vs trained tabular after 20k hands | -845.767 bb/100 (95% approximate ±442.119) |
| 100,000-hand teacher distillation | ~1,035,461 policy updates, 3.533 s |
| Distilled neural vs trained tabular over 12k hands | +68.104 bb/100 (95% approximate ±118.366) |
| Neural checkpoint after resume | ~152,045 gradient steps |

**Interpretation:** the randomly initialized neural network trained with sparse full-hand rewards performed poorly. Distillation greatly improved its result, but at 12,000 hands the confidence interval includes zero. It is not yet demonstrated stronger than the teacher. The teacher itself is not GTO. Training, execution and serialization are functional; strategy quality and robustness require more controlled benchmarks.

## Important limits

1. Training/inference use CPU currently. No AMD ROCm/Vulkan GPU acceleration has been implemented yet.
2. This is a small ~2.3k-parameter baseline, not the proposed 0.5–2 million parameter model; scaling requires a richer feature architecture and an optimized batched backend.
3. Opponent beliefs are still **3-class heuristic estimates**, not full 169-class or 1,326-combination weighted ranges.
4. The policy observes summaries of betting pressure, not the full ordered action history.
5. Main six-max engine needs independent conformance checking before serious poker strength claims.
6. The PHH imitator is a frozen coarse human-like/Pluribus-action approximation; it is not Pluribus, and the neural model here distills the tabular agent rather than directly consuming PHH observations.

**Next milestones:** replace range beliefs with card-blocked weighted combination distributions, encode full action histories, introduce GPU batch inference/training, train much larger networks, and establish held-out league evaluation and EV-loss versus a validated search reference.

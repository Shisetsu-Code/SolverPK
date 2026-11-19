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

## One-million-hand distillation follow-up

[GitHub Actions 44562688853](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44562688853) restored the stronger frozen 1M-hand scratch-trained tabular teacher and ran one million fresh **supervised** teacher rollouts. The 44 tests passed.

| Neural checkpoint | Against five tabular teacher copies | Against five PHH imitation bots |
|---|---:|---:|
| After 250k teacher deals | +19.167 ±35.532 bb/100 (60k hands) | +125.942 ±20.737 bb/100 (60k hands) |
| After 1M teacher deals | +14.397 ±25.754 bb/100 (120k hands) | +129.908 ±15.269 bb/100 (120k hands) |

The neural net used about 9.3M gradient updates across the million deals. There is **no statistically conclusive advantage over the teacher**; the point estimates remain near parity. Results versus PHH only measure superiority against a coarse six-player *imitation* of Pluribus, not against Pluribus itself. Both evaluations use no rake, simplified action sizing and the same six-max engine.

These are CPU-based measurements from GitHub Actions runners. No RX 6800 XT benchmark has been performed.

## 100 millones de manos de self-play neuronal — finalizado

**Estado verificado:** [GitHub Actions 44564068762](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44564068762) terminó con `completed/success`, diez bloques 10M exitosos, 44 tests aprobados y `CONFIRMED_100M_HANDS=100000000`.

Continuó el checkpoint neuronal previamente **distilado con 1M** de manos desde el tabular. Por tanto, «100M» significa **100 millones de manos sintéticas nuevas de self-play**, no 100 millones de historiales PHH. Se utilizó la misma MLP 64→32→6, en CPU, con learning rates decrecientes de 0,00060 a 0,000075 en bloques de 10M. Las actualizaciones neuronales acumuladas al final fueron **1.163.673.406**.

### Evaluación congelada

Cada evaluación usa **120.000 manos** por rival con semilla `944104` y seis posiciones de botón por grupo. El valor mostrado es `bb/100 ± semiancho IC 95 % aproximado`:

| Checkpoint | Vs. cinco copias del profesor tabular | Vs. cinco copias del imitador PHH |
|---|---:|---:|
| Inicial, tras distilación 1M | −22,332 ±25,077 | +112,355 ±15,233 |
| 10M nuevas | +24,149 ±8,568 | +29,530 ±6,015 |
| 50M nuevas | +36,948 ±13,463 | +44,428 ±8,284 |
| **100M nuevas** | **+60,114 ±14,417** | **+47,346 ±8,090** |

**Conclusión:** hay una mejora clara en esta simulación frente al profesor tabular, pero **la política perdió rendimiento relativo al checkpoint inicial** contra el imitador PHH. No se demostró GTO, ni robustez frente a rivales distintos, ni mejora generalizada en otros entornos. No se estimó varianza entre entrenamientos independientes.

### Artefactos y reproducción

- `neural-100m-full-evidence`: los diez checkpoints, logs y evaluaciones.
- `neural-100m-checkpoint-10m`, `neural-100m-checkpoint-50m`, `neural-100m-checkpoint-100m`.
- SHA-256 modelo final `a712525d815872286eb960f330b5f585e4218fef627f7276b2e2e1adb08a8ac4`.
- Workflow `.github/workflows/neural-100m.yml`.
- El experimento se ejecutó en el commit `0bca3c95a31149d8cb246dd89afd92cc5ffc8170`. Los resultados posteriores deben declarar su propio commit.
- Los artefactos tienen retención de 30 días; conservarlos externamente.

El informe técnico completo está en [PROJECT_STATUS.md](PROJECT_STATUS.md). La **lista de próximas mejoras**, con validación independiente del motor, liga de rivales, checkpoints reproducibles, rangos de 1.326 combos, historial de apuestas, actor–critic v2 y GPU AMD, está en [ROADMAP.md](ROADMAP.md). Evitar una nueva campaña masiva antes de validar la generalización del modelo actual.

# Neural six-max policy: current V2 and historical V1

**Current status — 2026-11-19:** the V2 implementation passed [140 tests](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44594831850): 121 unit tests, 17 CLI tests and two release audit tests. A three-seed pilot trained 100,000 fresh self-play hands per seed. It improved against the uniform reference and declined on average against the other seven references; **no champion was selected and no new 100M campaign was run**. [NEURAL_V2.md](NEURAL_V2.md) contains the full protocol, results and evidence.

The current neural actor is implemented with Rust's standard library. The separate tabular agent remains available as a reference. Historical V1 weights and results are preserved below; [AUDIT_SIXMAX.md](AUDIT_SIXMAX.md) records the earlier engine/accounting audit and frozen V1 reevaluation.

## Current V2 architecture and learning

- MLP: **128 inputs → 32 ReLU units → 6 masked-softmax actions**, with **4,326 trainable weights and biases**.
- Observation: 52 hero-card indicators, 52 visible-board indicators and 24 public scalars. Each known card retains its exact rank/suit association. Swapping hole-card or visible-board order leaves the observation unchanged. Opponents' private cards and unrevealed board cards are excluded.
- Six actions: fold, check/call, 33% pot, 75% pot, pot and all-in abstract raise targets.
- Self-play reward: the acting player's terminal **net return after rake divided by 100 BB**. Rewards and advantages are not clipped. Each player's policy-gradient terms use that player's own return.
- The policy and baseline remain frozen throughout a hand. All decision gradients are accumulated before one update; the fixed normalization is six players, not the number of decisions. A common gradient-norm limit and parameter bounds provide numerical safeguards.
- The baseline is a moving scalar, not a neural critic. Optional distillation minimizes cross-entropy to a frozen tabular teacher; its update procedure is separate from self-play.

The observation layout is documented in [sixmax_observation.rs](src/sixmax_observation.rs); inference, learning and checkpoint validation are in [neural.rs](src/neural.rs). These changes correct the representation and update semantics. They do not establish competitive poker strength.

### Checkpoints and continuation

| Contract | Current `SPKNN002` | Historical `SPKNN001` |
|---|---|---|
| Network input | Suit-aware 128D | Original 64D observation |
| Training | Fresh training and explicit `--resume` | Evaluation only; training is rejected |
| Inference | V2 card planes and public fields | Exactly the original 64 terms per hidden unit |
| Training provenance | Recorded training kind, seed, hand interval, learning rate and rake configuration | Historical exposure cannot be reconstructed fully from the file |

V2 also stores architecture/encoder/optimizer versions, the baseline, decision/update counters and an integrity checksum. Saving uses a temporary file and atomic publication. Fresh runs reject an existing destination; `--resume` must explicitly load V2. A resumed run with the **same seed continues its recorded hand index** rather than replaying that seed from hand zero. Matching configuration and hand order reproduce continuous training; there is no silent V1 conversion.

The current manual workflow is [neural-v2-train.yml](.github/workflows/neural-v2-train.yml). It creates a new output or copies an explicitly supplied V2 checkpoint before resuming, evaluates frozen weights and saves the source commit and checksums. The original V1 100M recipe is archived in [docs/history/workflows/neural-100m-v1.yml](docs/history/workflows/neural-100m-v1.yml).

### Commands

These examples use 5% rake, a 3 BB cap and no-flop-no-drop. Use `--rake-bps 0` for an explicitly rake-free experiment. New checkpoint filenames must not exist.

```bash
# Train a fresh V2 model:
cargo run --release -- --sixmax-neural-train --rake-bps 500 --rake-cap-bb 3 --hands 100000 --seed 73001 --learning-rate 0.0005 --checkpoint neural-v2.bin

# Resume V2 on the next 100,000 indices of the same seed:
cargo run --release -- --sixmax-neural-train --rake-bps 500 --rake-cap-bb 3 --resume --hands 100000 --seed 73001 --learning-rate 0.0005 --checkpoint neural-v2.bin

# Frozen league with its five default behavioral references:
cargo run --release -- --sixmax-neural-league --rake-bps 500 --rake-cap-bb 3 --hands 12000 --seed 990071 --checkpoint neural-v2.bin

# Add two frozen neural references; the option is repeatable:
cargo run --release -- --sixmax-neural-league --rake-bps 500 --rake-cap-bb 3 --hands 12000 --seed 990073 --checkpoint neural-v2.bin \
  --rival-checkpoint audit/2026-11-19/neural-v2/models/neural-v1-100m.bin \
  --rival-checkpoint audit/2026-11-19/neural-v2/models/neural-v2-73001-initial.bin

# Optional teacher distillation into a separate fresh V2 file:
cargo run --release -- --sixmax-train --rake-bps 500 --rake-cap-bb 3 --hands 100000 --seed 4101 --checkpoint tabular.bin
cargo run --release -- --sixmax-neural-distill --rake-bps 500 --rake-cap-bb 3 --hands 100000 --seed 6101 --teacher tabular.bin --checkpoint distilled-v2.bin

# Frozen comparison against the tabular teacher:
cargo run --release -- --sixmax-neural-eval --rake-bps 500 --rake-cap-bb 3 --hands 12000 --seed 990071 --checkpoint distilled-v2.bin --tabular tabular.bin
```

### Frozen league and current pilot

The league includes uniform legal, calling station, tight-aggressive, loose-aggressive and nit references by default. They are simple heuristics without competitive calibration. Add neural opponents with repeated `--rival-checkpoint`, a tabular opponent with `--tabular`, or a PHH imitator with `--phh-model`. Each matchup places the candidate against five copies of that rival, rotates the button through six positions, and shares deals across rivals. `--hands` means **hands per rival**, not the total across the league.

All candidates and opponents remain frozen. Approximate 95% Student-t intervals use six-hand deal clusters; they are per-rival intervals without a multiple-comparison adjustment. Evaluation rejects overlap with recorded training deal streams. Historical V1 or external-teacher provenance remains explicitly unverified; the guard does not prove an independent test set when that history is missing. The output reports descriptive results and the worst observed rival, with **no automatic model selection**.

The pilot used training seeds `73001`, `73002`, `73003`, 100,000 self-play hands each, learning rate `0.0005`, and 5% rake capped at 3 BB. Both initial and trained checkpoints were evaluated with seeds `990071` and `990073`, using 12,000 hands per rival and eight rivals. The following are descriptive mean bb/100 values across the three model seeds and two evaluation seeds; individual cluster intervals and full records are in [NEURAL_V2.md](NEURAL_V2.md).

| Rival | Initial V2 mean | Trained V2 mean |
|---|---:|---:|
| Uniform legal | −97.18 | +565.91 |
| Calling station | −116.81 | −201.16 |
| Nit | −87.87 | −96.46 |
| Tight-aggressive | −594.53 | −787.03 |
| Loose-aggressive | −1,362.87 | −1,865.07 |
| Historical neural V1 100M | −480.81 | −714.91 |
| Tabular | −943.67 | −959.87 |
| PHH imitation | +11.22 | −23.57 |

The improvement is concentrated in the uniform matchup. The pilot provides no basis to select a trained V2 champion or claim stronger general play. The implementation tests validate behavior and compatibility; they do not validate strategy quality. No new 100M training was launched.

The [permanent model directory](audit/2026-11-19/neural-v2/models) contains `neural-v2-{73001,73002,73003}-{initial,trained}.bin` and `neural-v1-100m.bin`. The V1 file retains SHA-256 `a712525d815872286eb960f330b5f585e4218fef627f7276b2e2e1adb08a8ac4`. Its compatibility comparison reproduced **+47.663 bb/100 against the tabular reference** and **+41.943 against PHH** under the corrected engine, 5% rake and 3 BB cap; these are V1 results, separate from the V2 pilot and from the pre-audit figures below. See [NEURAL_V2.md](NEURAL_V2.md) for the exact protocol.

## Current V2 limits and next work

1. Training/inference use CPU currently. No AMD ROCm/Vulkan GPU acceleration has been implemented yet.
2. The current actor has 4,326 weights and biases. The pilot does not justify increasing the training budget or treating the most recent checkpoint as a stronger policy.
3. Opponent beliefs are still **3-class heuristic estimates**, not full 169-class or 1,326-combination weighted ranges.
4. The policy observes summaries of betting pressure, not the full ordered action history.
5. Independent ranking/settlement checks and betting regressions now pass; full external NLHE conformance and competitive poker strength remain unproved. See [AUDIT_SIXMAX.md](AUDIT_SIXMAX.md).
6. The PHH imitator is a frozen coarse approximation of historical Pluribus actions; it is not Pluribus. Optional neural distillation uses the tabular agent as its teacher and does not directly consume PHH observations.

**Next work:** investigate the pilot's regressions with matched controls and multiple training seeds before another large campaign. Stronger external references, full action histories, card-blocked ranges and a neural critic remain proposals in [ROADMAP.md](ROADMAP.md). No exploitability or GTO claim follows from this league.

## Historical V1 first results — pre-audit, no rake

The remaining experiment tables describe the 64→32→6 V1 model and the engine used before the accounting audit. They are retained as historical evidence and do not describe V2.

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

## Historical V1 one-million-hand distillation — pre-audit, no rake

[GitHub Actions 44562688853](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44562688853) restored the stronger frozen 1M-hand scratch-trained tabular teacher and ran one million fresh **supervised** teacher rollouts. The 44 tests passed.

| Neural checkpoint | Against five tabular teacher copies | Against five PHH imitation bots |
|---|---:|---:|
| After 250k teacher deals | +19.167 ±35.532 bb/100 (60k hands) | +125.942 ±20.737 bb/100 (60k hands) |
| After 1M teacher deals | +14.397 ±25.754 bb/100 (120k hands) | +129.908 ±15.269 bb/100 (120k hands) |

The neural net used about 9.3M gradient updates across the million deals. There is **no statistically conclusive advantage over the teacher**; the point estimates remain near parity. Results versus PHH only measure superiority against a coarse six-player *imitation* of Pluribus, not against Pluribus itself. Both evaluations use no rake, simplified action sizing and the same six-max engine.

These are CPU-based measurements from GitHub Actions runners. No RX 6800 XT benchmark has been performed.

## Experimento histórico V1: 100 millones de manos — preauditoría, sin rake

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
- [Receta original archivada](docs/history/workflows/neural-100m-v1.yml), fuera de los workflows actuales de entrenamiento.
- El experimento se ejecutó en el commit `0bca3c95a31149d8cb246dd89afd92cc5ffc8170`. Los resultados posteriores deben declarar su propio commit.
- El modelo final también se conserva en [neural-v1-100m.bin](audit/2026-11-19/neural-v2/models/neural-v1-100m.bin), con el mismo SHA-256. Los artefactos originales de Actions tenían retención de 30 días.

El informe técnico general está en [PROJECT_STATUS.md](PROJECT_STATUS.md); las correcciones y resultados actuales están en [NEURAL_V2.md](NEURAL_V2.md). [ROADMAP.md](ROADMAP.md) distingue el trabajo realizado de las mejoras pendientes. Una reproducción del experimento original requiere su commit, configuración y archivos originales; ejecutar la rama actual constituye otro experimento.

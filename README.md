# SolverPK

Deterministic **Texas Hold'em heads-up simulator** with two independently acting bots, written in Rust without third-party dependencies.

## Run

```bash
cargo run --release -- --hands 10000 --samples 64 --seed 42
cargo run --release -- --hands 5 --samples 128 --verbose
cargo test
```

- Two agents: Bot0 tight/aggressive, Bot1 looser with occasional bluffs.
- 52-card shuffled deck, alternating blinds, 100 BB stacks, flop/turn/river, heads-up betting, showdown.
- Monte Carlo hand strength estimates against uniformly sampled opponent holdings.
- Exact 7-card showdown ranking from all 21 five-card combinations.
- Reproducible results with separate shuffle and decision random streams.
- Folds, net chip results, bb/100, action counts, and throughput.
- Cash-style hands independently reset stacks; no rake.
- Suitable for local simulation and research, **not** integration with real-money poker clients.

## Scope and limitations

**This is a baseline simulator, not a GTO solver.** The bots use sampled equity versus a random opponent holding and fixed betting rules, without opponent range inference, counterfactual regret minimization, or a GPU backend. These are the next architectural components.

The evaluator uses small temporary allocations to prioritize correctness and inspectability over performance. Next optimization steps are allocation-free ranking, batching evaluations, and GPU kernels. Strategy evaluation should use mirrored deals and confidence intervals; bb/100 from a short run is noisy.

## Next milestones

1. Validate betting state transitions, edge-case hand rankings and tournament/stack accounting against independent fixtures.
2. Replace the baseline agents with pluggable strategy interfaces and add an exact river CFR+ reference implementation.
3. Introduce a GPU backend for batched range equity/evaluation, benchmarked against CPU.
4. Add exploitability assessment, variance reduction, mirror matches and automated CI.

License: MIT.

## Self-play: agents that learn while playing

Run a league of 8 learning agents across 64 **logical** rooms, with all hands played as full heads-up Texas Hold'em deals:

```bash
cargo run --release -- --self-play --hands 10000 --rooms 64 --agents 8 --samples 8 --checkpoint selfplay.bin
```

Continue learning from saved state:

```bash
cargo run --release -- --self-play --resume --hands 10000 --rooms 64 --agents 8 --samples 8 --checkpoint selfplay.bin
```

Options: `--hands`, `--rooms`, `--agents` (2..256), `--samples` (Monte Carlo samples per action), `--seed`, `--learning-rate`, `--checkpoint`, `--resume`, `--verbose`.

### What actually learns

Agents use **tabular softmax actor-critic** (640 information-state buckets, three abstract actions). After each hand their chosen action probabilities are updated using the final chip reward and a per-bucket baseline. The models are tiny and checkpoint to a compact versioned binary format. Agents only observe their own hole cards via sampled equity, publicly visible board, pot, position and price to call. No hidden opponent cards are passed to the policy.

This is an operational **learning baseline**, not a neural-network model, a GTO solver, or proof of strong poker. The reward is a noisy whole-hand signal, the feature abstraction omits detailed betting histories and opponent ranges, and self-play alone can cycle or overfit to its own population. Validate progress using frozen opponent policies and held-out paired deals before interpreting bb/100 as strength.

**Important:** `--rooms` interleaves independent logical tables in one process; it does not yet parallelize execution across CPU cores. All learning currently runs on CPU; GPU training can follow after the simulator and benchmarks have been verified. The present self-play arena remains heads-up; 6-max/9-max will require a separate, validated multi-player betting engine with side pots.

No third-party runtime or ML crates are required.

## Measuring actual learning

The self-play command also evaluates agent 0 **before and after training** against a frozen copy of the initial policy. It runs these evaluations without policy updates and reports estimated bb/100 with approximate 95% uncertainty bands. Use `--eval-hands 10000` for more evaluation hands (or `--eval-hands 0` to skip). This is an initial regression indicator, not a rigorous proof of generalization or GTO quality; trajectories are not fully paired because players sample different randomized decisions. Evaluation against multiple held-out opponents is still required.

## Six-player no-limit arena

An independent, runnable 6-max simulator is available in `src/sixmax.rs`. It supports full-hand settlement with side pots and short all-in reopening checks, six abstract decision types (fold, call/check, bet/raise 33%, 75%, pot and all-in), rotating button and blinds, and independent public-action range beliefs. It is not a GTO solver or a neural network; the opponent range beliefs are coarse heuristics.

```bash
cargo run --release -- --sixmax-train --hands 1000000 --seed 42 --checkpoint sixmax.bin
cargo run --release -- --sixmax-eval --hands 120000 --seed 99 --checkpoint sixmax.bin --opponent random
```

To warm-start from an imported PHH imitation policy and compare two frozen generations, see [SIXMAX.md](SIXMAX.md). The six-max workflows evaluate against a frozen historical Pluribus **imitation**, not the actual Pluribus agent. Historical PHH replay and the earlier Heads-Up training use different model formats and cannot be mixed directly.

The 1M-hand six-max ablation confirmed the current projected PHH policy does **not** improve the trained agent (direct matched-table scratch +46.604 bb/100 versus PHH warm-start, approximate 95% half-width 5.598 bb/100). These are simulation metrics, not results versus human opponents.

## Actual neural policy (new)

`src/neural.rs` provides a **real MLP with backpropagation**, not the previous tabular policy. The six-max game engine can train it on full-hand rewards (`--sixmax-neural-train`) or distill a trained tabular teacher (`--sixmax-neural-distill`) and evaluate it in frozen full six-player matches (`--sixmax-neural-eval`). See [NEURAL_SIXMAX.md](NEURAL_SIXMAX.md).

The initial small neural architecture is a CPU Rust prototype, not yet ROCm/GPU-accelerated. Full 1,326-combination opponent ranges and full ordered betting history remain to be implemented. Neural versus tabular performance is not yet conclusively better.

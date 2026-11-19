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

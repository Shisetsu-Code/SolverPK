# Board-aware bankroll benchmark Implementation Plan

> **For agentic workers:** Use the assigned parallel modules below; root integrates and independently verifies the final branch. Steps use checkboxes to track execution.

**Goal:** Compare frozen 10M/20M tabular models against public-card opponents under the existing USD 50 bankroll and USD 10 wager cap.

**Architecture:** Add a View-only equity opponent, generalize the existing frozen league runner, and reuse bankroll accounting with an explicit actual buy-in. Persist exact session rows; a standard-library Python runner validates model identity/streams and calculates paired session bootstrap summaries.

**Tech Stack:** Rust standard library; Python standard library for experiment orchestration; GitHub Actions for compilation and execution.

**Spec:** `docs/superpowers/specs/2026-11-20-board-aware-benchmark-design.md`.

## Global Constraints

- Fixed model bytes; no training or new Rust dependencies.
- Bankroll USD 50, stop USD 50, maximum USD 10 total per hand; BB USD 0.10.
- 64 sessions, horizon 2,000, master seed 2026112007, 96 equity samples.
- Stack/rake conditions and 20 combinations exactly as specified.
- Report net gain per 100 actual hands; retain all stopped sessions.

## Review Focus

- Hidden cards or a deck seed reaching the Monte Carlo estimator: privacy tests in task 1.
- Equal board hands receiving full equity instead of tie fractions: multiway tie fixture in task 1.
- Two sessions receiving the same raw deal through nested XOR schedules: global-index and alias tests in task 2.
- Reserve remaining risk using the cap instead of actual buy-in: 50 BB financial boundary in task 2.
- Averaging session win rates or deleting zero-length sessions: denominator and paired bootstrap tests in task 3.

### Task 1: Public-information equity opponent

**Files:** create `src/sixmax_equity.rs`; root wires module in `src/sixmax.rs`.

**Interface:** `EquityPolicy::new(samples: usize) -> Self`; `probabilities(&self, view: View) -> [f32; ACTIONS]`. Consumes only player-visible information.

- [x] Write privacy, card exclusion, board sensitivity, multiway tie and legal-mask tests; observe failure against a minimal placeholder in CI.
- [x] Implement deterministic sampling without replacement with a separate public-information RNG.
- [x] Verify the focused tests and review the independence boundary.

### Task 2: Shared frozen play and risk-session CLI

**Files:** modify `src/sixmax_league.rs`, `src/sixmax_bankroll.rs`, `src/sixmax.rs`, `src/main.rs`; create `src/sixmax_risk_benchmark.rs`, `tests/risk_benchmark.rs`.

**Interface:** `--sixmax-risk-benchmark --candidate-kind tabular|neural --checkpoint PATH --opponent equity_multiway|tight_aggressive|loose_aggressive|phh|clone --sessions N --hands H --seed S --stack-bb 50|100 --equity-samples N` plus existing risk/rake flags and repeated `--exclude-stream SEED:START:COUNT`.

- [x] Write CLI regressions for missing capability, deterministic frozen output, actual-stack exposure, zero-hand sessions and stream overlap; observe baseline failures.
- [x] Generalize frozen runner with candidate/rival policy callbacks and retain original league behavior/RNG order.
- [x] Reuse Session with actual buy-in; preserve existing bankroll CLI defaults.
- [x] Allocate raw deals by `master XOR (session * horizon + hand) * stride`; validate exclusions before opening a session.
- [x] Emit `SIXMAX_RISK_SESSION` rows and `SIXMAX_RISK_SUMMARY`, including exact monetary units and separate terminal fields.
- [x] Run all Rust tests and the existing accounting/evaluator gates in CI.

### Task 3: Reproducible experiment and aggregation

**Files:** create `scripts/board_benchmark.py`, `scripts/board_benchmark_tests.py`, `.github/workflows/board-aware-benchmark.yml`, fixed protocol/manifest under `audit/2026-11-20/board-aware/`.

**Inputs:** exact session rows, pinned checkpoint SHA-256, source SHA, known excluded seed intervals. Root and the statistical implementer agree on row fields before coding.

- [x] Test unequal denominators, retained zero-hand sessions, paired identical results, distinct candidate denominators and strict JSON.
- [x] Implement model hash checks before/after, full session retention and whole-session paired bootstrap (5,000, seed 2026112011).
- [x] Run a small smoke on the release binary to measure runtime, then the fixed 20-case experiment if validation passes.
- [x] Save all raw rows, commands, hashes, protocol and summary without dropping unfavorable cases.

### Task 4: Review, documentation and integration

**Files:** add `BOARD_AWARE_BENCHMARK.md`; update README, PROJECT_STATUS, ROADMAP and CHANGELOG.

- [x] Independently review information boundaries, monetary controls, stream scheduling and statistics.
- [x] Verify exact tested source and checkpoint identities; report all results as conditional on this protocol.
- [x] Publish evidence and integrate the verified branch; confirm integration checks.

## Execution evidence

- RED: run `44706186593`, seven equity assertions and eight missing-CLI failures.
- GREEN: run `44707518790`, 177 ordinary Rust tests, 21 Python tests, accounting stress, exhaustive evaluator and all 20 smoke cases.
- Full: run `44708043940`, source `85d7fd05f46d1841a1bd2fb41351785c4ad4ef8f`, all gates passed, 1,280 retained sessions / 661,289 actual hands, original model hashes unchanged.
- Independent review approved Rust boundaries and statistical protocol; independent result recalculation matched all 20 rates and ten paired point estimates.
- Full report and raw evidence: `BOARD_AWARE_BENCHMARK.md` and `audit/2026-11-20/board-aware/`. No training or model promotion.
- Integration: PR #4 merged as `7126c07178577c1719a24827c0b363584e3f4e4b`; its tree exactly matched the reviewed branch. Main runs `44710111148` (accounting), `44710111157` (board benchmark) and `44710111203` (PokerKit/Treys comparison) all passed. See `audit/2026-11-20/board-aware/integration.json`.

# PHH integration — multiway replay and imitation baseline

Source: https://github.com/uoftcprg/phh-dataset

The existing SolverPK self-play arena remains **Heads-Up only**. This component is a separate
**2–10-player historical replay/validation engine**, not yet a playable 6-max self-play engine.

## Commands

```bash
cargo run --release -- --import-phh --source path/to/phh-dataset/data/pluribus --output observations.bin
cargo run --release -- --train-phh --source observations.bin --output imitation.bin
```

`--max-files N` limits the import for smoke tests. Files are sorted by pathname to keep hand IDs deterministic.

## Replay and validation

The Rust stdlib-only importer:
- Parses PHH no-limit Texas Hold'em (`variant = 'NT'`), including multiline action arrays.
- Supports 2–10 seats, forced blinds and antes, check/call, fold, bet/raise, street transitions, all-in callers, and side-pot construction in final settlement.
- Treats PHH `cbr amount` as **total contribution on the street** (raise-to), not the additional payment. This distinction was caught by a real Pluribus hand (#19).
- Maintains remaining stacks, contributed chips, current wager, minimum raise, folded seats, legal action mask, known community cards and actor's private cards.
- Rejects malformed cards, duplicate cards, wrong turn order, illegal wagers and contradictory finishing stacks when the outcome can be determined.
- Writes validated hands transactionally (failed hands contribute no records).
- Uses **only the acting player's private cards** in each input snapshot, never an opponent's hole cards. Future community cards remain unknown until revealed.

Incomplete/showdown hands without the necessary revealed information can only receive partial financial validation; these should be separately flagged before becoming trusted examples.

## Compact observation format

Output file: 8-byte ASCII `SPKOBS02` followed by 96-byte little-endian records.
Each record captures **the state before the recorded decision**, and the historical action label.

| Byte range | Contents |
|---|---|
| 0–7 | Input file ID (`u64`) |
| 8–9 | Decision ordinal (`u16`) |
| 10 | Actor seat index |
| 11 | Street (0 preflop … 3 river) |
| 12 | Historical action: 0 fold, 1 check/call, 2 bet/raise |
| 13 | Number of seats |
| 14 | Button index |
| 15 | Legal action bitmask |
| 16–17 | Actor's hole cards (0–51, or 255 unknown) |
| 18–22 | Community cards (255 for unrevealed) |
| 24–31 | Pot before action (`i64`, hundredths) |
| 32–39 | To-call amount (`i64`, hundredths) |
| 40–47 | Actor stack (`i64`, hundredths) |
| 48–55 | Minimum full raise increment (`i64`, hundredths) |
| 56–63 | Recorded raise-to amount (`i64`, hundredths) |
| 64–71 | Actor's committed street wager (`i64`, hundredths) |
| 72–79 | Number of non-folded players (`i64`) |
| 80–87 | Maximum opponent live stack (`i64`, hundredths) |

Remaining bytes reserved for future schema updates. File and model formats are versioned and intentionally separate from the earlier 16-byte `SPKPHH01` action index. Do not mix files.

## Initial supervised imitation

The `--train-phh` command makes two sequential streaming passes with bounded memory:
1. Build a tabular conditional action distribution from the training fold, grouped by basic public/own-hand features.
2. Compute the held-out negative log-likelihood and action accuracy on entire **unseen hands** (`hand_id % 5 == 0`).

Writes a separate `SPKIMT01` file holding action counts. This is a deliberately simple proof of the end-to-end training pipeline, **not** the existing Heads-Up agent, a neural network, or a GTO solver. There is no evidence yet that it wins more poker hands.

The small bucket schema includes street, own-hole strength grouping, suitedness, pot odds, active-player count and position. More sophisticated policies need richer range/history encodings and continuous bet sizing.

## Validation and limitations

Real PHH fixtures and the supervised baseline are exercised by:
https://github.com/Shisetsu-Code/SolverPK/actions/workflows/phh-import.yml

Known limits:
- Directory enumeration holds a list of paths, though individual PHH files and training records are consumed sequentially. A full 21-million-file import needs sharding and checkpointed progress.
- The current replay implementation has not passed an exhaustive independent poker-engine conformance suite; some rare all-in/minimum-bet and side-pot cases may require refinement.
- Some PHH source histories do not reveal full hands and thus cannot receive verified showdown settlement.
- Historical actions are demonstrations, not ground-truth optimum policies.
- The learner does not use hidden rival cards, but additional safeguards are necessary for train/evaluation splits across duplicate historical hands and sequences.

**Next engineering milestone:** extend replay validation to representative HandHQ files and larger samples, then implement a true multiway playable arena using the validated state transitions. Only after that should supervised demonstrations be incorporated into self-play.

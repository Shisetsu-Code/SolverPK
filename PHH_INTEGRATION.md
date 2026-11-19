# PHH dataset integration (stage 1)

Source: https://github.com/uoftcprg/phh-dataset

## Confirmed working

`src/phh.rs` implements a Rust-stdlib-only importer that reads PHH files, parses quoted action arrays (including multiline arrays), checks player counts, catches duplicate cards in dealt hole cards / board, and encodes each betting decision as a 16-byte record.

Run:

```bash
cargo run --release -- --import-phh --source path/to/phh-dataset/data/pluribus --output decisions.bin --max-files 1000
```

The first eight bytes of the output file are ASCII `SPKPHH01`.
Subsequent records are each **16 bytes**:

| Offset | Type | Meaning |
|---|---|---|
| 0..7 | u64 little endian | Sequential source file ID |
| 8..9 | u16 little endian | Decision index in the file |
| 10 | u8 | Player index (0-based) |
| 11 | u8 | Street 0 preflop, 1 flop, 2 turn, 3 river |
| 12 | u8 | Action 0 fold, 1 check/call, 2 completion/bet/raise |
| 13..15 | u24 little endian | Nominal raise/bet amount (not pot-relative) |

This is **an action index, not yet a supervised training set**: it does not encode hole cards, legal action masks, opponent ranges, betting state, blinds, stack depths or pre-decision pot size. It is not suitable for training a decision policy without further replay and feature extraction.

## GitHub Actions validation

[PHH importer workflow](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44537592655) compiled the executable, passed 21 tests, downloaded two public Pluribus PHH fixtures and imported both successfully:

- 2 files accepted, 0 rejected
- 26 decisions, 12 hole-card deal events
- 9 folds, 13 check/calls, 4 bet/raises
- 424 output bytes = 8 byte header + 26 × 16-byte records

## Limitations and next steps

- The currently supported data sources are individual `.phh` files; other archive layouts or compressed inputs need a streaming decoder.
- 6-player Pluribus data cannot be faithfully replayed using the current Heads-Up-only SolverPK game engine. A validated multiway engine with side pots and proper betting state is required.
- For HandHQ, large-directory enumeration and per-file parsing should be replaced by bounded streaming/chunk processing and resumable progress.
- The action schema cannot yet represent lossless fractional amounts, bet semantics, or pot-relative bet sizing.
- Rejected files should be transactional: validate a complete hand and only then append records. The current importer may write records before encountering a late error.
- Dealt opponent hole cards are for offline validation only. The model must never receive hidden opposing cards as input.
- Do not automatically treat recorded actions as optimal poker strategy; use them for imitation baselines or opponent behavior modeling, then evaluate against frozen policies.

The PHH import code has not started training a neural or tabular model on historical actions yet.

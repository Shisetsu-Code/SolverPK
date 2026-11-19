use super::*;

const STRESS_SEED: u64 = 0xd1ce_6a11_2026_1119;
const STRESS_HANDS: usize = 100_000;

// Separate from the simulator's RNG: deterministic, uniform selection among
// the masked action labels, including labels that happen to share a bet size.
struct StressRng(u64);
impl StressRng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }

    fn range(&mut self, count: usize) -> usize {
        assert!(count > 0);
        let bound = count as u64;
        let limit = u64::MAX - u64::MAX % bound;
        loop {
            let value = self.next();
            if value < limit {
                return (value % bound) as usize;
            }
        }
    }
}

struct Reference {
    settlement: Settlement,
    pots: usize,
    tied_pots: usize,
}

// Independent of accounting::settle and its sorted-funder tiers: first remove
// the single unmatched overbet, then cut pots only at LIVE contribution caps.
// Folded money enters through capped sums, never through new pot boundaries.
fn reference_settlement(table: &Table6, config: RakeConfig) -> Reference {
    let mut matched = table.total;
    let largest = (0..N).max_by_key(|&p| matched[p]).unwrap();
    let second = (0..N).filter(|&p| p != largest).map(|p| matched[p]).max().unwrap();
    let mut refunds = [0; N];
    refunds[largest] = (matched[largest] - second) * MONEY_SCALE;
    matched[largest] = second;

    let mut cutoffs: Vec<i64> = (0..N)
        .filter(|&p| !table.folded[p] && matched[p] > 0)
        .map(|p| matched[p])
        .collect();
    cutoffs.sort_unstable();
    cutoffs.dedup();
    let mut lower = 0;
    let mut pots: Vec<(i64, Vec<usize>)> = Vec::new();
    for upper in cutoffs {
        let amount = matched.iter()
            .map(|&contribution| contribution.min(upper) - contribution.min(lower))
            .sum::<i64>() * MONEY_SCALE;
        let eligible = (0..N)
            .filter(|&p| !table.folded[p] && matched[p] >= upper)
            .collect();
        pots.push((amount, eligible));
        lower = upper;
    }
    let contested = matched.iter().sum::<i64>() * MONEY_SCALE;
    assert_eq!(pots.iter().map(|pot| pot.0).sum::<i64>(), contested);
    let rake = if config.no_flop_no_drop && table.visible < 3 {
        0
    } else {
        ((i128::from(contested) * i128::from(config.basis_points) / 10_000) as i64)
            .min(config.cap_units)
    };

    // Rank each exact fractional remainder instead of using production's
    // iterative largest-remainder selection. Equal remainders favor main pots.
    let mut owed = vec![0; pots.len()];
    if rake > 0 {
        let fractions: Vec<i128> = pots.iter()
            .map(|pot| i128::from(rake) * i128::from(pot.0))
            .collect();
        for index in 0..pots.len() {
            owed[index] = (fractions[index] / i128::from(contested)) as i64;
        }
        let extra = (rake - owed.iter().sum::<i64>()) as usize;
        for index in 0..pots.len() {
            let remainder = fractions[index] % i128::from(contested);
            let earlier = (0..pots.len()).filter(|&other| {
                let theirs = fractions[other] % i128::from(contested);
                theirs > remainder || (theirs == remainder && other < index)
            }).count();
            owed[index] += i64::from(earlier < extra);
        }
    }
    // Hand ranking itself is covered by the independent five-card evaluator
    // oracle and exhaustive audit; this oracle isolates financial settlement.
    let ranks: [u64; N] = std::array::from_fn(|p| score7(&[
        table.hole[p][0], table.hole[p][1], table.board[0], table.board[1],
        table.board[2], table.board[3], table.board[4],
    ]));
    let mut payouts = refunds;
    let mut tied_pots = 0;
    for (index, (amount, eligible)) in pots.iter().enumerate() {
        let best = eligible.iter().map(|&p| ranks[p]).max().unwrap();
        let winners: Vec<usize> = (1..=N).map(|step| (table.button + step) % N)
            .filter(|p| eligible.contains(p) && ranks[*p] == best)
            .collect();
        tied_pots += usize::from(winners.len() > 1);
        let distributable = amount - owed[index];
        for (priority, &winner) in winners.iter().enumerate() {
            payouts[winner] += distributable / winners.len() as i64
                + i64::from((priority as i64) < distributable % winners.len() as i64);
        }
    }
    Reference {
        settlement: Settlement {
            payouts,
            net: std::array::from_fn(|p| payouts[p] - table.total[p] * MONEY_SCALE),
            refunds,
            rake,
        },
        pots: pots.len(),
        tied_pots,
    }
}

fn assert_table_ledger(table: &Table6, initial: [i64; N], hand: usize) {
    assert_eq!(table.pot, table.total.iter().sum::<i64>(), "hand={hand}");
    assert_eq!(table.pot + table.stack.iter().sum::<i64>(), initial.iter().sum::<i64>(), "hand={hand}");
    assert_eq!(table.high, *table.street.iter().max().unwrap(), "hand={hand}");
    for p in 0..N {
        assert!(table.stack[p] >= 0 && table.street[p] >= 0, "hand={hand}, seat={p}");
        assert!(table.street[p] <= table.total[p], "hand={hand}, seat={p}");
        assert_eq!(table.stack[p] + table.total[p], initial[p], "hand={hand}, seat={p}");
    }
}

fn assert_cash_ledger(table: &Table6, result: Settlement, hand: usize) {
    assert_eq!(result.payouts.iter().sum::<i64>() + result.rake, table.pot * MONEY_SCALE, "hand={hand}");
    assert_eq!(result.net.iter().sum::<i64>(), -result.rake, "hand={hand}");
    assert!(result.rake >= 0, "hand={hand}");
    for p in 0..N {
        assert!(result.payouts[p] >= result.refunds[p] && result.refunds[p] >= 0, "hand={hand}, seat={p}");
        assert_eq!(result.net[p], result.payouts[p] - table.total[p] * MONEY_SCALE, "hand={hand}, seat={p}");
        if table.folded[p] {
            assert_eq!(result.payouts[p], result.refunds[p], "folded winner: hand={hand}, seat={p}");
        }
    }
}

#[test]
#[ignore = "100,000 complete legal histories; cargo test --release audit_stress -- --ignored --nocapture"]
fn audit_stress_100000_legal_hands_against_independent_settlement() {
    let configs = [
        RakeConfig { basis_points: 500, cap_units: 3 * BB_UNITS, no_flop_no_drop: true },
        RakeConfig { basis_points: 500, cap_units: 3 * BB_UNITS, no_flop_no_drop: false },
        RakeConfig { basis_points: 10_000, cap_units: 100 * BB_UNITS, no_flop_no_drop: false },
        RakeConfig { basis_points: 500, cap_units: 1, no_flop_no_drop: false },
        RakeConfig { basis_points: 1, cap_units: 100 * BB_UNITS, no_flop_no_drop: true },
    ];
    let mut positions = [[0usize; N]; 2];
    let mut actions = 0usize;
    let mut showdowns = 0usize;
    let mut sidepot_hands = 0usize;
    let mut split_pot_hands = 0usize;
    let mut refunded_hands = 0usize;
    let mut raked_hands = 0usize;
    let mut uncalled_checks = 0usize;
    for hand in 0..STRESS_HANDS {
        let seed = STRESS_SEED ^ (hand as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let mut rng = StressRng(seed ^ 0x5be0_cd19_137e_2179);
        let kind = hand % 2;
        let button = (hand / 2) % N;
        positions[kind][button] += 1;
        let initial = std::array::from_fn(|_| if kind == 0 { STACK } else { 2 + rng.range(199) as i64 });
        let mut table = Table6::new(seed, button);
        table.stack = std::array::from_fn(|p| initial[p] - table.total[p]);
        assert_table_ledger(&table, initial, hand);
        let mut steps = 0;
        loop {
            while !table.completed() {
                steps += 1;
                assert!(steps < 2000, "nonterminating hand={hand}, seed={seed:#x}");
                let actor = table.actor;
                assert!(table.pending[actor] && !table.folded[actor] && table.stack[actor] > 0);
                let legal = table.legal(actor);
                let choices: Vec<usize> = (0..ACTIONS).filter(|&a| legal[a]).collect();
                let chosen = choices[rng.range(choices.len())];
                let action = match chosen {
                    0 => Action::Fold,
                    1 => Action::Call,
                    _ => Action::Raise(table.raise_to(actor, chosen)),
                };
                let stacks_before = table.stack;
                let totals_before = table.total;
                let call = (table.high - table.street[actor]).min(table.stack[actor]);
                let payment = match action {
                    Action::Fold => { assert!(call > 0); 0 }
                    Action::Call => call,
                    Action::Raise(to) => {
                        assert!(!table.acted[actor] || table.high - table.street[actor] >= table.min_raise);
                        assert!((0..N).any(|p| p != actor && !table.folded[p] && table.stack[p] > 0));
                        let paid = to - table.street[actor];
                        assert!(paid > call && paid <= table.stack[actor]);
                        assert!(to - table.high >= table.min_raise || paid == table.stack[actor]);
                        paid
                    }
                };
                table.act(action).unwrap_or_else(|error| panic!("hand={hand}, seed={seed:#x}, action={action:?}: {error}"));
                for p in 0..N {
                    let expected = if p == actor { payment } else { 0 };
                    assert_eq!(table.total[p] - totals_before[p], expected, "hand={hand}, seat={p}");
                    assert_eq!(stacks_before[p] - table.stack[p], expected, "hand={hand}, seat={p}");
                }
                assert_table_ledger(&table, initial, hand);
            }
            if table.alive() == 1 || table.round == 3 {
                break;
            }
            table.next_street().unwrap();
            assert_table_ledger(&table, initial, hand);
            if table.live_bettors() < 2 {
                assert!(table.completed(), "dry sidepot: hand={hand}");
            }
        }
        actions += steps;
        assert!(table.alive() > 0);
        if table.alive() > 1 {
            showdowns += 1;
            assert_eq!(table.visible, 5, "incomplete showdown: hand={hand}");
        }

        let gross = table.settle(RakeConfig::NONE);
        let reference = reference_settlement(&table, RakeConfig::NONE);
        assert_eq!(gross, reference.settlement, "gross hand={hand}, seed={seed:#x}, total={:?}, folded={:?}", table.total, table.folded);
        assert_cash_ledger(&table, gross, hand);
        sidepot_hands += usize::from(reference.pots > 1);
        split_pot_hands += usize::from(reference.tied_pots > 0);
        refunded_hands += usize::from(gross.refunds.iter().any(|&amount| amount > 0));

        let config = configs[(hand / 2) % configs.len()];
        let actual = table.settle(config);
        let expected = reference_settlement(&table, config).settlement;
        assert_eq!(actual, expected, "raked hand={hand}, seed={seed:#x}, config={config:?}, total={:?}, folded={:?}", table.total, table.folded);
        assert_cash_ledger(&table, actual, hand);
        raked_hands += usize::from(actual.rake > 0);

        if let Some(p) = (0..N).find(|&p| actual.refunds[p] > 0) {
            // Accounting-only metamorphism after the legal history: additional
            // unmatched money must return in full, without changing anyone's EV.
            let mut extended = table.clone();
            extended.total[p] += 7;
            extended.pot += 7;
            let changed = extended.settle(config);
            assert_eq!(changed.net, actual.net, "uncalled net: hand={hand}");
            assert_eq!(changed.rake, actual.rake, "uncalled rake: hand={hand}");
            for seat in 0..N {
                let added = if seat == p { 7 * MONEY_SCALE } else { 0 };
                assert_eq!(changed.refunds[seat], actual.refunds[seat] + added, "hand={hand}, seat={seat}");
                assert_eq!(changed.payouts[seat], actual.payouts[seat] + added, "hand={hand}, seat={seat}");
            }
            uncalled_checks += 1;
        }
    }
    assert_eq!(positions[0].iter().sum::<usize>(), STRESS_HANDS / 2);
    assert_eq!(positions[1].iter().sum::<usize>(), STRESS_HANDS / 2);
    assert!(positions.iter().flatten().all(|&count| count > 0));
    assert!(showdowns > 0 && showdowns < STRESS_HANDS);
    assert!(sidepot_hands > 0 && split_pot_hands > 0 && refunded_hands > 0 && raked_hands > 0);
    println!("AUDIT_STRESS seed={STRESS_SEED:#x} hands={STRESS_HANDS} fixed={} heterogeneous={} actions={actions} showdowns={showdowns} fold_wins={} sidepot_hands={sidepot_hands} split_pot_hands={split_pot_hands} refunded_hands={refunded_hands} uncalled_checks={uncalled_checks} raked_hands={raked_hands}",
        STRESS_HANDS / 2, STRESS_HANDS / 2, STRESS_HANDS - showdowns);
}

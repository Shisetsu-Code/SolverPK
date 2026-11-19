//! Exact financial accounting for the six-seat betting abstraction.
//! Betting contributions use chips of 0.5 BB; settlement uses 10,000 units
//! per chip, so its smallest monetary unit is 0.00005 BB.
use std::io;

pub const N: usize = 6;
pub const MONEY_SCALE: i64 = 10_000;
pub const BB_UNITS: i64 = 2 * MONEY_SCALE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RakeConfig {
    pub basis_points: u16,
    pub cap_units: i64,
    pub no_flop_no_drop: bool,
}

impl RakeConfig {
    pub const NONE: Self = Self {
        basis_points: 0,
        cap_units: 0,
        no_flop_no_drop: true,
    };

    pub fn from_args(args: &[String]) -> io::Result<Self> {
        from_args(args)
    }
}

pub const NONE: RakeConfig = RakeConfig::NONE;

/// Explicitly chooses the rake model; unrelated command arguments are ignored.
pub fn from_args(args: &[String]) -> io::Result<RakeConfig> {
    let mut basis_points = None;
    let mut cap_units = None;
    let mut preflop = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--rake-bps" => {
                if basis_points.is_some() {
                    return Err(invalid("duplicate --rake-bps"));
                }
                let value = args.get(index + 1).ok_or_else(|| invalid("missing --rake-bps value"))?;
                let rate = value.parse::<u16>().map_err(|_| invalid("--rake-bps must be an integer in 0..=10000"))?;
                if rate > 10_000 {
                    return Err(invalid("--rake-bps must be in 0..=10000"));
                }
                basis_points = Some(rate);
                index += 2;
            }
            "--rake-cap-bb" => {
                if cap_units.is_some() {
                    return Err(invalid("duplicate --rake-cap-bb"));
                }
                let value = args.get(index + 1).ok_or_else(|| invalid("missing --rake-cap-bb value"))?;
                cap_units = Some(parse_cap_units(value)?);
                index += 2;
            }
            "--rake-preflop" => {
                if preflop {
                    return Err(invalid("duplicate --rake-preflop"));
                }
                preflop = true;
                index += 1;
            }
            _ => index += 1,
        }
    }
    let basis_points = basis_points.ok_or_else(|| invalid("explicit --rake-bps is required; use 0 for no rake"))?;
    if basis_points > 0 && cap_units.is_none() {
        return Err(invalid("positive --rake-bps requires --rake-cap-bb"));
    }
    Ok(RakeConfig {
        basis_points,
        cap_units: cap_units.unwrap_or(0),
        no_flop_no_drop: !preflop,
    })
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Convert a finite decimal, optionally in scientific notation, exactly.
/// Caps between accounting units are rejected rather than silently rounded.
fn parse_cap_units(value: &str) -> io::Result<i64> {
    let error = || invalid("--rake-cap-bb must be in (0,100], in multiples of 0.00005 BB");
    let value = value.strip_prefix('+').unwrap_or(value);
    let mut exponent_parts = value.split(|c| c == 'e' || c == 'E');
    let mantissa = exponent_parts.next().ok_or_else(error)?;
    let exponent = match exponent_parts.next() {
        Some(part) => part.parse::<i32>().map_err(|_| error())?,
        None => 0,
    };
    if exponent_parts.next().is_some() {
        return Err(error());
    }
    let mut decimal_parts = mantissa.split('.');
    let whole = decimal_parts.next().ok_or_else(error)?;
    let fraction = decimal_parts.next().unwrap_or("");
    if decimal_parts.next().is_some()
        || (whole.is_empty() && fraction.is_empty())
        || !whole.bytes().chain(fraction.bytes()).all(|c| c.is_ascii_digit())
    {
        return Err(error());
    }
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    let significant = digits.trim_end_matches('0');
    if significant.is_empty() {
        return Err(error());
    }
    let trailing_zeros = i64::try_from(digits.len() - significant.len()).map_err(|_| error())?;
    let decimal_places = i64::try_from(fraction.len()).map_err(|_| error())?;
    let power = i64::from(exponent) - decimal_places + trailing_zeros + 4;
    let numerator = significant.parse::<i128>().ok().and_then(|n| n.checked_mul(2)).ok_or_else(error)?;
    // BB_UNITS = 2 * 10^4; keeping the conversion integral avoids float drift.
    let units = if power >= 0 {
        if power > 6 {
            return Err(error());
        }
        numerator.checked_mul(10_i128.pow(power as u32)).ok_or_else(error)?
    } else {
        let divisor_power = u32::try_from(-power).map_err(|_| error())?;
        let divisor = 10_i128.checked_pow(divisor_power).ok_or_else(error)?;
        if numerator % divisor != 0 {
            return Err(error());
        }
        numerator / divisor
    };
    if !(1..=i128::from(100 * BB_UNITS)).contains(&units) {
        return Err(error());
    }
    i64::try_from(units).map_err(|_| error())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settlement {
    /// All cash returned to each seat, including unmatched-bet refunds.
    pub payouts: [i64; N],
    pub net: [i64; N],
    pub refunds: [i64; N],
    pub rake: i64,
}

/// Contributions are betting chips; every field in the result is MONEY_SCALE
/// units. Higher ranks win. The button controls indivisible-unit priority.
pub fn settle(
    total: [i64; N],
    folded: [bool; N],
    ranks: [u64; N],
    button: usize,
    flop_dealt: bool,
    config: RakeConfig,
) -> Settlement {
    assert!(button < N, "button outside the table");
    assert!(total.iter().all(|&amount| amount >= 0), "negative contribution");
    assert!(config.basis_points <= 10_000, "rake above 100 percent");
    assert!(config.cap_units >= 0, "negative rake cap");
    assert!(config.basis_points == 0 || config.cap_units > 0, "positive rake requires a cap");
    let gross = i64::try_from(total.iter().map(|&amount| i128::from(amount) * i128::from(MONEY_SCALE)).sum::<i128>())
        .expect("contributions exceed accounting range");
    let contributed = total.map(|amount| amount * MONEY_SCALE);
    let live = (0..N).filter(|&p| !folded[p]).fold(0_u8, |mask, p| mask | (1 << p));
    let mut levels = total;
    levels.sort_unstable();
    let mut previous = 0;
    let mut pots = [Pot::default(); N];
    let mut pot_count = 0;
    let mut refunds = [0_i64; N];
    for level in levels {
        if level <= previous {
            continue;
        }
        let funders = (0..N).filter(|&p| total[p] >= level).fold(0_u8, |mask, p| mask | (1 << p));
        let amount = i64::try_from(i128::from(level - previous) * i128::from(funders.count_ones()) * i128::from(MONEY_SCALE))
            .expect("pot exceeds accounting range");
        previous = level;
        if funders.count_ones() == 1 {
            // Refunds belong to the funder, even if a caller supplied a state
            // with a folded sole funder. They never enter the rake base.
            refunds[funders.trailing_zeros() as usize] += amount;
            continue;
        }
        let eligible = funders & live;
        assert!(eligible != 0, "contested pot has no eligible player");
        if pot_count > 0 && pots[pot_count - 1].eligible == eligible {
            // Folded contribution boundaries cannot create extra odd units.
            pots[pot_count - 1].amount += amount;
        } else {
            pots[pot_count] = Pot { amount, eligible };
            pot_count += 1;
        }
    }

    let contested = pots[..pot_count].iter().map(|pot| pot.amount).sum::<i64>();
    let rake = if config.basis_points == 0 || (config.no_flop_no_drop && !flop_dealt) {
        0
    } else {
        // Floor once at the smallest accounting unit and cap once per hand.
        let percentage = i128::from(contested) * i128::from(config.basis_points) / 10_000;
        i64::try_from(percentage).expect("rake exceeds accounting range").min(config.cap_units)
    };
    let mut deductions = [0_i64; N];
    if rake > 0 {
        let mut remainders = [0_i128; N];
        let mut remaining = rake;
        for index in 0..pot_count {
            let numerator = i128::from(rake) * i128::from(pots[index].amount);
            deductions[index] = (numerator / i128::from(contested)) as i64;
            remainders[index] = numerator % i128::from(contested);
            remaining -= deductions[index];
        }
        while remaining > 0 {
            let mut largest = 0;
            for index in 1..pot_count {
                if remainders[index] > remainders[largest] {
                    largest = index;
                }
            }
            deductions[largest] += 1;
            remainders[largest] = -1;
            remaining -= 1;
        }
    }

    let mut payouts = refunds;
    for index in 0..pot_count {
        let pot = pots[index];
        let best = (0..N).filter(|&p| pot.eligible & (1 << p) != 0).map(|p| ranks[p]).max().unwrap();
        let winners = (0..N).filter(|&p| pot.eligible & (1 << p) != 0 && ranks[p] == best)
            .fold(0_u8, |mask, p| mask | (1 << p));
        let distributable = pot.amount - deductions[index];
        let share = distributable / i64::from(winners.count_ones());
        let mut extra = distributable % i64::from(winners.count_ones());
        for distance in 1..=N {
            let player = (button + distance) % N;
            if winners & (1 << player) != 0 {
                payouts[player] += share + if extra > 0 { 1 } else { 0 };
                if extra > 0 {
                    extra -= 1;
                }
            }
        }
    }
    let net = std::array::from_fn(|p| payouts[p] - contributed[p]);
    assert!(payouts.iter().all(|&amount| amount >= 0));
    assert_eq!(payouts.iter().sum::<i64>() + rake, gross);
    assert_eq!(net.iter().sum::<i64>(), -rake);
    Settlement { payouts, net, refunds, rake }
}

#[derive(Clone, Copy, Default)]
struct Pot {
    amount: i64,
    eligible: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIVE_PERCENT_THREE_BB: RakeConfig = RakeConfig {
        basis_points: 500,
        cap_units: 60_000,
        no_flop_no_drop: true,
    };

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn cli_requires_an_explicit_rake_choice() {
        assert!(from_args(&[]).is_err());
        assert!(from_args(&args(&["--sixmax-train", "--hands", "100"])).is_err());
    }

    #[test]
    fn cli_accepts_explicit_zero_and_a_capped_positive_rate() {
        assert_eq!(from_args(&args(&["--rake-bps", "0"])).unwrap(), NONE);
        assert_eq!(
            RakeConfig::from_args(&args(&[
                "--sixmax-train", "--rake-bps", "500", "--rake-cap-bb", "3",
            ]))
            .unwrap(),
            FIVE_PERCENT_THREE_BB,
        );
        assert_eq!(
            from_args(&args(&["--rake-bps", "10000", "--rake-cap-bb", "100"])).unwrap(),
            RakeConfig { basis_points: 10_000, cap_units: 2_000_000, no_flop_no_drop: true },
        );
    }

    #[test]
    fn cli_preserves_the_smallest_accounting_unit_and_preflop_override() {
        for cap in ["0.00005", "5e-5", "+0.0000500000000000000000"] {
            assert_eq!(
                from_args(&args(&[
                    "--rake-cap-bb", cap, "--rake-bps", "500", "--rake-preflop",
                ]))
                .unwrap(),
                RakeConfig {
                    basis_points: 500,
                    cap_units: 1,
                    no_flop_no_drop: false,
                },
            );
        }
    }

    #[test]
    fn cli_rejects_missing_duplicate_and_invalid_values() {
        let invalid: &[&[&str]] = &[
            &["--rake-bps"],
            &["--rake-bps", "500"],
            &["--rake-bps", "500", "--rake-cap-bb"],
            &["--rake-bps", "--rake-cap-bb", "3"],
            &["--rake-bps", "0", "--rake-bps", "0"],
            &["--rake-bps", "500", "--rake-cap-bb", "3", "--rake-cap-bb", "3"],
            &["--rake-bps", "0", "--rake-preflop", "--rake-preflop"],
            &["--rake-bps", "-1", "--rake-cap-bb", "3"],
            &["--rake-bps", "10001", "--rake-cap-bb", "3"],
            &["--rake-bps", "5.0", "--rake-cap-bb", "3"],
            &["--rake-bps", "NaN", "--rake-cap-bb", "3"],
            &["--rake-bps", "500", "--rake-cap-bb", "0"],
            &["--rake-bps", "500", "--rake-cap-bb", "-1"],
            &["--rake-bps", "500", "--rake-cap-bb", "100.00005"],
            &["--rake-bps", "500", "--rake-cap-bb", "NaN"],
            &["--rake-bps", "500", "--rake-cap-bb", "inf"],
            &["--rake-bps", "500", "--rake-cap-bb", "1e309"],
            &["--rake-bps", "500", "--rake-cap-bb", "0.00001"],
        ];
        for input in invalid {
            assert!(from_args(&args(input)).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn a_folded_contribution_does_not_create_an_extra_split_pot() {
        // One pot of 38 chips, three tied winners. Splitting its fictitious
        // 6/10/4/18-chip contribution layers would give seat 3 two odd units.
        let result = settle(
            [1, 3, 4, 10, 10, 10],
            [true, true, true, false, false, false],
            [0, 0, 0, 9, 9, 9],
            0,
            true,
            NONE,
        );
        assert_eq!(result.payouts, [0, 0, 0, 126_667, 126_667, 126_666]);
        assert_eq!(result.refunds, [0; N]);
        assert_eq!(result.net, [-10_000, -30_000, -40_000, 26_667, 26_667, 26_666]);
        assert_eq!(result.rake, 0);
    }

    #[test]
    fn genuine_side_pots_stay_separate_even_when_their_winners_match() {
        // Seat 0 is all-in for 1 and loses. Seat 4 folded after putting in 2.
        // Main 5 chips splits 16667/16667/16666; side 7 splits 23334/23333/23333.
        let result = settle(
            [1, 3, 3, 3, 2, 0],
            [false, false, false, false, true, true],
            [1, 9, 9, 9, 0, 0],
            0,
            true,
            NONE,
        );
        assert_eq!(result.payouts, [0, 40_001, 40_000, 39_999, 0, 0]);
    }

    #[test]
    fn unmatched_chips_are_refunded_before_the_rake_base_is_computed() {
        // Main 15 chips: rake 0.75. Side 6: rake 0.30. Unmatched 2: no rake.
        let result = settle(
            [5, 10, 8, 0, 0, 0],
            [false, false, true, true, true, true],
            [20, 10, 0, 0, 0, 0],
            0,
            true,
            FIVE_PERCENT_THREE_BB,
        );
        assert_eq!(result.rake, 10_500);
        assert_eq!(result.refunds, [0, 20_000, 0, 0, 0, 0]);
        assert_eq!(result.payouts, [142_500, 77_000, 0, 0, 0, 0]);
        assert_eq!(result.net, [92_500, -23_000, -80_000, 0, 0, 0]);
    }

    #[test]
    fn the_hand_cap_is_shared_proportionally_between_main_and_side_pots() {
        // Contested 400 chips: main 300 and side 100. Cap 6 chips is allocated
        // 4.5/1.5. The last seat also receives its unmatched 50 chips in full.
        let result = settle(
            [100, 150, 200, 0, 0, 0],
            [false, false, false, true, true, true],
            [30, 20, 10, 0, 0, 0],
            0,
            true,
            FIVE_PERCENT_THREE_BB,
        );
        assert_eq!(result.rake, 60_000);
        assert_eq!(result.refunds, [0, 0, 500_000, 0, 0, 0]);
        assert_eq!(result.payouts, [2_955_000, 985_000, 500_000, 0, 0, 0]);
        assert_eq!(result.net, [1_955_000, -515_000, -1_500_000, 0, 0, 0]);
    }

    #[test]
    fn proportional_rounding_assigns_the_unit_to_the_largest_remainder() {
        // Main/side ratio 150:60. From a 60000-unit cap they owe
        // 42857.142... and 17142.857..., so the side pot gets the extra unit.
        let result = settle(
            [50, 100, 80, 0, 0, 0],
            [false, false, true, true, true, true],
            [20, 10, 0, 0, 0, 0],
            0,
            true,
            FIVE_PERCENT_THREE_BB,
        );
        assert_eq!(result.rake, 60_000);
        assert_eq!(result.refunds, [0, 200_000, 0, 0, 0, 0]);
        assert_eq!(result.payouts, [1_457_143, 782_857, 0, 0, 0, 0]);
    }

    #[test]
    fn equal_rake_remainders_prioritize_the_main_pot() {
        let result = settle(
            [2, 5, 5, 0, 0, 0],
            [false, false, false, true, true, true],
            [30, 20, 10, 0, 0, 0],
            0,
            true,
            RakeConfig { cap_units: 1, ..FIVE_PERCENT_THREE_BB },
        );
        // Both genuine pots have 60000 units; the single rake unit is main's.
        assert_eq!(result.rake, 1);
        assert_eq!(result.payouts, [59_999, 60_000, 0, 0, 0, 0]);
    }

    #[test]
    fn no_flop_no_drop_returns_the_entire_gross_amount() {
        let result = settle(
            [0, 1, 2, 10, 0, 0],
            [true, true, true, false, true, true],
            [0; N],
            0,
            false,
            FIVE_PERCENT_THREE_BB,
        );
        assert_eq!(result.rake, 0);
        assert_eq!(result.refunds, [0, 0, 0, 80_000, 0, 0]);
        assert_eq!(result.payouts, [0, 0, 0, 130_000, 0, 0]);
        assert_eq!(result.net, [0, -10_000, -20_000, 30_000, 0, 0]);
    }

    #[test]
    fn explicit_preflop_rake_and_postflop_folds_both_charge_only_matched_money() {
        for (flop, config) in [
            (false, RakeConfig { no_flop_no_drop: false, ..FIVE_PERCENT_THREE_BB }),
            (true, FIVE_PERCENT_THREE_BB),
        ] {
            let result = settle(
                [0, 1, 2, 10, 0, 0],
                [true, true, true, false, true, true],
                [0; N],
                0,
                flop,
                config,
            );
            assert_eq!(result.rake, 2_500);
            assert_eq!(result.refunds, [0, 0, 0, 80_000, 0, 0]);
            assert_eq!(result.payouts, [0, 0, 0, 127_500, 0, 0]);
            assert_eq!(result.net, [0, -10_000, -20_000, 27_500, 0, 0]);
        }
    }

    #[test]
    fn preflop_all_ins_are_raked_when_the_board_runs_out() {
        let result = settle(
            [0, 1, 2, 200, 200, 0],
            [true, true, true, false, false, true],
            [0, 0, 0, 20, 10, 0],
            0,
            true,
            FIVE_PERCENT_THREE_BB,
        );
        assert_eq!(result.rake, 60_000);
        assert_eq!(result.refunds, [0; N]);
        assert_eq!(result.payouts, [0, 0, 0, 3_970_000, 0, 0]);
    }

    #[test]
    fn the_smallest_odd_unit_follows_the_button_clockwise() {
        for (button, expected) in [
            (0, [13_333, 13_334, 0, 13_333, 0, 0]),
            (3, [13_334, 13_333, 0, 13_333, 0, 0]),
        ] {
            let result = settle(
                [1, 1, 1, 1, 0, 0],
                [false, false, false, false, true, true],
                [9, 9, 1, 9, 0, 0],
                button,
                true,
                NONE,
            );
            assert_eq!(result.payouts, expected);
        }
    }

    #[test]
    fn an_unmatched_layer_is_a_refund_even_if_its_funder_is_folded() {
        let result = settle(
            [10, 5, 0, 0, 0, 0],
            [true, false, true, true, true, true],
            [100, 1, 0, 0, 0, 0],
            0,
            true,
            NONE,
        );
        assert_eq!(result.refunds, [50_000, 0, 0, 0, 0, 0]);
        assert_eq!(result.payouts, [50_000, 100_000, 0, 0, 0, 0]);
        assert_eq!(result.net, [-50_000, 50_000, 0, 0, 0, 0]);
    }

    #[test]
    fn a_full_percentage_is_still_limited_by_the_hand_cap() {
        for (cap_units, expected_rake, expected_payout) in [
            (15_000, 15_000, 5_000),
            (60_000, 20_000, 0),
        ] {
            let result = settle(
                [1, 1, 0, 0, 0, 0],
                [false, false, true, true, true, true],
                [20, 10, 0, 0, 0, 0],
                0,
                true,
                RakeConfig { basis_points: 10_000, cap_units, no_flop_no_drop: true },
            );
            assert_eq!(result.rake, expected_rake);
            assert_eq!(result.payouts, [expected_payout, 0, 0, 0, 0, 0]);
        }
    }

    #[test]
    fn an_empty_pot_has_no_rake_or_payouts() {
        let result = settle([0; N], [false; N], [0; N], 0, true, FIVE_PERCENT_THREE_BB);
        assert_eq!(result, Settlement { payouts: [0; N], net: [0; N], refunds: [0; N], rake: 0 });
    }

    #[test]
    fn zero_ranks_tie_and_a_folded_higher_rank_is_ignored() {
        let result = settle(
            [1, 1, 1, 1, 0, 0],
            [false, false, true, false, true, true],
            [0, 0, u64::MAX, 0, 0, 0],
            0,
            true,
            NONE,
        );
        assert_eq!(result.payouts, [13_333, 13_334, 0, 13_333, 0, 0]);
    }

    #[test]
    fn increasing_an_uncalled_bet_changes_neither_rake_nor_player_net() {
        for (bet, expected_refund, expected_payout) in [
            (5, 0, 95_000),
            (50, 450_000, 545_000),
        ] {
            let result = settle(
                [bet, 5, 0, 0, 0, 0],
                [false, false, true, true, true, true],
                [20, 10, 0, 0, 0, 0],
                0,
                true,
                FIVE_PERCENT_THREE_BB,
            );
            assert_eq!(result.rake, 5_000);
            assert_eq!(result.refunds, [expected_refund, 0, 0, 0, 0, 0]);
            assert_eq!(result.payouts, [expected_payout, 0, 0, 0, 0, 0]);
            assert_eq!(result.net, [45_000, -50_000, 0, 0, 0, 0]);
        }
    }
}

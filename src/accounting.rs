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
pub fn from_args(_args: &[String]) -> io::Result<RakeConfig> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "rake configuration not implemented",
    ))
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
    _total: [i64; N],
    _folded: [bool; N],
    _ranks: [u64; N],
    _button: usize,
    _flop_dealt: bool,
    _config: RakeConfig,
) -> Settlement {
    Settlement {
        payouts: [0; N],
        net: [0; N],
        refunds: [0; N],
        rake: 0,
    }
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
    }

    #[test]
    fn cli_preserves_the_smallest_accounting_unit_and_preflop_override() {
        assert_eq!(
            from_args(&args(&[
                "--rake-cap-bb", "0.00005", "--rake-bps", "500", "--rake-preflop",
            ]))
            .unwrap(),
            RakeConfig {
                basis_points: 500,
                cap_units: 1,
                no_flop_no_drop: false,
            },
        );
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
}

//! Independent, public-information Monte Carlo reference for the frozen arena.
//! The betting rule is a fixed diagnostic heuristic, not a GTO policy.
use super::{Rng, View, ACTIONS, N};

pub(super) struct EquityPolicy {
    samples: usize,
}

impl EquityPolicy {
    pub(super) fn new(samples: usize) -> Self {
        Self { samples }
    }

    pub(super) fn probabilities(&self, _view: View) -> [f32; ACTIONS] {
        [0.0; ACTIONS]
    }

    fn equity(&self, _view: View) -> f64 {
        let _ = self.samples;
        0.0
    }
}

struct UnknownCards {
    cards: [u8; 52],
    len: usize,
}

impl UnknownCards {
    fn new(_view: View) -> Self {
        Self { cards: [0; 52], len: 52 }
    }

    fn draw(&mut self, count: usize, _rng: &mut Rng) -> &[u8] {
        &self.cards[..count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Table6;

    fn fixture() -> View {
        let table = Table6::new(2026112007, 0);
        let mut view = table.view(table.actor);
        view.hole = [0, 14]; // 2c 3d.
        view.board = [26, 40, 8, 22, 37]; // 2h 3s Tc Jd Kh.
        view.visible = 5;
        view.street = 3;
        view.active = N;
        view.pot = 80;
        view.call = 10;
        view.stack = 100;
        view.legal = [true; ACTIONS];
        view
    }

    #[test]
    fn rejects_a_zero_simulation_budget() {
        assert!(std::panic::catch_unwind(|| EquityPolicy::new(0)).is_err());
    }

    #[test]
    fn board_only_royal_flush_shares_equity_between_all_active_players() {
        let policy = EquityPolicy::new(96);
        let mut view = fixture();
        view.board = [34, 35, 36, 37, 38]; // Th Jh Qh Kh Ah: everyone ties.
        for (active, expected) in [(2, 0.5), (3, 1.0 / 3.0), (4, 0.25),
            (5, 0.2), (6, 1.0 / 6.0)] {
            view.active = active;
            assert!((policy.equity(view) - expected).abs() < 1e-12,
                "a shared board must award only 1/{active} of the pot");
        }
    }

    #[test]
    fn visible_board_changes_equity_and_the_betting_distribution() {
        let policy = EquityPolicy::new(256);
        let mut nuts = fixture();
        nuts.hole = [38, 37]; // Ah Kh.
        nuts.board = [36, 35, 34, 0, 14]; // Qh Jh Th 2c 3d: exclusive royal.
        let mut missed = nuts;
        missed.board = [0, 17, 32, 46, 9]; // 2c 6d 8h 9s Jc: ace high.
        assert_eq!(policy.equity(nuts), 1.0);
        assert!(policy.equity(missed) < 0.5,
            "ace high against five unknown hands cannot look like the nuts");
        let strong = policy.probabilities(nuts);
        let weak = policy.probabilities(missed);
        assert!(strong[2..].iter().sum::<f32>() > weak[2..].iter().sum::<f32>(),
            "the board must influence actual actions, not only a diagnostic score");
    }

    #[test]
    fn hero_suit_changes_equity_when_the_board_makes_a_flush() {
        let policy = EquityPolicy::new(256);
        let mut hearts = fixture();
        hearts.visible = 3;
        hearts.street = 1;
        hearts.hole = [38, 31]; // Ah 7h.
        hearts.board = [26, 29, 35, 255, 255]; // 2h 5h Jh.
        let mut clubs = hearts;
        clubs.hole = [12, 5]; // Ac 7c, same ranks without a heart.
        assert!(policy.equity(hearts) > policy.equity(clubs) + 0.4,
            "a made ace-high flush must be distinguished from unpaired clubs");
    }

    #[test]
    fn hidden_hole_cards_and_future_board_do_not_change_the_policy() {
        let policy = EquityPolicy::new(96);
        for (round, visible) in [(0, 0), (1, 3), (2, 4), (3, 5)] {
            let mut table = Table6::new(2026112019, 0);
            table.round = round;
            table.visible = visible;
            let player = table.actor;
            let before = table.view(player);
            let mut changed = table.clone();
            for other in 0..N {
                if other != player { changed.hole[other] = [50, 51]; }
            }
            changed.board[visible..].fill(49);
            assert_eq!(policy.equity(before), policy.equity(changed.view(player)));
            assert_eq!(policy.probabilities(before), policy.probabilities(changed.view(player)));

            // Even if a caller supplies values beyond `visible`, the estimator
            // must not use them for the pool, its RNG seed, or the final board.
            let mut unsanitized = before;
            unsanitized.board[visible..].fill(48);
            assert_eq!(policy.equity(before), policy.equity(unsanitized));
            assert_eq!(policy.probabilities(before), policy.probabilities(unsanitized));
        }
    }

    #[test]
    fn equivalent_card_order_repeats_the_same_sampling_and_policy() {
        let policy = EquityPolicy::new(96);
        let first = fixture();
        let mut reordered = first;
        reordered.hole.swap(0, 1);
        reordered.board.reverse();
        assert_eq!(policy.equity(first), policy.equity(reordered));
        assert_eq!(policy.probabilities(first), policy.probabilities(reordered));
        assert_eq!(policy.probabilities(first), policy.probabilities(first));
    }

    #[test]
    fn every_nonempty_legal_mask_produces_a_valid_distribution() {
        let policy = EquityPolicy::new(16);
        for mask in 1u8..64 {
            let mut view = fixture();
            view.legal = std::array::from_fn(|action| mask & (1 << action) != 0);
            let distribution = policy.probabilities(view);
            assert!((distribution.iter().sum::<f32>() - 1.0).abs() < 1e-6);
            for action in 0..ACTIONS {
                assert!(distribution[action].is_finite() && distribution[action] >= 0.0);
                if !view.legal[action] { assert_eq!(distribution[action], 0.0); }
            }
        }
    }

    #[test]
    fn unknown_pool_excludes_own_cards_and_only_the_visible_board() {
        for visible in [0, 3, 4, 5] {
            let mut view = fixture();
            view.visible = visible;
            let pool = UnknownCards::new(view);
            assert_eq!(pool.len, 50 - visible);
            for card in 0u8..52 {
                let known = view.hole.contains(&card) || view.board[..visible].contains(&card);
                assert_eq!(pool.cards[..pool.len].contains(&card), !known,
                    "wrong exclusion for card {card} with {visible} public cards");
            }
        }
    }

    #[test]
    fn sampled_multiway_runouts_have_no_duplicate_or_known_cards() {
        for visible in [0, 3, 4, 5] {
            for active in 2..=N {
                let mut view = fixture();
                view.visible = visible;
                view.active = active;
                let mut pool = UnknownCards::new(view);
                let mut rng = Rng::new(918223);
                let needed = 5 - visible + 2 * (active - 1);
                for _ in 0..128 {
                    let sample = pool.draw(needed, &mut rng);
                    assert_eq!(sample.len(), needed);
                    let mut seen = 0u64;
                    for &card in sample {
                        assert!(card < 52);
                        assert!(!view.hole.contains(&card));
                        assert!(!view.board[..visible].contains(&card));
                        assert_eq!(seen & (1u64 << card), 0, "duplicated sampled card");
                        seen |= 1u64 << card;
                    }
                }
            }
        }
    }
}

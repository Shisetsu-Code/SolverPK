//! Independent, public-information Monte Carlo reference for the frozen arena.
//! Unknown hands are uniform and do not condition on prior betting. Equity is
//! showdown pot share, including multiway ties; it is not an action EV estimate.
//! The betting rule is a fixed diagnostic heuristic, not a GTO policy.
use super::{Rng, View, ACTIONS, N};
use crate::evaluator::score7;

pub(super) struct EquityPolicy {
    samples: usize,
}

impl EquityPolicy {
    pub(super) fn new(samples: usize) -> Self {
        assert!(samples > 0, "equity simulation budget must be positive");
        Self { samples }
    }

    pub(super) fn probabilities(&self, view: View) -> [f32; ACTIONS] {
        let equity = self.equity(view) as f32;
        let price = view.call as f32 / (view.pot + view.call).max(1) as f32;
        let fair_share = 1.0 / view.active as f32;
        let value_threshold = (fair_share + 0.18).max(0.45);
        // Fixed before benchmark results: small call tolerance, infrequent
        // bluffs, and value aggression above both pot odds and fair pot share.
        // This ignores range conditioning, future betting and side-pot EV.
        let (fold, call, raise) = if view.call > 0 && equity + 0.025 < price {
            (0.96, 0.04, 0.0)
        } else if equity >= value_threshold && equity >= price + 0.12 {
            (0.0, 0.20, 0.80)
        } else if view.call == 0 {
            (0.0, 0.98, 0.02)
        } else {
            (0.04, 0.94, 0.02)
        };
        let sizes = [0.40, 0.40, 0.18, 0.02];
        let mut result = [0.0; ACTIONS];
        result[0] = fold;
        result[1] = call;
        for action in 2..ACTIONS { result[action] = raise * sizes[action - 2]; }
        for action in 0..ACTIONS {
            if !view.legal[action] { result[action] = 0.0; }
        }
        let sum = result.iter().sum::<f32>();
        if sum > 0.0 {
            for probability in &mut result { *probability /= sum; }
        } else if let Some(action) = (0..ACTIONS).find(|&action| view.legal[action]) {
            result[action] = 1.0;
        }
        result
    }

    fn equity(&self, view: View) -> f64 {
        assert!((1..=N).contains(&view.active), "invalid active player count");
        let mut unknown = UnknownCards::new(view);
        let mut rng = Rng::new(information_seed(view));
        let missing = 5 - view.visible;
        let opponents = view.active - 1;
        let needed = missing + 2 * opponents;
        let mut hero = [0u8; 7];
        hero[..2].copy_from_slice(&view.hole);
        hero[2..2 + view.visible].copy_from_slice(&view.board[..view.visible]);
        // The completed hero hand never changes on the river.
        let river_score = if missing == 0 { Some(score7(&hero)) } else { None };
        let mut rival = [0u8; 7];
        let mut share = 0.0;
        for _ in 0..self.samples {
            let sampled = unknown.draw(needed, &mut rng);
            hero[2 + view.visible..].copy_from_slice(&sampled[..missing]);
            rival[2..].copy_from_slice(&hero[2..]);
            let our_score = river_score.unwrap_or_else(|| score7(&hero));
            let mut ties = 1usize;
            let mut lost = false;
            for opponent in 0..opponents {
                let offset = missing + 2 * opponent;
                rival[..2].copy_from_slice(&sampled[offset..offset + 2]);
                let their_score = score7(&rival);
                if their_score > our_score {
                    lost = true;
                    break;
                }
                if their_score == our_score { ties += 1; }
            }
            if !lost { share += 1.0 / ties as f64; }
        }
        share / self.samples as f64
    }
}

fn information_seed(view: View) -> u64 {
    // Canonical known-card sets make sampling insensitive to card order.
    // No real-deal seed, opponent cards or unrevealed board enter this RNG.
    let hole = view.hole.iter().fold(0u64, |mask, &card| mask | (1u64 << card));
    let board = view.board[..view.visible].iter()
        .fold(0u64, |mask, &card| mask | (1u64 << card));
    let mut hash = 0x73f4_06d1_b8a9_2ce5u64; // Equity sampler domain.
    for value in [hole, board, view.active as u64] {
        for byte in value.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

struct UnknownCards {
    cards: [u8; 52],
    len: usize,
}

impl UnknownCards {
    fn new(view: View) -> Self {
        assert!(view.visible <= 5, "invalid visible board length");
        let mut excluded = 0u64;
        for &card in view.hole.iter().chain(view.board[..view.visible].iter()) {
            assert!(card < 52, "invalid known card");
            excluded |= 1u64 << card;
        }
        let mut result = Self { cards: [0; 52], len: 0 };
        for card in 0u8..52 {
            if excluded & (1u64 << card) == 0 {
                result.cards[result.len] = card;
                result.len += 1;
            }
        }
        result
    }

    fn draw(&mut self, count: usize, rng: &mut Rng) -> &[u8] {
        assert!(count <= self.len, "not enough unknown cards");
        // A partial Fisher-Yates shuffle selects an ordered uniform sample
        // without replacement. Reusing the permuted pool remains unbiased.
        for index in 0..count {
            let other = index + rng.range(self.len - index);
            self.cards.swap(index, other);
        }
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

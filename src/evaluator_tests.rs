use super::{evaluate, score5, score7};

// The oracle deliberately does not use evaluate, straight_high, packed, the
// production rank-count/mask algorithm, or the production RNG/deck. Its key is
// category followed by five descending tie breakers, compared lexicographically.
type Key = [u8; 6];

fn card(rank: u8, suit: u8) -> u8 {
    suit * 13 + rank - 2
}

fn score_key(score: u64) -> Key {
    assert_eq!(score >> 24, 0, "unexpected bits above the six score nibbles");
    std::array::from_fn(|index| ((score >> (4 * (5 - index))) & 15) as u8)
}

fn reference_five(cards: &[u8; 5]) -> Key {
    let mut ranks = cards.map(|card| card % 13 + 2);
    ranks.sort_unstable_by(|a, b| b.cmp(a));
    let flush = cards.iter().all(|card| card / 13 == cards[0] / 13);
    let straight = if ranks == [14, 5, 4, 3, 2] {
        5
    } else if ranks.windows(2).all(|pair| pair[0] == pair[1] + 1) {
        ranks[0]
    } else {
        0
    };

    // Count equal-rank runs in the sorted list, then order groups by size and
    // rank. This is independent of the evaluator's array indexed by rank.
    let mut groups = [(0u8, 0u8); 5];
    let mut group_count = 0;
    for rank in ranks {
        if group_count > 0 && groups[group_count - 1].1 == rank {
            groups[group_count - 1].0 += 1;
        } else {
            groups[group_count] = (1, rank);
            group_count += 1;
        }
    }
    groups[..group_count].sort_unstable_by(|a, b| b.cmp(a));

    if flush && straight > 0 {
        [8, straight, 0, 0, 0, 0]
    } else if groups[0].0 == 4 {
        [7, groups[0].1, groups[1].1, 0, 0, 0]
    } else if groups[0].0 == 3 && groups[1].0 == 2 {
        [6, groups[0].1, groups[1].1, 0, 0, 0]
    } else if flush {
        [5, ranks[0], ranks[1], ranks[2], ranks[3], ranks[4]]
    } else if straight > 0 {
        [4, straight, 0, 0, 0, 0]
    } else if groups[0].0 == 3 {
        [3, groups[0].1, groups[1].1, groups[2].1, 0, 0]
    } else if groups[0].0 == 2 && groups[1].0 == 2 {
        [2, groups[0].1, groups[1].1, groups[2].1, 0, 0]
    } else if groups[0].0 == 2 {
        [1, groups[0].1, groups[1].1, groups[2].1, groups[3].1, 0]
    } else {
        [0, ranks[0], ranks[1], ranks[2], ranks[3], ranks[4]]
    }
}

fn reference_best(cards: &[u8]) -> Key {
    let mut best = [0; 6];
    for a in 0..cards.len() - 4 {
        for b in a + 1..cards.len() - 3 {
            for c in b + 1..cards.len() - 2 {
                for d in c + 1..cards.len() - 1 {
                    for e in d + 1..cards.len() {
                        best = best.max(reference_five(&[
                            cards[a], cards[b], cards[c], cards[d], cards[e],
                        ]));
                    }
                }
            }
        }
    }
    best
}

// Separate deterministic generator: randomized coverage must not depend on
// the simulator's shuffle being correct. Modulo bias is immaterial here; this
// is a reproducible test corpus, not a statistical test of dealing fairness.
fn sample_seven(state: &mut u64) -> [u8; 7] {
    let mut cards = [0; 7];
    for index in 0..cards.len() {
        loop {
            *state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let candidate = ((*state >> 32) % 52) as u8;
            if !cards[..index].contains(&candidate) {
                cards[index] = candidate;
                break;
            }
        }
    }
    cards
}

#[test]
fn five_card_categories_and_tiebreakers_match_hand_checked_fixtures() {
    let c = card;
    let fixtures = [
        ([c(14,0),c(13,1),c(9,2),c(5,3),c(3,0)], [0,14,13,9,5,3]),
        ([c(11,0),c(11,1),c(14,2),c(9,3),c(4,0)], [1,11,14,9,4,0]),
        ([c(8,0),c(8,1),c(4,2),c(4,3),c(14,0)], [2,8,4,14,0,0]),
        ([c(6,0),c(6,1),c(6,2),c(14,3),c(9,0)], [3,6,14,9,0,0]),
        ([c(14,0),c(2,1),c(3,2),c(4,3),c(5,0)], [4,5,0,0,0,0]),
        ([c(2,0),c(3,1),c(4,2),c(5,3),c(6,0)], [4,6,0,0,0,0]),
        ([c(10,0),c(11,1),c(12,2),c(13,3),c(14,0)], [4,14,0,0,0,0]),
        ([c(14,2),c(11,2),c(9,2),c(4,2),c(2,2)], [5,14,11,9,4,2]),
        ([c(4,0),c(4,1),c(4,2),c(14,0),c(14,1)], [6,4,14,0,0,0]),
        ([c(3,0),c(3,1),c(3,2),c(3,3),c(14,0)], [7,3,14,0,0,0]),
        ([c(14,3),c(2,3),c(3,3),c(4,3),c(5,3)], [8,5,0,0,0,0]),
        ([c(10,0),c(11,0),c(12,0),c(13,0),c(14,0)], [8,14,0,0,0,0]),
        // An ace cannot join Q-K-A-2-3 into a straight.
        ([c(12,0),c(13,1),c(14,2),c(2,3),c(3,0)], [0,14,13,12,3,2]),
    ];
    for (cards, expected) in fixtures {
        assert_eq!(reference_five(&cards), expected, "oracle cards={cards:?}");
        assert_eq!(score_key(score5(&cards)), expected, "cards={cards:?}");
    }
}

#[test]
fn seven_card_edge_cases_match_hand_checked_fixtures() {
    let c = card;
    let fixtures = [
        // A mixed-suit straight plus a flush is not a straight flush.
        ([c(2,0),c(3,0),c(4,0),c(5,0),c(9,0),c(6,1),c(7,2)], [5,9,5,4,3,2]),
        // Use the higher trips as trips and the lower trips as a pair.
        ([c(14,0),c(14,1),c(14,2),c(13,0),c(13,1),c(13,2),c(2,3)], [6,14,13,0,0,0]),
        // The third pair can supply the two-pair kicker.
        ([c(14,0),c(14,1),c(13,0),c(13,1),c(12,0),c(12,1),c(2,3)], [2,14,13,12,0,0]),
        ([c(5,0),c(5,1),c(4,0),c(4,1),c(3,0),c(3,1),c(14,3)], [2,5,4,14,0,0]),
        // With trips and two pairs, use the higher pair for the full house.
        ([c(8,0),c(8,1),c(8,2),c(14,0),c(14,1),c(13,0),c(13,1)], [6,8,14,0,0,0]),
        ([c(10,0),c(10,1),c(10,2),c(10,3),c(14,1),c(13,2),c(12,3)], [7,10,14,0,0,0]),
        ([c(14,0),c(14,1),c(14,2),c(14,3),c(13,0),c(13,1),c(12,2)], [7,14,13,0,0,0]),
        // Keep only the five highest cards of a six/seven-card flush.
        ([c(14,0),c(13,0),c(11,0),c(8,0),c(5,0),c(3,0),c(2,0)], [5,14,13,11,8,5]),
        ([c(14,0),c(12,0),c(11,0),c(9,0),c(4,0),c(2,0),c(13,1)], [5,14,12,11,9,4]),
        ([c(14,0),c(2,0),c(3,0),c(4,0),c(5,0),c(6,1),c(7,2)], [8,5,0,0,0,0]),
        ([c(8,2),c(9,2),c(10,2),c(11,2),c(12,2),c(13,2),c(14,2)], [8,14,0,0,0,0]),
        // Six-high wins over the wheel; a duplicate rank cannot break a straight.
        ([c(14,0),c(2,1),c(3,2),c(4,3),c(5,0),c(6,1),c(13,2)], [4,6,0,0,0,0]),
        ([c(5,0),c(6,0),c(7,1),c(7,2),c(8,3),c(9,2),c(14,3)], [4,9,0,0,0,0]),
        ([c(14,0),c(14,1),c(14,2),c(13,0),c(12,1),c(11,2),c(9,3)], [3,14,13,12,0,0]),
        ([c(2,0),c(2,1),c(14,0),c(13,1),c(12,2),c(11,3),c(9,2)], [1,2,14,13,12,0]),
        ([c(14,0),c(13,1),c(11,2),c(9,3),c(7,0),c(5,1),c(3,2)], [0,14,13,11,9,7]),
    ];
    for (cards, expected) in fixtures {
        assert_eq!(reference_best(&cards), expected, "oracle cards={cards:?}");
        assert_eq!(score_key(score7(&cards)), expected, "cards={cards:?}");
    }
}

#[test]
fn independent_oracle_matches_random_five_six_and_seven_card_hands() {
    for seed in [0, 1, 0x5ead_cafe_9b37_6402, u64::MAX] {
        let mut state = seed;
        for sample in 0..2500 {
            let cards = sample_seven(&mut state);
            for count in 5..=7 {
                let hand = &cards[..count];
                assert_eq!(
                    score_key(evaluate(hand)), reference_best(hand),
                    "seed={seed}, sample={sample}, cards={hand:?}",
                );
            }
        }
    }
}

#[test]
fn card_order_and_suit_relabeling_preserve_rank() {
    let mut state = 0x6c40_8a1b_3d9e_f275;
    for _ in 0..256 {
        let cards = sample_seven(&mut state);
        let expected = score7(&cards);
        let mut reordered = cards;
        for _ in 0..7 {
            reordered.rotate_left(1);
            assert_eq!(score7(&reordered), expected);
            reordered.reverse();
            assert_eq!(score7(&reordered), expected);
            reordered.reverse();
        }
        // All 24 bijections of the four suits must leave the hand unchanged.
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    if a == b || a == c || b == c {
                        continue;
                    }
                    let suits = [a, b, c, 6 - a - b - c];
                    let relabeled = cards.map(|card| suits[(card / 13) as usize] * 13 + card % 13);
                    assert_eq!(score7(&relabeled), expected, "cards={cards:?}, suits={suits:?}");
                }
            }
        }
    }
}

#[test]
fn unused_hole_cards_do_not_break_a_board_tie() {
    let c = card;
    // Four nines plus the ace on the board beat both players' hole cards.
    let first = [c(9,0),c(9,1),c(9,2),c(9,3),c(14,0),c(13,1),c(12,2)];
    let second = [c(9,0),c(9,1),c(9,2),c(9,3),c(14,0),c(11,1),c(10,2)];
    assert_eq!(score_key(score7(&first)), [7,9,14,0,0,0]);
    assert_eq!(score7(&first), score7(&second));
}

#[test]
#[ignore = "exhaustive 2,598,960-hand audit; run with cargo test --release exhaustive_five_card -- --ignored"]
fn exhaustive_five_card_scores_match_independent_oracle_and_frequencies() {
    let mut counts = [0u64; 9];
    let mut distinct = std::collections::BTreeSet::new();
    for a in 0..48u8 {
        for b in a + 1..49 {
            for c in b + 1..50 {
                for d in c + 1..51 {
                    for e in d + 1..52 {
                        let cards = [a, b, c, d, e];
                        let actual = score_key(score5(&cards));
                        assert_eq!(actual, reference_five(&cards), "cards={cards:?}");
                        counts[actual[0] as usize] += 1;
                        distinct.insert(actual);
                    }
                }
            }
        }
    }
    // Exact combinatorial frequencies, in ascending category order. Straights
    // and flushes here exclude straight flushes; royal flushes are included in
    // the 40 straight flushes. Sum = C(52,5).
    assert_eq!(counts, [1_302_540, 1_098_240, 123_552, 54_912, 10_200, 5_108, 3_744, 624, 40]);
    assert_eq!(counts.iter().sum::<u64>(), 2_598_960);
    let mut distinct_counts = [0usize; 9];
    for rank in distinct {
        distinct_counts[rank[0] as usize] += 1;
    }
    assert_eq!(distinct_counts, [1_277, 2_860, 858, 858, 10, 1_277, 156, 156, 10]);
    assert_eq!(distinct_counts.iter().sum::<usize>(), 7_462);
}

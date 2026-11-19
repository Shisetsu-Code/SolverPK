use super::*;

// During the RED stage these helpers deliberately exercise the current encoder.
// The production V2 encoder and the frozen V1 encoder replace them after the
// regression failures have been observed in CI.
fn current_input(t: &Table6, p: usize) -> Vec<f32> {
    neural_input(t, p).to_vec()
}

fn historical_input(t: &Table6, p: usize) -> Vec<f32> {
    neural_input(t, p).to_vec()
}

fn visible_fixture() -> Table6 {
    let mut t = Table6::new(73, 0);
    // Card numbering is clubs, diamonds, hearts, spades; 2 through ace.
    t.hole = [[38, 31], [39, 40], [41, 42], [43, 44], [45, 46], [47, 48]];
    t.board = [26, 29, 35, 13, 14]; // 2h 5h Jh | 2d 3d
    t.visible = 3;
    t.round = 1;
    t
}

#[test]
fn observation_distinguishes_heart_flush_from_same_rank_clubs() {
    let hearts = visible_fixture(); // Ah 7h on 2h 5h Jh
    let mut clubs = hearts.clone();
    clubs.hole[0] = [12, 5]; // Ac 7c on the same public board
    assert_ne!(current_input(&hearts, 0), current_input(&clubs, 0),
        "a made heart flush must not collide with two clubs of the same ranks");
}

#[test]
fn observation_keeps_rank_suit_association_for_offsuit_hole_cards() {
    let mut ace_of_hearts = visible_fixture();
    ace_of_hearts.hole[0] = [38, 5]; // Ah 7c
    let mut seven_of_hearts = ace_of_hearts.clone();
    seven_of_hearts.hole[0] = [12, 31]; // Ac 7h
    assert_ne!(current_input(&ace_of_hearts, 0), current_input(&seven_of_hearts, 0),
        "the heart ace and the heart seven have different blockers");
}

#[test]
fn observation_keeps_rank_suit_association_for_board_cards() {
    let mut heart_deuce = visible_fixture();
    heart_deuce.board[..3].copy_from_slice(&[26, 29, 9]); // 2h 5h Jc
    let mut heart_jack = heart_deuce.clone();
    heart_jack.board[..3].copy_from_slice(&[0, 29, 35]); // 2c 5h Jh
    assert_ne!(current_input(&heart_deuce, 0), current_input(&heart_jack, 0),
        "matching rank and suit marginals must not hide different board cards");
}

#[test]
fn observation_does_not_depend_on_hole_card_order() {
    let first = visible_fixture();
    let mut reversed = first.clone();
    reversed.hole[0].swap(0, 1);
    assert_eq!(current_input(&first, 0), current_input(&reversed, 0));
}

#[test]
fn observation_does_not_depend_on_visible_board_order() {
    let mut first = visible_fixture();
    first.visible = 5;
    first.round = 3;
    let mut reordered = first.clone();
    reordered.board = [35, 14, 26, 13, 29];
    assert_eq!(current_input(&first, 0), current_input(&reordered, 0));
}

#[test]
fn observation_never_reads_opponent_cards_or_unrevealed_board() {
    for (round, visible) in [(0, 0), (1, 3), (2, 4), (3, 5)] {
        let mut first = visible_fixture();
        first.visible = visible;
        first.round = round;
        let mut hidden_changed = first.clone();
        for q in 1..N {
            hidden_changed.hole[q] = [(13 + 2*q) as u8, (14 + 2*q) as u8];
        }
        for i in visible..5 {
            hidden_changed.board[i] = i as u8;
        }
        assert_eq!(current_input(&first, 0), current_input(&hidden_changed, 0),
            "hidden cards leaked at round {round}");
        assert_eq!(historical_input(&first, 0), historical_input(&hidden_changed, 0),
            "legacy inference leaked hidden cards at round {round}");
    }
}

#[test]
fn legacy_observation_matches_the_original_64_feature_reference() {
    let mut t = visible_fixture();
    t.hole.swap(0, 2);
    t.button = 4;
    t.stack = [180, 160, 140, 120, 100, 80];
    t.street = [5, 10, 15, 20, 25, 30];
    t.total = [20, 40, 60, 80, 100, 120];
    t.pot = 420;
    t.folded = [false, true, false, false, true, false];
    t.high = 30;
    t.min_raise = 10;
    t.raise_count = 3;
    t.raise_per_seat = [0, 1, 3, 1, 2, 1];
    t.estimated_ranges[0] = [0.75, 0.125, 0.125];
    t.estimated_ranges[3] = [0.50, 0.250, 0.250];
    t.estimated_ranges[5] = [0.25, 0.375, 0.375];
    // Hand-derived V1 layout; all omitted coordinates must remain zero.
    let mut expected = [0.0f32; 64];
    for (index, value) in [
        (12, 1.0), (18, 1.0), (26, 1.0),
        (28, 0.25), (31, 0.25), (37, 0.25), (43, 0.6),
        (45, 1.0/3.0), (46, 4.0/6.0), (47, 1.75),
        (48, 0.075), (49, 0.7), (50, 0.0), (51, 2.0/3.0),
        (52, 1.0), (53, 1.0), (54, 0.6), (55, 0.15),
        (56, 0.5), (57, 0.25), (58, 0.25), (59, 2.0/6.0),
        (60, 0.3), (61, 0.075), (62, 0.6), (63, 0.05),
    ] {
        expected[index] = value;
    }
    assert_eq!(historical_input(&t, 2), expected.to_vec());
}

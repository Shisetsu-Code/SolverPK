use super::*;

fn current_input(t: &Table6, p: usize) -> Vec<f32> {
    neural_input(t, p, crate::neural::ObservationVersion::SuitAwareV2).to_vec()
}

fn historical_input(t: &Table6, p: usize) -> Vec<f32> {
    neural_input(t, p, crate::neural::ObservationVersion::LegacyV1)
        [..crate::neural::LEGACY_INPUT].to_vec()
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
        for p in 0..N {
            let mut hidden_changed = first.clone();
            for q in 0..N {
                if q != p {
                    hidden_changed.hole[q] = [(2*q) as u8, (2*q + 1) as u8];
                }
            }
            for i in visible..5 {
                hidden_changed.board[i] = (15 + i) as u8;
            }
            assert_eq!(current_input(&first, p), current_input(&hidden_changed, p),
                "hidden cards leaked at round {round}, seat {p}");
            assert_eq!(historical_input(&first, p), historical_input(&hidden_changed, p),
                "legacy inference leaked hidden cards at round {round}, seat {p}");
        }
    }
}

#[test]
fn legacy_observation_matches_the_original_64_feature_reference() {
    let t = public_fixture();
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
    let padded = neural_input(&t, 2, crate::neural::ObservationVersion::LegacyV1);
    assert!(padded[64..].iter().all(|&x| x == 0.0));
}

fn public_fixture() -> Table6 {
    let mut t = visible_fixture();
    t.hole.swap(0, 2);
    t.button = 4;
    t.sb = 5;
    t.bb = 0;
    t.actor = 2;
    t.stack = [180, 160, 140, 120, 100, 80];
    t.street = [5, 10, 15, 20, 25, 30];
    t.total = [20, 40, 60, 80, 100, 120];
    t.pot = 420;
    t.folded = [false, true, false, false, true, false];
    t.pending = [true, false, true, true, false, false];
    t.acted = [false, false, true, false, false, false];
    t.high = 30;
    t.min_raise = 10;
    t.raise_count = 3;
    t.raise_per_seat = [0, 1, 3, 1, 2, 1];
    t.estimated_ranges[0] = [0.75, 0.125, 0.125];
    t.estimated_ranges[3] = [0.50, 0.250, 0.250];
    t.estimated_ranges[5] = [0.25, 0.375, 0.375];
    t
}

#[test]
fn observation_card_planes_contain_exactly_the_known_cards() {
    let t = visible_fixture();
    let x = current_input(&t, 0);
    let mut expected_hole = [0.0f32; 52];
    expected_hole[31] = 1.0; // 7h
    expected_hole[38] = 1.0; // Ah
    let mut expected_board = [0.0f32; 52];
    expected_board[26] = 1.0; // 2h
    expected_board[29] = 1.0; // 5h
    expected_board[35] = 1.0; // Jh
    assert_eq!(&x[..52], &expected_hole);
    assert_eq!(&x[52..104], &expected_board);
    let other_seat = current_input(&t, 2);
    let mut other_hole = [0.0f32; 52];
    other_hole[41] = 1.0; // 4s
    other_hole[42] = 1.0; // 5s
    assert_eq!(&other_seat[..52], &other_hole);
    assert_eq!(&other_seat[52..104], &expected_board);
}

#[test]
fn observation_encodes_public_state_with_physical_normalization() {
    let t = public_fixture();
    let x = current_input(&t, 2);
    // Round, alive, pot, call, stack, button, texture, pressure, range,
    // raises, high, 3 beliefs, folded, total, street, visible, min_raise,
    // relative position, live bettors, acted, opponent stack, pending.
    let expected: [f32; 24] = [
        1.0/3.0, 4.0/6.0, 0.35, 0.075, 0.7, 0.0, 2.0/3.0, 1.0, 1.0,
        3.0/255.0, 0.15, 0.5, 0.25, 0.25, 2.0/6.0, 0.3, 0.075, 0.6,
        0.05, 0.8, 4.0/6.0, 1.0, 0.9, 0.4,
    ];
    assert_eq!(&x[104..], &expected);
}

#[test]
fn observation_public_scalars_are_bounded_at_their_maxima() {
    let mut t = visible_fixture();
    t.visible = 5;
    t.round = 3;
    t.stack = [0; N];
    t.street = [200; N];
    t.total = [200; N];
    t.pot = 1200;
    t.high = 200;
    t.min_raise = 200;
    t.folded = [false; N];
    t.pending = [false; N];
    t.acted = [true; N];
    t.raise_count = u8::MAX;
    t.raise_per_seat = [u8::MAX; N];
    let x = current_input(&t, 0);
    assert!(x[104..].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    for index in [106, 113, 114, 119, 120, 122] {
        assert_eq!(x[index], 1.0);
    }
    for index in [107, 108, 124, 126, 127] {
        assert_eq!(x[index], 0.0);
    }
}

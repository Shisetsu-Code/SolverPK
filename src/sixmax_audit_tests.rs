use super::*;

#[test]
fn audit_regression_cumulative_short_allins_reopen_after_full_raise_faced() {
    let mut t = Table6::new(23, 0);
    t.act(Action::Raise(20)).unwrap(); // seat 3, full increment 18
    t.stack[4] = 30;
    t.act(Action::Raise(30)).unwrap(); // short +10
    t.stack[5] = 40;
    t.act(Action::Raise(40)).unwrap(); // short +10; seat 3 now faces +20
    for _ in 0..3 { t.act(Action::Call).unwrap(); }
    assert_eq!(t.actor, 3);
    assert_eq!(t.min_raise, 18);
    assert!(t.legal(3)[2..].iter().any(|&yes| yes));
    t.act(Action::Raise(58)).unwrap();
}

#[test]
fn calling_an_intermediate_short_raise_resets_reopening_threshold() {
    let mut t = Table6::new(23, 0);
    t.act(Action::Raise(20)).unwrap();
    t.stack[4] = 30;
    t.act(Action::Raise(30)).unwrap();
    t.act(Action::Call).unwrap(); // seat 5 has now acted at 30
    t.stack[0] = 40;
    t.act(Action::Raise(40)).unwrap();
    for _ in 0..3 { t.act(Action::Call).unwrap(); }
    assert_eq!(t.actor, 5);
    assert!(!t.legal(5)[2..].iter().any(|&yes| yes));
}

#[test]
fn audit_regression_pot_raise_includes_the_call() {
    let t = Table6::new(7, 0);
    // Pot 3, call 2, pot after calling 5, so raise-to is 2 + 5 = 7.
    assert_eq!(t.raise_to(3, 4), 7);
    let mut t = t;
    for _ in 0..6 { t.act(Action::Call).unwrap(); }
    t.next_street().unwrap();
    t.act(Action::Raise(6)).unwrap();
    assert_eq!((t.pot, t.high, t.call(t.actor)), (18, 6, 6));
    assert_eq!(t.raise_to(t.actor, 4), 30);
    assert_eq!(t.raise_to(t.actor, 2), 14);
    assert_eq!(t.raise_to(t.actor, 3), 24);
}

#[test]
fn audit_regression_folded_contributions_do_not_create_extra_odd_chips() {
    let mut t = Table6::new(11, 0);
    t.hole = std::array::from_fn(|p| [(2*p) as u8, (2*p+1) as u8]);
    t.board = [47, 48, 49, 50, 51]; // royal flush on board
    for action in [Action::Raise(5), Action::Raise(10), Action::Call,
        Action::Fold, Action::Fold, Action::Fold, Action::Fold] {
        t.act(action).unwrap();
    }
    assert_eq!(t.total, [0, 1, 2, 5, 10, 10]);
    for _ in 0..3 {
        t.next_street().unwrap();
        while !t.completed() { t.act(Action::Call).unwrap(); }
    }
    // There is ONE pot and no unmatched bet. Folded players' contributions
    // cannot create extra odd chips for seat 4 in a 28-chip split pot.
    assert_eq!(t.settle(), [0, 0, 0, 0, 14, 14]);
    assert_eq!(t.net(), [0, -1, -2, -5, 4, 4]);
}

#[test]
fn audit_regression_no_artificial_decisions_in_dry_sidepot() {
    let mut t = Table6::new(13, 0);
    t.folded = [false, false, true, true, true, true];
    t.stack = [0, 100, 0, 0, 0, 0];
    t.street = [100, 100, 0, 0, 0, 0];
    t.high = 100;
    t.pending = [false; N];
    t.next_street().unwrap();
    assert!(t.completed(), "one live stack cannot bet into an empty side pot");
}

#[test]
fn outstanding_allin_call_is_not_skipped() {
    let mut t = Table6::new(13, 0);
    t.act(Action::Raise(200)).unwrap();
    assert!(!t.completed());
    assert_eq!(t.actor, 4);
    assert_eq!(t.call(4), 200);
    assert!(t.legal(4)[0]);
    assert!(t.legal(4)[1]);
}

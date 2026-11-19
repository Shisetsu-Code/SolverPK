use super::*;

fn zero_net() -> Net {
    let mut net = Net::new(101);
    for row in net.w1.iter_mut() { row.fill(0.0); }
    net.b1.fill(0.0);
    for row in &mut net.w2 { row.fill(0.0); }
    net.b2.fill(0.0);
    net.baseline = 0.0;
    net
}

fn smooth_net() -> Net {
    let mut net = zero_net();
    net.b1.fill(-0.5); // Keep every unused unit away from the ReLU kink.
    net.b1[0] = 0.4;
    net.b1[1] = 0.3;
    net.w1[0][0] = 0.2;
    net.w1[0][INPUT - 1] = -0.1;
    net.w1[1][0] = -0.1;
    net.w1[1][INPUT - 1] = 0.15;
    for a in 0..ACTIONS {
        net.w2[a][0] = 0.03 * a as f32 - 0.06;
        net.w2[a][1] = 0.04 - 0.02 * a as f32;
        net.b2[a] = 0.025 * a as f32 - 0.05;
    }
    net
}

#[test]
fn fresh_checkpoint_uses_a_distinct_format_for_suit_aware_inputs() {
    let net = Net::new(103);
    let path = std::env::temp_dir().join(format!("solverpk-neural-v2-magic-{}.bin", std::process::id()));
    net.save(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(&bytes[..8], b"SPKNN002", "new card semantics must never masquerade as SPKNN001");
}

#[test]
fn positive_and_negative_returns_keep_their_monetary_magnitude() {
    let x = [0.0; INPUT];
    let legal = [false, true, true, false, false, false];
    for reward in [4.0, -4.0] {
        let mut net = zero_net();
        net.reinforce_hand(&[PolicySample { features: &x, legal, action: 1, reward }], &[reward], 0.001).unwrap();
        // At p=1/2, d log p(action=1)/d bias_1 = 1/2.
        let expected = 0.001 * reward * 0.5;
        assert!((net.b2[1] - expected).abs() < 1e-7,
            "terminal utility was clipped: reward={reward}, bias={}, expected={expected}", net.b2[1]);
    }
}

#[derive(Clone, Copy, Debug)]
enum Coordinate { W1(usize, usize), B1(usize), W2(usize, usize), B2(usize) }

fn parameter(net: &mut Net, coordinate: Coordinate) -> &mut f32 {
    match coordinate {
        Coordinate::W1(j, i) => &mut net.w1[j][i],
        Coordinate::B1(j) => &mut net.b1[j],
        Coordinate::W2(a, j) => &mut net.w2[a][j],
        Coordinate::B2(a) => &mut net.b2[a],
    }
}

#[test]
fn policy_gradient_matches_finite_differences_in_every_parameter_family() {
    let mut before = smooth_net();
    before.baseline = 0.25;
    let mut x = [0.0; INPUT];
    x[0] = 0.7;
    x[INPUT - 1] = 0.4;
    let legal = [false, true, true, true, false, true];
    let reward = 4.5;
    let lr = 0.001;
    let mut after = before.clone();
    after.reinforce_hand(&[PolicySample { features: &x, legal, action: 1, reward }], &[reward], lr).unwrap();
    for coordinate in [Coordinate::W1(0, 0), Coordinate::W1(1, INPUT - 1),
        Coordinate::B1(0), Coordinate::B1(1), Coordinate::W2(1, 0),
        Coordinate::W2(3, 1), Coordinate::B2(1), Coordinate::B2(3),
        Coordinate::W2(0, 0), Coordinate::B2(0), Coordinate::B1(2)] {
        let epsilon = 0.002;
        let mut plus = before.clone();
        let mut minus = before.clone();
        *parameter(&mut plus, coordinate) += epsilon;
        *parameter(&mut minus, coordinate) -= epsilon;
        // Numerical derivative of the frozen log-likelihood surrogate;
        // no optimizer/backprop helper participates in this expectation.
        let objective = |net: &Net| (reward - before.baseline) as f64
            * (net.forward(&x, legal).prob[1] as f64).ln();
        let numeric = (objective(&plus) - objective(&minus)) / (2.0 * epsilon as f64);
        let actual = (*parameter(&mut after, coordinate) as f64
            - *parameter(&mut before.clone(), coordinate) as f64) / lr as f64;
        assert!((actual - numeric).abs() < 0.001 + 0.005 * numeric.abs(),
            "{coordinate:?}: update gradient {actual}, finite difference {numeric}");
    }
}

#[test]
fn an_entire_hand_uses_the_preupdate_policy_and_baseline() {
    let x = [0.0; INPUT];
    let legal = [false, true, true, false, false, false];
    let mut net = zero_net();
    let samples = [
        PolicySample { features: &x, legal, action: 1, reward: 1.0 },
        PolicySample { features: &x, legal, action: 2, reward: 1.0 },
    ];
    net.reinforce_hand(&samples, &[1.0], 0.01).unwrap();
    // Opposite scores at the SAME uniform policy cancel exactly.
    assert!(net.b2.iter().all(|b| b.abs() < 1e-8),
        "visits used sequentially updated parameters: {:?}", net.b2);
    assert!((net.baseline - 0.0005).abs() < 1e-9, "baseline changed more than once per hand");
    assert_eq!(net.steps, 2);
}

#[test]
fn reversing_a_hand_does_not_change_its_policy_update() {
    let before = smooth_net();
    let mut x = [0.0; INPUT]; x[0] = 0.7; x[INPUT - 1] = 0.4;
    let mut y = [0.0; INPUT]; y[0] = -0.2; y[INPUT - 1] = 0.8;
    let legal = [false, true, true, true, false, false];
    let samples = [
        PolicySample { features: &x, legal, action: 1, reward: 1.4 },
        PolicySample { features: &y, legal, action: 2, reward: -0.8 },
        PolicySample { features: &x, legal, action: 3, reward: 0.6 },
    ];
    let reversed = [samples[2], samples[1], samples[0]];
    let rewards = [1.4, -0.8, 0.6];
    let mut first = before.clone();
    let mut second = before;
    first.reinforce_hand(&samples, &rewards, 0.02).unwrap();
    second.reinforce_hand(&reversed, &rewards, 0.02).unwrap();
    let a = first.w1.iter().flatten().chain(first.b1.iter())
        .chain(first.w2.iter().flatten()).chain(first.b2.iter());
    let b = second.w1.iter().flatten().chain(second.b1.iter())
        .chain(second.w2.iter().flatten()).chain(second.b2.iter());
    for (&left, &right) in a.zip(b) {
        assert!((left - right).abs() < 1e-7,
            "trajectory traversal changed the update: {left} != {right}");
    }
    assert_eq!(first.baseline, second.baseline);
}

#[test]
fn zero_sum_selfplay_uses_each_players_own_return() {
    let x = [0.0; INPUT];
    let legal = [false, true, true, false, false, false];
    let mut net = zero_net();
    net.reinforce_hand(&[
        PolicySample { features: &x, legal, action: 1, reward: 1.0 },
        PolicySample { features: &x, legal, action: 2, reward: -1.0 },
    ], &[1.0, -1.0], 0.01).unwrap();
    // Mean independent-player score: ((+.5)*1 + (-.5)*(-1))/2 = +.5.
    // Multiplying every score by the table's total return would give zero.
    assert!((net.b2[1] - 0.005).abs() < 1e-8);
    assert!((net.b2[2] + 0.005).abs() < 1e-8);
    assert_eq!(net.baseline, 0.0);
}

#[test]
fn gradient_is_normalized_by_players_never_by_decision_count() {
    let x = [0.0; INPUT];
    let legal = [false, true, true, false, false, false];
    let sample = PolicySample { features: &x, legal, action: 1, reward: 1.0 };
    let mut two = zero_net();
    let mut four = zero_net();
    // Player two may have no policy decisions (for example after going all-in).
    two.reinforce_hand(&[sample; 2], &[1.0, -1.0], 0.01).unwrap();
    four.reinforce_hand(&[sample; 4], &[1.0, -1.0], 0.01).unwrap();
    assert!((two.b2[1] - 0.005).abs() < 1e-8);
    assert!((four.b2[1] - 0.010).abs() < 1e-8);
    assert_eq!(two.baseline, 0.0, "a seat with no decisions disappeared from the baseline");
}

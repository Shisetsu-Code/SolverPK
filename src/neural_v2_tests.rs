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

fn checkpoint_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("solverpk-neural-v2-{label}-{}.bin", std::process::id()))
}

fn segment(seed: u64, start_hand: u64, hands: u64) -> TrainingSegment {
    TrainingSegment { kind: TrainingKind::SelfPlay, seed, start_hand, hands,
        learning_rate: 0.002, rake_bps: 500, rake_cap_units: 60_000, no_flop_no_drop: true }
}

#[test]
fn legacy_checkpoint_keeps_original_bytes_and_float_operations() {
    // Independently construct a full historical 64 -> 32 -> 6 payload.
    let w1: [[f32; 64]; HIDDEN] = std::array::from_fn(|j|
        std::array::from_fn(|i| ((j * 64 + i) % 17) as f32 * 0.03125 - 0.25));
    let b1: [f32; HIDDEN] = std::array::from_fn(|j| j as f32 * 0.015625 - 0.125);
    let w2: [[f32; HIDDEN]; ACTIONS] = std::array::from_fn(|a|
        std::array::from_fn(|j| ((a + j) % 9) as f32 * 0.0625 - 0.25));
    let b2: [f32; ACTIONS] = std::array::from_fn(|a| a as f32 * 0.03125 - 0.0625);
    let mut original = b"SPKNN001".to_vec();
    original.extend_from_slice(&37_u64.to_le_bytes());
    original.extend_from_slice(&(-0.375_f32).to_le_bytes());
    for &v in w1.iter().flatten().chain(b1.iter()).chain(w2.iter().flatten()).chain(b2.iter()) {
        original.extend_from_slice(&v.to_le_bytes());
    }
    let path = checkpoint_path("legacy");
    std::fs::write(&path, &original).unwrap();
    let mut restored = Net::load(path.to_str().unwrap()).unwrap();
    assert_eq!(restored.observation_version(), ObservationVersion::LegacyV1);
    assert!(!restored.provenance_complete());
    assert!(Net::load_for_training(path.to_str().unwrap()).is_err());
    let mut x = [0.0; INPUT];
    for (i, v) in x[..64].iter_mut().enumerate() { *v = (i % 7) as f32 * 0.125; }
    x[64..].fill(123.0); // The historical forward pass must never touch this tail.
    let legal = [false, true, true, false, true, true];
    let hidden: [f32; HIDDEN] = std::array::from_fn(|j| {
        let mut sum = b1[j];
        for i in 0..64 { sum += w1[j][i] * x[i]; }
        sum.max(0.0)
    });
    let mut expected = [0.0_f32; ACTIONS];
    let mut scores = [f32::NEG_INFINITY; ACTIONS];
    for a in 0..ACTIONS {
        if legal[a] {
            let mut sum = b2[a];
            for j in 0..HIDDEN { sum += w2[a][j] * hidden[j]; }
            scores[a] = sum;
        }
    }
    let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut total = 0.0_f32;
    for a in 0..ACTIONS { if legal[a] { expected[a] = (scores[a] - max).exp(); total += expected[a]; } }
    for p in &mut expected { *p /= total; }
    assert_eq!(restored.forward(&x, legal).prob.map(f32::to_bits), expected.map(f32::to_bits));
    assert!(restored.reinforce_hand(&[PolicySample { features: &x, legal, action: 1, reward: 1.0 }], &[1.0], 0.01).is_err());
    assert!(restored.imitate(&x, legal, [0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 0.01).is_err());
    restored.save(path.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original,
        "evaluating or rejecting an update changed a historical checkpoint");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn resumed_training_metadata_roundtrips_and_merges_contiguous_work() {
    let mut net = smooth_net();
    let x = [0.1; INPUT];
    net.imitate(&x, [true; ACTIONS], [0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 0.01).unwrap();
    net.record_training(segment(211, 0, 3)).unwrap();
    net.record_training(segment(211, 3, 2)).unwrap();
    assert_eq!(net.training_segments(), &[segment(211, 0, 5)]);
    let mut distillation = segment(211, 5, 4);
    distillation.kind = TrainingKind::Distillation;
    net.record_training(distillation).unwrap();
    assert_eq!(net.next_training_hand(211).unwrap(), 9);
    assert_eq!(net.next_training_hand(999).unwrap(), 0);
    assert!(net.provenance_complete());
    net.mark_provenance_incomplete();
    let path = checkpoint_path("history");
    net.save(path.to_str().unwrap()).unwrap();
    let first = std::fs::read(&path).unwrap();
    let restored = Net::load_for_training(path.to_str().unwrap()).unwrap();
    assert_eq!(restored.observation_version(), ObservationVersion::SuitAwareV2);
    assert_eq!(restored.steps, net.steps);
    assert_eq!(restored.updates, 1);
    assert_eq!(restored.training_segments(), net.training_segments());
    assert_eq!(restored.next_training_hand(211).unwrap(), 9);
    assert!(!restored.provenance_complete());
    assert_eq!(restored.forward(&x, [true; ACTIONS]).prob, net.forward(&x, [true; ACTIONS]).prob);
    restored.save(path.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), first);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_training_history_is_rejected_without_mutation() {
    let mut net = Net::new(223);
    net.record_training(segment(7, 10, 5)).unwrap();
    let before = net.training_segments().to_vec();
    let mut cases = vec![segment(7, 14, 2), segment(8, u64::MAX, 1), segment(8, 0, 0)];
    let mut invalid = segment(8, 0, 1); invalid.learning_rate = f32::NAN; cases.push(invalid);
    let mut invalid = segment(8, 0, 1); invalid.rake_bps = 10_001; cases.push(invalid);
    let mut invalid = segment(8, 0, 1); invalid.rake_cap_units = -1; cases.push(invalid);
    let mut invalid = segment(7, 10, 1); invalid.kind = TrainingKind::Distillation; cases.push(invalid);
    for invalid in cases {
        assert!(net.record_training(invalid).is_err(), "accepted invalid segment {invalid:?}");
        assert_eq!(net.training_segments(), before.as_slice());
    }
}

fn refresh_checksum(bytes: &mut [u8]) {
    let end = bytes.len() - 8;
    let mut hash = 0xcbf29ce484222325_u64;
    for &byte in &bytes[..end] { hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3); }
    bytes[end..].copy_from_slice(&hash.to_le_bytes());
}

#[test]
fn checkpoint_rejects_corruption_trailing_data_and_wrong_schema() {
    let mut net = Net::new(227);
    net.record_training(segment(227, 0, 1)).unwrap();
    let path = checkpoint_path("corruption");
    net.save(path.to_str().unwrap()).unwrap();
    let valid = std::fs::read(&path).unwrap();
    let mut cases = Vec::new();
    let mut corrupt = valid.clone(); let last_weight = corrupt.len() - 9; corrupt[last_weight] ^= 1; cases.push(corrupt);
    let mut trailing = valid.clone(); trailing.push(0); cases.push(trailing);
    cases.push(valid[..valid.len() - 1].to_vec());
    // Recompute checksums: these must fail schema validation, not just integrity.
    for offset in [8, 12, 16, 20, 24] {
        let mut wrong = valid.clone(); wrong[offset..offset + 4].copy_from_slice(&999_u32.to_le_bytes());
        refresh_checksum(&mut wrong); cases.push(wrong);
    }
    for (offset, value) in [(48, 2), (53, 99), (92, 2)] {
        let mut wrong = valid.clone(); wrong[offset] = value; refresh_checksum(&mut wrong); cases.push(wrong);
    }
    let mut too_many = valid.clone(); too_many[49..53].copy_from_slice(&u32::MAX.to_le_bytes());
    refresh_checksum(&mut too_many); cases.push(too_many);
    let mut nonfinite = valid.clone(); nonfinite[44..48].copy_from_slice(&f32::NAN.to_le_bytes());
    refresh_checksum(&mut nonfinite); cases.push(nonfinite);
    for (i, invalid) in cases.iter().enumerate() {
        std::fs::write(&path, invalid).unwrap();
        assert!(Net::load(path.to_str().unwrap()).is_err(), "accepted malformed case {i}");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_late_sample_never_partially_updates_a_hand() {
    let x = [0.1; INPUT];
    let legal = [false, true, true, false, false, false];
    let mut net = smooth_net();
    let before = net.clone();
    let result = net.reinforce_hand(&[
        PolicySample { features: &x, legal, action: 1, reward: 1.0 },
        PolicySample { features: &x, legal, action: 0, reward: -1.0 },
    ], &[1.0, -1.0], 0.01);
    assert!(result.is_err());
    assert_eq!(net.w1, before.w1);
    assert_eq!(net.b1, before.b1);
    assert_eq!(net.w2, before.w2);
    assert_eq!(net.b2, before.b2);
    assert_eq!(net.baseline, before.baseline);
    assert_eq!(net.steps, before.steps);
    assert_eq!(net.updates, before.updates);
}

#[test]
fn zero_learning_rate_keeps_the_whole_batch_frozen() {
    let x = [0.1; INPUT];
    let mut net = smooth_net();
    let before = net.clone();
    net.reinforce_hand(&[PolicySample { features: &x, legal: [true; ACTIONS], action: 1, reward: 5.0 }], &[5.0], 0.0).unwrap();
    assert_eq!(net.w1, before.w1);
    assert_eq!(net.b1, before.b1);
    assert_eq!(net.w2, before.w2);
    assert_eq!(net.b2, before.b2);
    assert_eq!(net.baseline, before.baseline);
    assert_eq!(net.steps, before.steps);
    assert_eq!(net.updates, before.updates);
}

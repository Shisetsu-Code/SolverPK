//! Frozen evaluation against deliberately varied, public-information policies.
//! The behavioral references are simple heuristics, not calibrated poker bots.
use super::*;
use crate::neural::{ObservationVersion, TrainingSegment};

const DEAL_STRIDE: u64 = 0x9e3779b97f4a7c15;
const DEAL_STRIDE_INVERSE: u64 = 0xf1de83e19937733d;
// Rng::new replaces its unique zero mixed state with DEAL_STRIDE. These two
// raw seeds consequently produce the same RNG state and the same deck.
const RNG_ALIAS_A: u64 = 0x61c8864680b583eb;
const RNG_ALIAS_B: u64 = 0xcd36da00563c5ef6;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn deal_seed(seed: u64, group: u64) -> u64 {
    seed ^ group.wrapping_mul(DEAL_STRIDE)
}

fn recorded_index(seed: u64, training_seed: u64, start: u64, end: u64) -> Option<u64> {
    let index = (seed ^ training_seed).wrapping_mul(DEAL_STRIDE_INVERSE);
    if (start..end).contains(&index) { Some(index) } else { None }
}

fn validate_segments(segments: &[TrainingSegment], seed: u64, groups: usize) -> io::Result<()> {
    let raw: Vec<_> = segments.iter().map(|s| (s.seed, s.start_hand, s.hands)).collect();
    validate_raw_segments(&raw, seed, groups)
}

/// Validate an externally supplied (master seed, start index, count) manifest.
/// The caller binds that manifest to model hashes; this checks actual raw deals.
pub(super) fn validate_raw_segments(segments: &[(u64, u64, u64)], seed: u64, groups: usize) -> io::Result<()> {
    let groups = u64::try_from(groups).map_err(|_| invalid("too many evaluation clusters"))?;
    let mut intervals = Vec::with_capacity(segments.len());
    for &(training_seed, start, count) in segments {
        let end = start.checked_add(count).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "training segment hand interval overflows")
        })?;
        intervals.push((training_seed, start, end));
    }
    for group in 0..groups {
        let current = deal_seed(seed, group);
        let alias = match current {
            RNG_ALIAS_A => Some(RNG_ALIAS_B),
            RNG_ALIAS_B => Some(RNG_ALIAS_A),
            _ => None,
        };
        for &(training_seed, start, end) in &intervals {
            let overlap = recorded_index(current, training_seed, start, end)
                .or_else(|| alias.and_then(|other| recorded_index(other, training_seed, start, end)));
            if let Some(index) = overlap {
                return Err(invalid(format!(
                    "evaluation deal stream overlaps recorded training: eval_group={group} train_seed={training_seed} train_hand={index}"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_unique_stream(seed: u64, hands: usize) -> io::Result<()> {
    let count = u64::try_from(hands).map_err(|_| invalid("too many planned hands"))?;
    let a = (RNG_ALIAS_A ^ seed).wrapping_mul(DEAL_STRIDE_INVERSE);
    let b = (RNG_ALIAS_B ^ seed).wrapping_mul(DEAL_STRIDE_INVERSE);
    if a < count && b < count {
        return Err(invalid("planned evaluation deals overlap through the RNG fallback alias"));
    }
    Ok(())
}

/// Reject reuse of any recorded deterministic training deal, including streams
/// with different master seeds and the RNG's two equivalent raw seeds. Missing
/// historical provenance is disclosed by the caller; it is never called proven.
pub(super) fn validate_neural_eval_split(net: &Net, seed: u64, groups: usize) -> io::Result<()> {
    validate_segments(net.training_segments(), seed, groups)
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Heuristic { UniformLegal, CallingStation, TightAggressive, LooseAggressive, Nit }

impl Heuristic {
    fn name(self) -> &'static str {
        match self {
            Self::UniformLegal => "uniform_legal",
            Self::CallingStation => "calling_station",
            Self::TightAggressive => "tight_aggressive",
            Self::LooseAggressive => "loose_aggressive",
            Self::Nit => "nit",
        }
    }

    // This boundary deliberately accepts View, not Table6: future board cards
    // and other players' private cards cannot be consulted by a heuristic.
    fn probabilities(self, view: View) -> [f32; ACTIONS] {
        if matches!(self, Self::UniformLegal) {
            return masked_weights([1.0; ACTIONS], view.legal);
        }
        if matches!(self, Self::CallingStation) {
            return masked_weights([0.0, 1.0, 0.0, 0.0, 0.0, 0.0], view.legal);
        }
        let (enter, raise, bluff, weak_call, strong_raise) = match self {
            Self::TightAggressive => (0.43, 0.64, 0.035, 0.035, 0.82),
            Self::LooseAggressive => (0.27, 0.49, 0.16, 0.14, 0.92),
            Self::Nit => (0.59, 0.78, 0.005, 0.005, 0.70),
            _ => unreachable!(),
        };
        let price = view.call as f32 / (view.pot + view.call).max(1) as f32;
        let strength = visible_strength(view) - 0.40 * price
            + if view.on_button { 0.025 } else { 0.0 };
        let raising = view.legal[2..].iter().any(|&legal| legal);
        let (fold, call, aggressive) = if strength >= raise && raising {
            (0.0, 1.0 - strong_raise, strong_raise)
        } else if strength >= enter {
            let aggression = if raising { bluff * 1.5 } else { 0.0 };
            (if view.call > 0 { 0.035 } else { 0.0 }, 1.0 - aggression, aggression)
        } else if view.call == 0 {
            (0.0, 1.0 - bluff, bluff)
        } else {
            (1.0 - weak_call, weak_call, bluff * 0.15)
        };
        // Distinct bet sizing as well as distinct ranges. Sizes are the engine's
        // existing discrete legal actions, not a modification to its rules.
        let sizes = if matches!(self, Self::LooseAggressive) {
            [0.20, 0.35, 0.35, 0.10]
        } else {
            [0.45, 0.40, 0.13, 0.02]
        };
        let mut weights = [0.0; ACTIONS];
        weights[0] = fold;
        weights[1] = call;
        for action in 2..ACTIONS { weights[action] = aggressive * sizes[action - 2]; }
        masked_weights(weights, view.legal)
    }
}

fn masked_weights(weights: [f32; ACTIONS], legal: [bool; ACTIONS]) -> [f32; ACTIONS] {
    let mut result = std::array::from_fn(|action| if legal[action] { weights[action] } else { 0.0 });
    let sum = result.iter().sum::<f32>();
    if sum > 0.0 {
        for probability in &mut result { *probability /= sum; }
    } else if let Some(action) = (0..ACTIONS).find(|&action| legal[action]) {
        result[action] = 1.0;
    }
    result
}

/// An intentionally coarse strength cue, not an equity estimate. Every card
/// consumed here is known to this player at the decision being evaluated.
fn visible_strength(view: View) -> f32 {
    let a = view.hole[0] % 13 + 2;
    let b = view.hole[1] % 13 + 2;
    if view.visible == 0 {
        if a == b { return 0.55 + 0.40 * (a - 2) as f32 / 12.0; }
        let mut value = (a + b) as f32 / 28.0 * 0.50;
        if a.max(b) == 14 { value += 0.10; }
        if view.hole[0] / 13 == view.hole[1] / 13 { value += 0.06; }
        if a.abs_diff(b) <= 1 { value += 0.05; }
        if a.min(b) >= 10 { value += 0.07; }
        return value;
    }
    let mut cards = [0u8; 7];
    cards[..2].copy_from_slice(&view.hole);
    cards[2..2 + view.visible].copy_from_slice(&view.board[..view.visible]);
    let score = crate::evaluator::evaluate(&cards[..2 + view.visible]);
    let category = score >> 20;
    let mut value = match category {
        0 => 0.14 + 0.12 * a.max(b) as f32 / 14.0,
        1 => 0.36 + 0.18 * ((score >> 16) & 15) as f32 / 14.0,
        2 => 0.64,
        3 => 0.73,
        4 => 0.84,
        5 => 0.88,
        6 => 0.94,
        7 => 0.98,
        _ => 1.0,
    };
    if view.visible < 5 {
        let mut suits = [0u8; 4];
        let mut ranks = 0u16;
        for &card in &cards[..2 + view.visible] {
            suits[(card / 13) as usize] += 1;
            let rank = card % 13 + 2;
            ranks |= 1 << rank;
            if rank == 14 { ranks |= 1 << 1; }
        }
        if suits.iter().enumerate().any(|(suit, &count)| count == 4
            && view.hole.iter().any(|card| (*card / 13) as usize == suit)) {
            value += 0.10;
        }
        if (1..=10).any(|low| ((ranks >> low) & 0b11111).count_ones() == 4) {
            value += 0.06;
        }
    }
    value.min(1.0)
}

pub(super) enum RivalPolicy {
    Heuristic(Heuristic), Neural(Net), Tabular(Bot), Phh(FrozenPolicy),
    Equity(super::equity::EquityPolicy),
}

impl RivalPolicy {
    fn probabilities(&self, table: &Table6, player: usize) -> [f32; ACTIONS] {
        let view = table.view(player);
        match self {
            Self::Heuristic(policy) => policy.probabilities(view),
            Self::Equity(policy) => policy.probabilities(view),
            Self::Neural(net) => neural_prob(net, table, player),
            Self::Tabular(bot) => probs(&bot.table[state(view)], view.legal),
            Self::Phh(model) => {
                let coarse = [view.legal[0], view.legal[1], view.legal[2..].iter().any(|&value| value)];
                let raw = model.predict_multi(view.street as usize, view.hole, view.pot,
                    view.call, view.on_button, view.active, coarse).0;
                let sizes = [0.30, 0.35, 0.25, 0.10];
                let denominator = (2..ACTIONS).filter(|&action| view.legal[action])
                    .map(|action| sizes[action - 2]).sum::<f32>();
                let mut result = [0.0; ACTIONS];
                result[0] = raw[0]; result[1] = raw[1];
                if denominator > 0.0 {
                    for action in 2..ACTIONS {
                        if view.legal[action] { result[action] = raw[2] * sizes[action - 2] / denominator; }
                    }
                }
                masked_weights(result, view.legal)
            }
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Heuristic(_) => "toy_heuristic",
            Self::Equity(_) => "equity_mc_uniform_ranges",
            Self::Neural(_) => "frozen_neural",
            Self::Tabular(_) => "frozen_tabular_seat0",
            Self::Phh(_) => "frozen_historical_imitation",
        }
    }
}

fn play_frozen(candidate: &Net, opponent: &RivalPolicy, seed: u64, button: usize, rake: RakeConfig) -> Settlement {
    play_with(|table, player| neural_prob(candidate, table, player), opponent, seed, button, rake, STACK).0
}

pub(super) fn play_policies(candidate: &RivalPolicy, opponent: &RivalPolicy,
    seed: u64, button: usize, rake: RakeConfig, initial_stack: i64) -> (Settlement, RiskTelemetry) {
    play_with(|table, player| candidate.probabilities(table, player), opponent, seed, button, rake, initial_stack)
}

fn play_with(candidate: impl Fn(&Table6, usize) -> [f32; ACTIONS], opponent: &RivalPolicy,
    seed: u64, button: usize, rake: RakeConfig, initial_stack: i64) -> (Settlement, RiskTelemetry) {
    assert!((BB..=STACK).contains(&initial_stack));
    let mut table = Table6::new(seed, button);
    // Blinds, deck, and public state are unchanged; only the funded starting
    // stack varies. Observations keep their original 100 BB reference scale.
    for player in 0..N { table.stack[player] = initial_stack - table.total[player]; }
    let mut risk = RiskTelemetry { hero_max_single_action: table.total[0],
        hero_max_exposure: table.total[0], ..RiskTelemetry::default() };
    // Stable common random numbers across opponents; policy draws do not
    // consume deck randomness or another seat's action stream.
    let mut action_rng: [Rng; N] = std::array::from_fn(|player| {
        Rng::new(seed ^ 0x73a8c94ef021bd65
            ^ (button as u64).wrapping_mul(0xd1b54a32d192ed03)
            ^ (player as u64).wrapping_mul(0x94d049bb133111eb))
    });
    for street in 0..4 {
        if street > 0 {
            if table.alive() == 1 { break; }
            table.next_street().expect("league street progression");
        }
        let mut steps = 0;
        while !table.completed() {
            steps += 1;
            assert!(steps <= 200, "league betting loop");
            let player = table.actor;
            let distribution = if player == 0 { candidate(&table, player) }
                else { opponent.probabilities(&table, player) };
            let choice = draw(distribution, &mut action_rng[player]);
            let action = match choice {
                0 => Action::Fold,
                1 => Action::Call,
                action => Action::Raise(table.raise_to(player, action)),
            };
            let before = table.total[player];
            table.act(action).expect("league policy proposed an illegal action");
            if player == 0 {
                let paid = table.total[0] - before;
                risk.hero_max_single_action = risk.hero_max_single_action.max(paid);
                risk.hero_max_exposure = risk.hero_max_exposure.max(table.total[0]);
                if paid > 0 && table.stack[0] == 0 { risk.hero_allins += 1; }
            }
        }
        if table.alive() == 1 { break; }
    }
    (table.settle(rake), risk)
}

#[derive(Default)]
struct ClusterMoments { count: usize, mean: f64, m2: f64 }

impl ClusterMoments {
    fn add(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (value - self.mean);
    }

    fn result(&self) -> (f64, f64) {
        assert!(self.count >= 2);
        let variance = (self.m2 / (self.count - 1) as f64).max(0.0);
        let scale = 100.0 / N as f64;
        (self.mean * scale, student_t_critical(self.count - 1)
            * (variance / self.count as f64).sqrt() * scale)
    }
}

fn student_t_critical(df: usize) -> f64 {
    // Two-sided 95% Student-t quantiles, rounded upward. For larger df use
    // conservative step bounds; these are still approximate sampling CIs
    // because six-hand returns need not have a normal distribution.
    const SMALL: [f64; 30] = [
        12.707, 4.303, 3.183, 2.777, 2.571, 2.447, 2.365, 2.307, 2.263, 2.229,
        2.201, 2.179, 2.161, 2.145, 2.132, 2.120, 2.110, 2.101, 2.094, 2.086,
        2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.049, 2.046, 2.043,
    ];
    assert!(df > 0);
    if df <= 30 { SMALL[df - 1] }
    else if df <= 40 { 2.043 }
    else if df <= 60 { 2.022 }
    else if df <= 120 { 2.001 }
    else { 1.980 }
}

fn observation_name(net: &Net) -> &'static str {
    match net.observation_version() {
        ObservationVersion::LegacyV1 => "legacy_v1",
        ObservationVersion::SuitAwareV2 => "suit_aware_v2",
    }
}

fn split_status(candidate: &Net, opponent: &RivalPolicy) -> &'static str {
    let mut legacy = !candidate.provenance_complete()
        && candidate.observation_version() == ObservationVersion::LegacyV1;
    let mut untracked = !candidate.provenance_complete();
    match opponent {
        RivalPolicy::Neural(net) => {
            legacy |= !net.provenance_complete() && net.observation_version() == ObservationVersion::LegacyV1;
            untracked |= !net.provenance_complete();
        }
        RivalPolicy::Tabular(_) | RivalPolicy::Phh(_) => untracked = true,
        RivalPolicy::Heuristic(_) | RivalPolicy::Equity(_) => {}
    }
    if legacy { "unverified_legacy" }
    else if untracked { "unverified_external" }
    else { "recorded_streams_disjoint" }
}

fn values<'a>(args: &'a [String], flag: &str) -> io::Result<Vec<&'a str>> {
    let mut result = Vec::new();
    for (index, argument) in args.iter().enumerate() {
        if argument == flag {
            let value = args.get(index + 1).filter(|value| !value.starts_with("--"))
                .ok_or_else(|| invalid(format!("missing {flag} value")))?;
            result.push(value.as_str());
        }
    }
    Ok(result)
}

fn single_value<'a>(args: &'a [String], flag: &str) -> io::Result<Option<&'a str>> {
    super::neural_option(args,flag)
}

fn fingerprint(path: &str) -> io::Result<u64> {
    // Explicitly a reproducibility identifier, not a cryptographic checksum.
    let mut file = File::open(path)?;
    let mut hash = 0xcbf29ce484222325u64;
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { return Ok(hash); }
        for &byte in &buffer[..count] {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
}

struct ModelRecord { path: String, id: u64, observation: &'static str, steps: Option<u64> }
struct Rival { name: String, policy: RivalPolicy, record: Option<ModelRecord> }

fn print_model(role: &str, record: &ModelRecord) {
    println!("SIXMAX_LEAGUE_MODEL role={} file_fnv1a64={:016x} observation={} steps={} path={:?}",
        role, record.id, record.observation,
        record.steps.map(|steps| steps.to_string()).unwrap_or_else(|| "na".into()), record.path);
}

pub fn neural_league(args: &[String]) -> io::Result<()> {
    let rake = RakeConfig::from_args(args)?;
    let hands: usize = single_value(args, "--hands")?.unwrap_or("6000").parse()
        .map_err(|_| invalid("--hands must be an integer"))?;
    let seed: u64 = single_value(args, "--seed")?.unwrap_or("98722").parse()
        .map_err(|_| invalid("--seed must be a nonnegative 64-bit integer"))?;
    if hands < 2 * N || hands % N != 0 {
        return Err(invalid("--hands must be a multiple of six and at least 12 (two independent clusters)"));
    }
    let path = single_value(args, "--checkpoint")?.ok_or_else(|| invalid("missing --checkpoint"))?;
    let candidate = Net::load(path)?;
    let candidate_record = ModelRecord { path: path.into(), id: fingerprint(path)?,
        observation: observation_name(&candidate), steps: Some(candidate.steps) };
    let groups = hands / N;
    validate_neural_eval_split(&candidate, seed, groups)?;
    let mut rivals = Vec::new();
    for heuristic in [Heuristic::UniformLegal, Heuristic::CallingStation,
        Heuristic::TightAggressive, Heuristic::LooseAggressive, Heuristic::Nit] {
        rivals.push(Rival { name: heuristic.name().into(), policy: RivalPolicy::Heuristic(heuristic), record: None });
    }
    for (index, rival_path) in values(args, "--rival-checkpoint")?.into_iter().enumerate() {
        let net = Net::load(rival_path)?;
        validate_neural_eval_split(&net, seed, groups)?;
        let record = ModelRecord { path: rival_path.into(), id: fingerprint(rival_path)?,
            observation: observation_name(&net), steps: Some(net.steps) };
        rivals.push(Rival { name: format!("neural_{}", index + 1), policy: RivalPolicy::Neural(net), record: Some(record) });
    }
    if let Some(tabular_path) = single_value(args, "--tabular")? {
        rivals.push(Rival { name: "tabular".into(), policy: RivalPolicy::Tabular(load(tabular_path)?[0].clone()),
            record: Some(ModelRecord { path: tabular_path.into(), id: fingerprint(tabular_path)?, observation: "tabular", steps: None }) });
    }
    if let Some(phh_path) = single_value(args, "--phh-model")? {
        rivals.push(Rival { name: "phh_imitation".into(), policy: RivalPolicy::Phh(FrozenPolicy::load(phh_path)?),
            record: Some(ModelRecord { path: phh_path.into(), id: fingerprint(phh_path)?, observation: "historical_imitation", steps: None }) });
    }
    print_rules(rake);
    print_model("candidate", &candidate_record);
    for rival in &rivals {
        if let Some(record) = &rival.record { print_model(&rival.name, record); }
    }
    println!("SIXMAX_LEAGUE_PROTOCOL version=1 candidate_seat=0 opponents=5 button_rotations=6 shared_deals_across_rivals=true frozen=true seed_schedule=xor_stride_v1 action_rng=per_seat_button_v1 heuristic_quality=unvalidated ci_method=student_t_six_hand_cluster_approx ci_scope=per_rival_unadjusted comparison=descriptive");
    let started = Instant::now();
    let mut worst_name = "";
    let mut worst_mean = f64::INFINITY;
    let mut overall_split = "recorded_streams_disjoint";
    for rival in &rivals {
        let mut moments = ClusterMoments::default();
        let mut total_rake = 0i128;
        for group in 0..groups {
            let hand_seed = deal_seed(seed, group as u64);
            let mut cluster = 0.0;
            for button in 0..N {
                let settlement = play_frozen(&candidate, &rival.policy, hand_seed, button, rake);
                cluster += settlement.net[0] as f64 / BB_UNITS as f64;
                total_rake += settlement.rake as i128;
            }
            moments.add(cluster);
        }
        let (average, interval) = moments.result();
        let split = split_status(&candidate, &rival.policy);
        if split == "unverified_legacy" || (split == "unverified_external" && overall_split != "unverified_legacy") {
            overall_split = split;
        }
        println!("SIXMAX_LEAGUE_ROW opponent={} policy={} hands={} clusters={} bb100={:+.3} ci95_cluster_half={:.3} table_rake_bb100={:.5} eval_seed={} split={}",
            rival.name, rival.policy.kind(), hands, groups, average, interval,
            total_rake as f64 / BB_UNITS as f64 * 100.0 / hands as f64, seed, split);
        if average < worst_mean { worst_mean = average; worst_name = &rival.name; }
    }
    println!("SIXMAX_NEURAL_LEAGUE rivals={} hands_per_rival={} clusters_per_rival={} eval_seed={} worst_observed_rival={} worst_observed_bb100={:+.3} comparison=descriptive model_selection=none split={} elapsed={:.3}s",
        rivals.len(), hands, groups, seed, worst_name, worst_mean, overall_split, started.elapsed().as_secs_f64());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neural::TrainingKind;

    fn segment(seed: u64, start_hand: u64, hands: u64) -> TrainingSegment {
        TrainingSegment { kind: TrainingKind::SelfPlay, seed, start_hand, hands,
            learning_rate: 0.001, rake_bps: 0, rake_cap_units: 0, no_flop_no_drop: true }
    }

    #[test]
    fn split_guard_checks_shifted_intervals_and_every_evaluation_cluster() {
        let recorded = [segment(0, 1, 2)];
        assert!(validate_segments(&recorded, 0, 1).is_ok());
        assert!(validate_segments(&recorded, 0x9e3779b97f4a7c15, 1).is_err());
        assert!(validate_segments(&recorded, 0x3c6ef372fe94f82a, 1).is_err());
        assert!(validate_segments(&recorded, 0xdaa66d2c7ddf743f, 1).is_ok());
        assert!(validate_segments(&recorded, 0, 2).is_err(), "later evaluation deal was not checked");
        assert!(validate_segments(&[segment(7, u64::MAX, 2)], 0, 1).is_err());
    }

    #[test]
    fn split_guard_catches_both_directions_of_the_rng_fallback_alias() {
        assert_eq!(Rng::new(RNG_ALIAS_A).0, Rng::new(RNG_ALIAS_B).0);
        assert_eq!(deck(&mut Rng::new(RNG_ALIAS_A)), deck(&mut Rng::new(RNG_ALIAS_B)));
        assert!(validate_segments(&[segment(RNG_ALIAS_A, 0, 1)], RNG_ALIAS_B, 1).is_err());
        assert!(validate_segments(&[segment(RNG_ALIAS_B, 0, 1)], RNG_ALIAS_A, 1).is_err());
    }

    #[test]
    fn all_behavioral_policies_are_masked_and_normalized() {
        let table = Table6::new(7001, 0);
        for policy in [Heuristic::UniformLegal, Heuristic::CallingStation,
            Heuristic::TightAggressive, Heuristic::LooseAggressive, Heuristic::Nit] {
            for mask in 1u8..64 {
                let mut view = table.view(table.actor);
                view.legal = std::array::from_fn(|action| mask & (1 << action) != 0);
                let probabilities = policy.probabilities(view);
                assert!((probabilities.iter().sum::<f32>() - 1.0).abs() < 1e-6);
                for action in 0..ACTIONS {
                    assert!(probabilities[action].is_finite() && probabilities[action] >= 0.0);
                    if !view.legal[action] { assert_eq!(probabilities[action], 0.0); }
                }
            }
        }
    }

    #[test]
    fn weak_free_hands_distinguish_station_nit_tag_and_lag() {
        let table = Table6::new(7002, 0);
        let mut view = table.view(table.actor);
        view.hole = [0, 18]; // 72 offsuit.
        view.call = 0;
        view.legal = [false, true, true, true, true, true];
        let station = Heuristic::CallingStation.probabilities(view);
        let nit = Heuristic::Nit.probabilities(view);
        let tag = Heuristic::TightAggressive.probabilities(view);
        let lag = Heuristic::LooseAggressive.probabilities(view);
        assert_eq!(station[1], 1.0);
        assert!(station[1] > nit[1] && nit[1] > tag[1] && tag[1] > lag[1]);
        assert_eq!(Heuristic::UniformLegal.probabilities(view), [0.0, 0.2, 0.2, 0.2, 0.2, 0.2]);
    }

    #[test]
    fn heuristic_decisions_do_not_depend_on_hidden_cards() {
        for visible in [0, 3, 4] {
            let mut table = Table6::new(7003, 0);
            table.visible = visible;
            table.round = match visible { 0 => 0, 3 => 1, _ => 2 };
            let player = table.actor;
            let mut changed = table.clone();
            for other in 0..N {
                if other != player { changed.hole[other] = [51, 50]; }
            }
            for card in &mut changed.board[visible..] { *card = 49; }
            for policy in [Heuristic::UniformLegal, Heuristic::CallingStation,
                Heuristic::TightAggressive, Heuristic::LooseAggressive, Heuristic::Nit] {
                assert_eq!(policy.probabilities(table.view(player)), policy.probabilities(changed.view(player)));
            }
        }
    }

    #[test]
    fn cluster_intervals_use_six_hand_units_and_small_sample_uncertainty() {
        let mut moments = ClusterMoments::default();
        moments.add(6.0);
        moments.add(12.0);
        let (mean, half) = moments.result();
        assert!((mean - 150.0).abs() < 1e-9);
        assert!((half - 635.35).abs() < 1e-8);
        let mut constant = ClusterMoments::default();
        constant.add(-3.0); constant.add(-3.0);
        assert_eq!(constant.result(), (-50.0, 0.0));
    }

    #[test]
    fn frozen_league_games_repeat_and_conserve_after_rake() {
        let candidate = Net::new(7004);
        let opponents = [RivalPolicy::Heuristic(Heuristic::CallingStation),
            RivalPolicy::Heuristic(Heuristic::LooseAggressive), RivalPolicy::Neural(Net::new(7005))];
        let rake = RakeConfig { basis_points: 500, cap_units: 3 * BB_UNITS, no_flop_no_drop: true };
        for opponent in &opponents {
            for button in 0..N {
                let first = play_frozen(&candidate, opponent, 7006, button, rake);
                let second = play_frozen(&candidate, opponent, 7006, button, rake);
                assert_eq!(first, second);
                assert_eq!(first.net.iter().sum::<i64>(), -first.rake);
            }
        }
    }
}

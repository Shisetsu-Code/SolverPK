//! Frozen public-information benchmarks with a finite bankroll per session.
//! Each session owns a disjoint block of a single deterministic deal stream.
use super::*;
use super::bankroll::{Limits, Session, USD_UNITS};
use super::league::{Heuristic, RivalPolicy};
use std::collections::HashSet;

const STRIDE: u64 = 0x9e3779b97f4a7c15;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn validate_arguments(args: &[String]) -> io::Result<()> {
    const VALUES: &[&str] = &["--candidate-kind", "--checkpoint", "--opponent", "--sessions",
        "--hands", "--seed", "--stack-bb", "--equity-samples", "--bankroll-usd",
        "--stop-loss-usd", "--max-bet-usd", "--rake-bps", "--rake-cap-bb",
        "--phh-model", "--exclude-stream"];
    let mut seen = HashSet::new();
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        if !seen.insert(flag) && flag != "--exclude-stream" {
            return Err(invalid(format!("duplicate {flag}")));
        }
        if matches!(flag, "--sixmax-risk-benchmark" | "--rake-preflop") {
            index += 1;
        } else if VALUES.contains(&flag) {
            if args.get(index + 1).is_none_or(|value| value.starts_with("--")) {
                return Err(invalid(format!("missing {flag} value")));
            }
            index += 2;
        } else {
            return Err(invalid(format!("unknown risk benchmark argument {flag}")));
        }
    }
    Ok(())
}

fn excluded_streams(args: &[String]) -> io::Result<Vec<(u64, u64, u64)>> {
    let mut result = Vec::new();
    for index in 0..args.len() {
        if args[index] != "--exclude-stream" { continue; }
        let parts: Vec<_> = args[index + 1].split(':').collect();
        if parts.len() != 3 { return Err(invalid("--exclude-stream must be SEED:START:COUNT")); }
        let mut values = [0u64; 3];
        for i in 0..3 {
            values[i] = parts[i].parse().map_err(|_| invalid("invalid --exclude-stream integer"))?;
        }
        values[1].checked_add(values[2]).ok_or_else(|| invalid("--exclude-stream interval overflows"))?;
        result.push((values[0], values[1], values[2]));
    }
    Ok(result)
}

fn rate(net: i128, hands: u128, scale: i64) -> String {
    if hands == 0 { "NA".into() }
    else { format!("{:+.6}", 100.0 * net as f64 / (scale as f64 * hands as f64)) }
}

pub fn risk_benchmark(args: &[String]) -> io::Result<()> {
    validate_arguments(args)?;
    let rake = RakeConfig::from_args(args)?;
    let limits = Limits::from_args(args)?;
    let sessions: usize = neural_arg(args, "--sessions", 64)?;
    let hands: usize = neural_arg(args, "--hands", 2000)?;
    let seed: u64 = neural_arg(args, "--seed", 2026112007)?;
    let stack_bb: i64 = neural_arg(args, "--stack-bb", 100)?;
    let samples: usize = neural_arg(args, "--equity-samples", 96)?;
    let kind = neural_option(args, "--candidate-kind")?.unwrap_or("tabular");
    let rival_name = neural_option(args, "--opponent")?.unwrap_or("equity_multiway");
    if sessions == 0 || hands == 0 { return Err(invalid("sessions and hands must be positive")); }
    if !matches!(stack_bb, 50 | 100) { return Err(invalid("--stack-bb must be 50 or 100")); }
    if !(1..=4096).contains(&samples) { return Err(invalid("--equity-samples must be in 1..4096")); }
    if !matches!(kind, "tabular" | "neural") { return Err(invalid("--candidate-kind must be tabular or neural")); }
    if !matches!(rival_name, "equity_multiway" | "tight_aggressive" | "loose_aggressive" | "phh" | "clone") {
        return Err(invalid("unknown --opponent"));
    }
    let path = neural_option(args, "--checkpoint")?.ok_or_else(|| invalid("missing --checkpoint"))?;
    let phh_path = neural_option(args, "--phh-model")?;
    if rival_name == "phh" && phh_path.is_none() { return Err(invalid("missing --phh-model")); }
    if rival_name != "phh" && phh_path.is_some() { return Err(invalid("--phh-model requires --opponent phh")); }
    let planned = sessions.checked_mul(hands).ok_or_else(|| invalid("planned hand count overflows"))?;
    let exclusions = excluded_streams(args)?;
    league::validate_unique_stream(seed, planned)?;
    league::validate_raw_segments(&exclusions, seed, planned)?;
    let candidate = match kind {
        "tabular" => RivalPolicy::Tabular(load(path)?[0].clone()),
        _ => {
            let net = Net::load(path)?;
            league::validate_neural_eval_split(&net, seed, planned)?;
            RivalPolicy::Neural(net)
        }
    };
    let rival = match rival_name {
        "equity_multiway" => Some(RivalPolicy::Equity(equity::EquityPolicy::new(samples))),
        "tight_aggressive" => Some(RivalPolicy::Heuristic(Heuristic::TightAggressive)),
        "loose_aggressive" => Some(RivalPolicy::Heuristic(Heuristic::LooseAggressive)),
        "phh" => Some(RivalPolicy::Phh(FrozenPolicy::load(phh_path.unwrap())?)),
        _ => None,
    };
    let opponent = rival.as_ref().unwrap_or(&candidate);
    let initial_stack = stack_bb * BB;
    let buyin = initial_stack * MONEY_SCALE;
    print_rules(rake);
    println!("SIXMAX_RISK_PROTOCOL version=1 candidate_kind={} opponent={} frozen=true candidate_seat=0 stack_bb={} bb_usd=0.10 equity_samples={} equity_ranges=uniform_unvalidated seed_schedule=global_index_xor_stride_v1 action_rng=per_seat_button_v1 session_sampling=fresh_deal_each_hand excluded_streams={} model_selection=none rate_scope=realized_under_stopping_policy",
        kind, rival_name, stack_bb, samples, exclusions.len());
    let mut total_hands = 0u128;
    let mut total_profit = 0i128;
    let mut stopped = 0usize;
    for session_index in 0..sessions {
        let mut session = Session::new(limits);
        let offset = session_index * hands;
        let start_button = session_index % N;
        for hand_index in 0..hands {
            if !session.eligible_for_buyin(limits, buyin) { break; }
            // One global index, not nested XOR session/hand seeds: nested XOR
            // would duplicate (session=0, hand=1) and (session=1, hand=0).
            let deal_seed = seed ^ ((offset + hand_index) as u64).wrapping_mul(STRIDE);
            let (settlement, risk) = league::play_policies(&candidate, opponent, deal_seed,
                (start_button + hand_index % N) % N, rake, initial_stack);
            if settlement.net.iter().sum::<i64>() != -settlement.rake {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "settlement conservation failed"));
            }
            session.record_for_buyin(settlement.net[0], settlement.rake, &risk, buyin)?;
            if session.balance < session.initial - limits.stop_loss {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "session crossed the stop-loss floor"));
            }
        }
        let reached_horizon = session.hands as u128 == hands as u128;
        let can_continue = session.eligible_for_buyin(limits, buyin);
        if can_continue { session.stopped = Some("hands_limit"); } else { stopped += 1; }
        let profit = session.balance - session.initial;
        total_hands += session.hands as u128;
        total_profit += profit as i128;
        println!("SIXMAX_RISK_SESSION session={} seed={} hand_offset={} start_button={} hands_requested={} hands_played={} initial_units={} final_units={} profit_units={} buyin_units={} stop_loss_units={} max_wager_units={} max_drawdown_units={} rake_table_units={} hero_allin_actions={} hero_max_single_action_units={} hero_max_hand_exposure_units={} reached_horizon={} can_continue={} stop_reason={} bb100={} usd100={}",
            session_index, seed, offset, start_button, hands, session.hands, session.initial,
            session.balance, profit, buyin, limits.stop_loss, limits.max_bet, session.max_drawdown,
            session.total_rake, session.hero_allins, session.max_single_action * MONEY_SCALE,
            session.max_hand_exposure * MONEY_SCALE, reached_horizon, can_continue,
            session.stopped.unwrap(), rate(profit as i128, session.hands as u128, BB_UNITS),
            rate(profit as i128, session.hands as u128, USD_UNITS));
    }
    println!("SIXMAX_RISK_SUMMARY sessions={} hands_played={} profit_units={} stopped_sessions={} bb100={} usd100={}",
        sessions, total_hands, total_profit, stopped, rate(total_profit, total_hands, BB_UNITS),
        rate(total_profit, total_hands, USD_UNITS));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_frozen_runner_conserves_at_both_stack_depths_and_rakes() {
        let candidate = RivalPolicy::Neural(Net::new(1110));
        let rival = RivalPolicy::Equity(equity::EquityPolicy::new(8));
        for stack in [50 * BB, 100 * BB] {
            for rake in [RakeConfig::NONE,
                RakeConfig { basis_points: 500, cap_units: 3 * BB_UNITS, no_flop_no_drop: true }] {
                for button in 0..N {
                    let (result, risk) = league::play_policies(&candidate, &rival, 2026112019, button, rake, stack);
                    assert_eq!(result.net.iter().sum::<i64>(), -result.rake);
                    assert!(risk.hero_max_exposure <= stack && risk.hero_max_single_action <= stack);
                    assert!(result.net.iter().all(|&net| net >= -stack * MONEY_SCALE));
                }
            }
        }
    }

    #[test]
    fn contiguous_session_blocks_do_not_duplicate_raw_deals() {
        let seed = 2026112007;
        let horizon = 12;
        let mut seen = HashSet::new();
        for session in 0..64 {
            for hand in 0..horizon {
                let raw = seed ^ ((session * horizon + hand) as u64).wrapping_mul(STRIDE);
                assert!(seen.insert(Rng::new(raw).0));
            }
        }
        league::validate_unique_stream(seed, 64 * horizon).unwrap();
    }
}

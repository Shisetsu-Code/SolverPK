//! Export complete legal histories from the real engine for independent replay.
//! No copy of the production betting or settlement logic lives in this adapter.
use super::*;
use std::io::BufWriter;

#[test]
#[ignore = "writes histories for the pinned external Python engines"]
fn external_engine_export() {
    let path = std::env::var("SOLVERPK_EXTERNAL_HANDS_JSONL")
        .expect("set SOLVERPK_EXTERNAL_HANDS_JSONL");
    let hands: usize = std::env::var("SOLVERPK_EXTERNAL_HANDS")
        .unwrap_or_else(|_| "6000".into()).parse().unwrap();
    assert!(hands >= 12);
    let mut out = BufWriter::new(File::create(path).unwrap());
    let mut rng = Rng::new(0xabad_2026_1119_6000);
    let raked = RakeConfig { basis_points: 500, cap_units: 3 * BB_UNITS, no_flop_no_drop: true };
    let sizes = [2, 3, 5, 10, 25, 50, 75, 100, 150, 200, 400];
    let mut action_count = 0;
    for id in 0..hands {
        let seed = rng.next();
        let button = id % N;
        let initial = if id % 2 == 0 { [STACK; N] }
            else { std::array::from_fn(|_| sizes[rng.range(sizes.len())]) };
        let mut table = Table6::new(seed, button);
        table.stack = initial;
        table.street = [0; N]; table.total = [0; N]; table.pot = 0;
        table.pay(table.sb, 1); table.pay(table.bb, BB);
        write!(out, "{{\"id\":{id},\"seed\":{seed},\"button\":{button},\"initial\":{initial:?},\"hole\":{:?},\"board\":{:?},\"steps\":[", table.hole, table.board).unwrap();
        let mut first = true;
        let mut steps = 0;
        loop {
            while !table.completed() {
                steps += 1; assert!(steps < 500);
                let p = table.actor;
                let legal = table.legal(p);
                let allowed: Vec<usize> = (0..ACTIONS).filter(|&a| legal[a]).collect();
                let roll = rng.range(100);
                let choice = if roll < 45 { 1 } else if roll < 60 && legal[0] { 0 }
                    else { allowed[rng.range(allowed.len())] };
                let targets: [i64; 4] = std::array::from_fn(|a| table.raise_to(p, a + 2));
                if !first { write!(out, ",").unwrap(); } first = false;
                write!(out, "{{\"r\":{},\"p\":{},\"s\":{:?},\"b\":{:?},\"t\":{:?},\"f\":{:?},\"h\":{},\"m\":{},\"l\":{:?},\"to\":{:?},\"a\":{choice}}}",
                    table.round, p, table.stack, table.street, table.total, table.folded,
                    table.high, table.min_raise, legal, targets).unwrap();
                let action = match choice { 0 => Action::Fold, 1 => Action::Call,
                    a => Action::Raise(targets[a - 2]) };
                table.act(action).unwrap();
            }
            if table.alive() == 1 || table.round == 3 { break; }
            table.next_street().unwrap();
        }
        action_count += steps;
        let gross = table.settle(RakeConfig::NONE);
        let net = table.settle(raked);
        writeln!(out, "],\"total\":{:?},\"folded\":{:?},\"visible\":{},\"gross\":{{\"payouts\":{:?},\"net\":{:?},\"refunds\":{:?},\"rake\":{}}},\"raked\":{{\"payouts\":{:?},\"net\":{:?},\"refunds\":{:?},\"rake\":{}}}}}",
            table.total, table.folded, table.visible,
            gross.payouts, gross.net, gross.refunds, gross.rake,
            net.payouts, net.net, net.refunds, net.rake).unwrap();
    }
    out.flush().unwrap();
    println!("EXTERNAL_ENGINE_EXPORT hands={hands} actions={action_count} money_scale={MONEY_SCALE}");
}

#[test]
fn external_covered_opponent_cannot_fund_a_raise() {
    // UTG shoves twenty; every opponent except HJ has at most ten chips.
    // HJ may call or fold, but nobody can match an additional side-pot raise.
    let mut table = Table6::new(17, 0);
    table.stack = [2, 2, 2, 20, 200, 10];
    table.street = [0; N]; table.total = [0; N]; table.pot = 0;
    table.pay(table.sb, 1); table.pay(table.bb, BB);
    table.act(Action::Raise(20)).unwrap();
    assert_eq!(table.actor, 4);
    assert!(!table.legal(4)[2..].iter().any(|&allowed| allowed),
        "a raise cannot create a contested side pot when every opponent is covered");
}

fn fixed_action_bot(action: usize) -> Bot {
    let mut bot = Bot::new();
    for row in bot.table.iter_mut() {
        row.weights = [-80.0; ACTIONS];
        row.weights[1] = 0.0;
        row.weights[action] = 80.0;
    }
    bot
}

#[test]
fn external_telemetry_counts_allin_calls() {
    let mut bots = std::array::from_fn(|_| fixed_action_bot(0));
    bots[0] = fixed_action_bot(1);
    bots[1] = fixed_action_bot(5);
    let mut risk = RiskTelemetry::default();
    let mut actions = [0; ACTIONS];
    play_audited(&mut bots, 42, 3, 0.0, false, None, false,
        &mut actions, RakeConfig::NONE, Some(&mut risk));
    assert_eq!(risk.hero_max_exposure, STACK);
    assert_eq!(risk.hero_allins, 1, "calling off the stack is also an all-in");
}

#[test]
fn external_telemetry_counts_clipped_pot_raise_allin() {
    let mut bots = std::array::from_fn(|_| fixed_action_bot(1));
    bots[0] = fixed_action_bot(4);
    let mut risk = RiskTelemetry::default();
    let mut actions = [0; ACTIONS];
    play_audited(&mut bots, 42, 3, 0.0, false, None, false,
        &mut actions, RakeConfig::NONE, Some(&mut risk));
    assert_eq!(actions[5], 0, "test must not select explicit shove action");
    assert_eq!(risk.hero_max_exposure, STACK);
    assert_eq!(risk.hero_allins, 1);
}

#[test]
fn external_telemetry_includes_big_blind_without_a_decision() {
    let mut bots = std::array::from_fn(|_| fixed_action_bot(0));
    let mut risk = RiskTelemetry::default();
    let mut actions = [0; ACTIONS];
    play_audited(&mut bots, 42, 4, 0.0, false, None, false,
        &mut actions, RakeConfig::NONE, Some(&mut risk));
    assert_eq!(actions[0], 5);
    assert_eq!(risk.hero_max_exposure, BB);
    assert_eq!(risk.hero_max_single_action, BB);
    assert_eq!(risk.hero_allins, 0);
}

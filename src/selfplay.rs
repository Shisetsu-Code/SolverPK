//! Lightweight on-policy self-play arena; no external crates.
//! Agents use tabular softmax actor-critic, not a neural network or GTO solver.
use crate::{Rng, Table, BIG_BLIND};
use std::fs;
use std::io::{self, Write};
use std::time::Instant;

const STATES: usize = 4 * 5 * 4 * 4 * 2;
const ACTIONS: usize = 3; // fold, check/call, raise
const MAGIC: &[u8; 8] = b"SPKPOL01";

#[derive(Clone, Copy)]
struct Row {
    logits: [f32; ACTIONS],
    value: f32,
    seen: u32,
}
impl Default for Row {
    fn default() -> Self { Self { logits: [-0.6, 0.5, -0.2], value: 0.0, seen: 0 } }
}
#[derive(Clone)]
struct Agent {
    rows: [Row; STATES],
    hands: u64,
    net: i64,
}
impl Agent {
    fn new() -> Self { Self { rows: [Row::default(); STATES], hands: 0, net: 0 } }
}
struct Experience {
    state: usize,
    action: usize,
    probs: [f32; ACTIONS],
}
fn bucket(street: usize, equity: f64, odds: f64, pot: i64, position: usize) -> usize {
    let eq = ((equity * 5.0) as usize).min(4);
    let price = ((odds * 4.0) as usize).min(3);
    let pot_bucket = match pot { 0..=7 => 0, 8..=25 => 1, 26..=75 => 2, _ => 3 };
    ((((street * 5 + eq) * 4 + price) * 4 + pot_bucket) * 2) + position
}
fn probabilities(row: &Row, legal: [bool; ACTIONS]) -> [f32; ACTIONS] {
    let mut out = [0.0; ACTIONS];
    let max = (0..ACTIONS).filter(|&i| legal[i]).map(|i| row.logits[i]).fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0;
    for i in 0..ACTIONS {
        if legal[i] { out[i] = (row.logits[i] - max).exp(); sum += out[i]; }
    }
    for p in &mut out { *p /= sum; }
    out
}
fn sample(probs: [f32; ACTIONS], rng: &mut Rng) -> usize {
    let x = rng.next() as f64 / u64::MAX as f64;
    let mut running = 0.0;
    for (i, p) in probs.iter().enumerate() {
        running += *p as f64;
        if x < running { return i; }
    }
    (0..ACTIONS).rev().find(|&i| probs[i] > 0.0).unwrap_or(1)
}
fn learn(agent: &mut Agent, history: &[Experience], net: i64, alpha: f32) {
    agent.hands += 1;
    agent.net += net;
    // Terminal reward is clipped to prevent rare all-in outcomes dominating gradients.
    let reward = ((net as f32) / (BIG_BLIND as f32 * 20.0)).clamp(-4.0, 4.0);
    for event in history {
        let row = &mut agent.rows[event.state];
        let advantage = (reward - row.value).clamp(-4.0, 4.0);
        for a in 0..ACTIONS {
            let gradient = if event.action == a { 1.0 } else { 0.0 } - event.probs[a];
            row.logits[a] = (row.logits[a] + alpha * advantage * gradient).clamp(-8.0, 8.0);
        }
        row.value += 0.025 * (reward - row.value);
        row.seen = row.seen.saturating_add(1);
    }
}
fn play(
    agents: &mut [Agent], first: usize, second: usize, hand: usize,
    rng: &mut Rng, samples: usize, alpha: f32, verbose: bool,
) -> (i64, bool) {
    let mut table = Table::new(rng, hand % 2);
    let mut history: [Vec<Experience>; 2] = [Vec::new(), Vec::new()];
    let mut street_name = "preflop";
    if verbose { println!("Hand {}: agent {} vs agent {}", hand+1, first, second); }
    for street in 0..4 {
        if street > 0 {
            table.street = [0, 0];
            table.visible = match street { 1 => 3, 2 => 4, _ => 5 };
        }
        street_name = match street { 0 => "preflop", 1 => "flop", 2 => "turn", _ => "river" };
        let mut actor = if street == 0 { table.pos } else { 1-table.pos };
        let mut pending = 2usize;
        let mut minimum_raise = BIG_BLIND;
        let mut steps = 0usize;
        while pending > 0 && table.stack[0] > 0 && table.stack[1] > 0 {
            steps += 1;
            assert!(steps <= 100, "betting round stuck");
            let call = table.call_amount(actor);
            let max_extra = table.stack[actor].min(call + table.stack[1-actor]);
            let can_raise = max_extra > call && max_extra-call >= minimum_raise;
            let legal = [call>0, true, can_raise && steps < 20];
            let equity = table.equity(actor, samples, rng);
            let price = call as f64 / (table.pot()+call).max(1) as f64;
            let state = bucket(street, equity, price, table.pot(), (actor == table.pos) as usize);
            let index = if actor == 0 { first } else { second };
            let probs = probabilities(&agents[index].rows[state], legal);
            let action = sample(probs, rng);
            history[actor].push(Experience { state, action, probs });
            if action == 0 {
                table.folded = Some(actor);
                pending = 0;
            } else if action == 2 {
                let raise = (table.pot()/2).max(minimum_raise);
                let previous = table.street[1-actor];
                table.put(actor, (call+raise).min(max_extra));
                minimum_raise = table.street[actor] - previous;
                pending = 1;
            } else {
                table.put(actor, call);
                pending -= 1;
            }
            if verbose { println!("  {} p{} equity={:.2} action={} pot={}",street_name, actor,equity,action,table.pot()); }
            if table.folded.is_some() { break; }
            actor = 1-actor;
        }
        if table.folded.is_some() { break; }
    }
    // Settlement uses the same exact 7-card evaluator as the reference simulator.
    let (win, _, showdown) = if let Some(loser) = table.folded {
        let mut payouts = [0i64; 2];
        payouts[1-loser] = table.pot();
        (payouts,Some(1-loser),false)
    } else {
        let mut payouts = [0i64; 2];
        let matched = table.invested[0].min(table.invested[1]);
        for p in 0..2 { payouts[p] += table.invested[p] - matched; }
        let score = [0,1].map(|p| crate::score7(&[
            table.hole[p][0], table.hole[p][1],
            table.board[0], table.board[1], table.board[2], table.board[3], table.board[4],
        ]));
        if score[0] == score[1] { payouts[0] += matched; payouts[1] += matched; }
        else { payouts[if score[0]>score[1] {0} else {1}] += 2*matched; }
        (payouts,None,true)
    };
    let net = [win[0]-table.invested[0],win[1]-table.invested[1]];
    assert_eq!(net[0]+net[1],0,"chips not conserved");
    learn(&mut agents[first],&history[0],net[0],alpha);
    learn(&mut agents[second],&history[1],net[1],alpha);
    if verbose { println!("  terminal={} net={} / {}",street_name,net[0],net[1]); }
    (net[0],showdown)
}
fn persist(path: &str, agents: &[Agent]) -> io::Result<()> {
    let tmp = format!("{path}.tmp");
    let mut data = Vec::with_capacity(16+agents.len()*STATES*20);
    data.extend_from_slice(MAGIC);
    data.extend_from_slice(&(agents.len() as u32).to_le_bytes());
    for agent in agents {
        data.extend_from_slice(&agent.hands.to_le_bytes());
        data.extend_from_slice(&agent.net.to_le_bytes());
        for row in &agent.rows {
            for logit in row.logits { data.extend_from_slice(&logit.to_le_bytes()); }
            data.extend_from_slice(&row.value.to_le_bytes());
            data.extend_from_slice(&row.seen.to_le_bytes());
        }
    }
    fs::write(&tmp, data)?;
    fs::rename(tmp, path)
}
fn restore(path: &str) -> io::Result<Vec<Agent>> {
    let data = fs::read(path)?;
    let mut offset = 0usize;
    fn read<const N:usize>(data:&[u8], pos:&mut usize)->io::Result<[u8;N]> {
        let part = data.get(*pos..*pos+N)
            .ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"truncated checkpoint"))?;
        *pos += N;
        Ok(part.try_into().unwrap())
    }
    if &read::<8>(&data,&mut offset)? != MAGIC { return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid checkpoint")); }
    let count = u32::from_le_bytes(read::<4>(&data,&mut offset)?) as usize;
    if !(2..=256).contains(&count) { return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid agent count")); }
    let mut out = vec![Agent::new();count];
    for agent in &mut out {
        agent.hands = u64::from_le_bytes(read::<8>(&data,&mut offset)?);
        agent.net = i64::from_le_bytes(read::<8>(&data,&mut offset)?);
        for row in &mut agent.rows {
            for l in &mut row.logits { *l=f32::from_le_bytes(read::<4>(&data,&mut offset)?); }
            row.value=f32::from_le_bytes(read::<4>(&data,&mut offset)?);
            row.seen=u32::from_le_bytes(read::<4>(&data,&mut offset)?);
        }
    }
    if offset != data.len() || out.iter().flat_map(|a|a.rows.iter()).any(|r|r.logits.iter().any(|v|!v.is_finite()) || !r.value.is_finite()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData,"checkpoint contains invalid data"));
    }
    Ok(out)
}
pub fn run(args: &[String]) -> io::Result<()> {
    let hands: usize = super::arg(args,"--hands",10000);
    let rooms: usize = super::arg(args,"--rooms",64);
    let count: usize = super::arg(args,"--agents",8);
    let samples: usize = super::arg(args,"--samples",8);
    let seed: u64 = super::arg(args,"--seed",42);
    let alpha: f32 = super::arg(args,"--learning-rate",0.06);
    let path = args.windows(2).find(|w|w[0]=="--checkpoint").map(|w|w[1].as_str()).unwrap_or("selfplay.bin");
    let resume = args.iter().any(|a|a=="--resume");
    let verbose = args.iter().any(|a|a=="--verbose");
    if hands==0 || rooms==0 || count<2 || count>256 || samples==0 || !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid hands/rooms/agents/samples/learning-rate"));
    }
    let mut agents=if resume {restore(path)?}else{vec![Agent::new();count]};
    if resume && agents.len()!=count {return Err(io::Error::new(io::ErrorKind::InvalidInput,"agent count differs from checkpoint"));}
    let mut rng=Rng::new(seed);
    let start=Instant::now();
    let mut showdowns=0u64;
    let mut net_sum=0i64;
    // Rooms are logical tables interleaved in one CPU process; not hardware-parallel.
    for hand in 0..hands {
        let room = hand % rooms;
        let a = rng.range(agents.len());
        let mut b = rng.range(agents.len()-1);
        if b >= a {b+=1;}
        let (net,showdown)=play(&mut agents,a,b,hand+room,&mut rng,samples,alpha,verbose);
        net_sum+=net;
        showdowns+=showdown as u64;
        if (hand+1)%10000==0 { println!("played={} hands/s={:.1} showdowns={}",hand+1,(hand+1)as f64/start.elapsed().as_secs_f64().max(1e-9),showdowns); }
    }
    persist(path,&agents)?;
    println!("SELF-PLAY hands={} logical_rooms={} agents={} samples={} seed={} time={:.2}s throughput={:.1} hands/s",
        hands,rooms,agents.len(),samples,seed,start.elapsed().as_secs_f64(),hands as f64/start.elapsed().as_secs_f64().max(1e-9));
    println!("showdowns={} net_sum_for_first_seat={} checkpoint={}",showdowns,net_sum,path);
    for (i,a) in agents.iter().enumerate() {
        println!("agent {} hands={} net={} bb/100={:+.2} seen_states={}",
            i,a.hands,a.net,if a.hands>0 {a.net as f64/BIG_BLIND as f64*100.0/a.hands as f64}else{0.0},
            a.rows.iter().filter(|r|r.seen>0).count());
    }
    io::stdout().flush()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn valid_softmax() {
        let p=probabilities(&Row::default(),[false,true,false]);
        assert_eq!(p,[0.0,1.0,0.0]);
    }
    #[test] fn save_load() {
        let agents=vec![Agent::new(),Agent::new()];
        let path=std::env::temp_dir().join(format!("solverpk-test-{}.bin",std::process::id()));
        persist(path.to_str().unwrap(),&agents).unwrap();
        let restored=restore(path.to_str().unwrap()).unwrap();
        assert_eq!(restored.len(),2);
        std::fs::remove_file(path).unwrap();
    }
    #[test] fn self_play_conserves_chips() {
        let mut agents=vec![Agent::new(),Agent::new()];
        let mut rng=Rng::new(7);
        for i in 0..20 { play(&mut agents,0,1,i,&mut rng,2,0.01,false); }
        assert_eq!(agents[0].net+agents[1].net,0);
    }
}

mod selfplay;
use std::env;
use std::time::Instant;

const START_STACK: i64 = 200;
const SMALL_BLIND: i64 = 1;
const BIG_BLIND: i64 = 2;

#[derive(Clone)]
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self { Self(if seed == 0 { 0x9e3779b97f4a7c15 } else { seed }) }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12; x ^= x << 25; x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545f4914f6cdd1d)
    }
    fn range(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
    fn chance(&mut self, p: f64) -> bool { (self.next() as f64 / u64::MAX as f64) < p }
}
fn deck(rng: &mut Rng) -> [u8; 52] {
    let mut cards = [0; 52];
    for (i, card) in cards.iter_mut().enumerate() { *card = i as u8; }
    for i in (1..52).rev() { let j = rng.range(i + 1); cards.swap(i, j); }
    cards
}
fn rank(card: u8) -> usize { (card % 13) as usize + 2 }
fn suit(card: u8) -> usize { (card / 13) as usize }
fn score5(cards: &[u8; 5]) -> u64 {
    let mut count = [0u8; 15];
    let mut suits = [0u8; 4];
    for &c in cards { count[rank(c)] += 1; suits[suit(c)] += 1; }
    let flush = suits.contains(&5);
    let mut straight = 0;
    for h in (5..=14).rev() {
        if (0..5).all(|d| count[if h == 5 && d == 4 { 14 } else { h-d }] > 0) {
            straight = h; break;
        }
    }
    let mut groups = Vec::with_capacity(5);
    for r in (2..=14).rev() { if count[r] > 0 { groups.push((count[r], r)); } }
    groups.sort_by(|a,b| b.cmp(a));
    let category;
    let kick: Vec<usize>;
    if flush && straight > 0 { category = 8; kick = vec![straight]; }
    else if groups[0].0 == 4 { category = 7; kick = vec![groups[0].1, groups[1].1]; }
    else if groups[0].0 == 3 && groups[1].0 == 2 { category = 6; kick = vec![groups[0].1, groups[1].1]; }
    else if flush { category = 5; kick = (2..=14).rev().filter(|&r| count[r]>0).collect(); }
    else if straight > 0 { category = 4; kick = vec![straight]; }
    else if groups[0].0 == 3 {
        category = 3;
        kick = std::iter::once(groups[0].1).chain(groups[1..].iter().map(|g|g.1)).collect();
    } else if groups[0].0 == 2 && groups[1].0 == 2 {
        category = 2;
        kick = vec![groups[0].1, groups[1].1, groups[2].1];
    } else if groups[0].0 == 2 {
        category = 1;
        kick = std::iter::once(groups[0].1).chain(groups[1..].iter().map(|g|g.1)).collect();
    } else { category = 0; kick = groups.iter().map(|g|g.1).collect(); }
    let mut result = category as u64;
    for i in 0..5 { result = (result << 4) | *kick.get(i).unwrap_or(&0) as u64; }
    result
}
fn score7(cards: &[u8; 7]) -> u64 {
    let mut best = 0;
    for a in 0..3 { for b in a+1..4 { for c in b+1..5 {
        for d in c+1..6 { for e in d+1..7 {
            best = best.max(score5(&[cards[a],cards[b],cards[c],cards[d],cards[e]]));
        } }
    } } }
    best
}
#[derive(Default, Clone, Copy)]
struct Stats { won: u64, folds: u64, showdowns: u64, net: i64, actions: u64 }
struct Table {
    hole: [[u8; 2]; 2],
    board: [u8; 5],
    deck: [u8; 52],
    pos: usize,
    visible: usize,
    stack: [i64; 2],
    street: [i64; 2],
    invested: [i64; 2],
    folded: Option<usize>,
    actions: [u64; 2],
}
impl Table {
    fn new(rng: &mut Rng, sb: usize) -> Self {
        let d = deck(rng);
        let mut t = Self {
            hole: [[d[0],d[2]], [d[1],d[3]]],
            board: [d[4],d[5],d[6],d[7],d[8]],
            deck: d, pos: sb, visible: 0,
            stack: [START_STACK; 2], street: [0; 2], invested: [0; 2],
            folded: None, actions: [0; 2],
        };
        t.put(sb, SMALL_BLIND);
        t.put(1-sb, BIG_BLIND);
        t
    }
    fn put(&mut self, player: usize, amount: i64) {
        let n = amount.max(0).min(self.stack[player]);
        self.stack[player] -= n;
        self.street[player] += n;
        self.invested[player] += n;
    }
    fn pot(&self) -> i64 { self.invested[0] + self.invested[1] }
    fn call_amount(&self, p: usize) -> i64 { (self.street[1-p] - self.street[p]).max(0) }
    fn equity(&self, p: usize, samples: usize, rng: &mut Rng) -> f64 {
        let mut known = [false; 52];
        for &c in &self.hole[p] { known[c as usize] = true; }
        for &c in &self.board[..self.visible] { known[c as usize] = true; }
        let unknown: Vec<u8> = (0..52).filter(|&c| !known[c]).map(|c|c as u8).collect();
        let missing = 5 - self.visible;
        let mut points = 0.0;
        for _ in 0..samples {
            let mut pool = unknown.clone();
            for k in 0..missing+2 {
                let j = k + rng.range(pool.len()-k);
                pool.swap(k,j);
            }
            let opp = [pool[0], pool[1]];
            let mut b = self.board;
            b[self.visible..].copy_from_slice(&pool[2..2+missing]);
            let ours = score7(&[self.hole[p][0],self.hole[p][1],b[0],b[1],b[2],b[3],b[4]]);
            let theirs = score7(&[opp[0],opp[1],b[0],b[1],b[2],b[3],b[4]]);
            points += if ours > theirs { 1.0 } else if ours == theirs { 0.5 } else { 0.0 };
        }
        points / samples as f64
    }
    fn betting(&mut self, first: usize, rng: &mut Rng, simulations: usize, verbose: bool) {
        // Heads-up: each player acts once unless a bet/raise reopens action.
        let mut actor = first;
        let mut pending = 2usize;
        let mut min_raise = BIG_BLIND;
        let mut steps = 0;
        while pending > 0 && self.stack[0] > 0 && self.stack[1] > 0 {
            steps += 1;
            assert!(steps < 1000, "betting round did not terminate");
            let call = self.call_amount(actor);
            let max_extra = self.stack[actor].min(call + self.stack[1-actor]);
            let equity = self.equity(actor, simulations, rng);
            let pot = self.pot();
            let odds = call as f64 / (pot+call).max(1) as f64;
            let tight = actor == 0;
            let fold = call > 0 && equity + if tight { 0.025 } else { 0.06 } < odds;
            let want_raise = if tight {
                equity > if call > 0 { 0.76 } else { 0.65 }
            } else {
                equity > if call > 0 { 0.69 } else { 0.56 } ||
                    (equity < 0.43 && rng.chance(0.12))
            };
            let raise_size = (pot / 2).max(min_raise);
            let can_raise = max_extra > call && max_extra - call >= min_raise;
            let action;
            if fold {
                self.folded = Some(actor);
                action = "fold".to_string();
            } else if want_raise && can_raise && steps < 20 {
                let extra = (call + raise_size).min(max_extra);
                let previous_high = self.street[1-actor];
                self.put(actor, extra);
                let increment = self.street[actor] - previous_high;
                min_raise = increment;
                pending = 1;
                action = format!("raise-to {}", self.street[actor]);
            } else {
                self.put(actor, call.min(max_extra));
                pending -= 1;
                action = if call == 0 { "check".to_string() } else { "call".to_string() };
            }
            self.actions[actor] += 1;
            if verbose { println!("  Bot{} equity={:.3} pot={} -> {}", actor, equity, pot, action); }
            if self.folded.is_some() { break; }
            actor = 1-actor;
        }
    }
    fn play(&mut self, rng: &mut Rng, simulations: usize, verbose: bool) -> ([i64;2], Option<usize>, bool) {
        if verbose {
            println!("Hole: bot0 {} {}, bot1 {} {}", fmt(self.hole[0][0]),fmt(self.hole[0][1]),fmt(self.hole[1][0]),fmt(self.hole[1][1]));
        }
        for street in 0..4 {
            if street > 0 {
                self.street = [0; 2];
                self.visible = match street { 1=>3, 2=>4, _=>5 };
                if verbose {
                    println!("Board: {}",self.board[..self.visible].iter().map(|&c|fmt(c)).collect::<Vec<_>>().join(" "));
                }
            }
            self.betting(if street==0 { self.pos } else { 1-self.pos }, rng, simulations, verbose);
            if self.folded.is_some() { break; }
        }
        let mut winnings = [0i64; 2];
        if let Some(loser) = self.folded {
            winnings[1-loser] = self.pot();
            return (winnings, Some(1-loser), false);
        }
        // Refund unmatched contributions; keep invested unchanged for net accounting.
        for p in 0..2 {
            if self.invested[p] > self.invested[1-p] {
                let refund = self.invested[p] - self.invested[1-p];
                winnings[p] += refund;
            }
        }
        let pot = 2 * self.invested[0].min(self.invested[1]);
        let scores = [0,1].map(|p|score7(&[
            self.hole[p][0],self.hole[p][1],self.board[0],self.board[1],
            self.board[2],self.board[3],self.board[4]
        ]));
        let winner = if scores[0] > scores[1] { Some(0) }
            else if scores[1] > scores[0] { Some(1) } else { None };
        if let Some(p) = winner { winnings[p] += pot; }
        else {
            winnings[0] += pot / 2;
            winnings[1] += pot - pot/2;
        }
        (winnings,winner,true)
    }
}
fn fmt(card: u8) -> String {
    let r = b"23456789TJQKA"[(card%13) as usize] as char;
    let s = b"cdhs"[(card/13) as usize] as char;
    format!("{r}{s}")
}
fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.windows(2).find(|w|w[0]==flag).and_then(|w|w[1].parse().ok()).unwrap_or(default)
}
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a|a=="--help" || a=="-h") {
        println!("solverpk --hands 10000 --seed 42 --samples 64 --verbose");
        println!("solverpk --self-play --hands 10000 --rooms 64 --agents 8 --samples 8 --checkpoint selfplay.bin");
        println!("solverpk --self-play --resume --hands 10000 --rooms 64 --agents 8 --checkpoint selfplay.bin");
        println!("Bot0: tight/aggressive. Bot1: loose/aggressive with occasional bluffs.");
        println!("Heads-up cash simulation, 100 BB stacks, no rake. Every hand resets stacks.");
        return;
    }
    if args.iter().any(|a| a=="--self-play") {
        if let Err(e)=selfplay::run(&args) {eprintln!("Self-play error: {e}"); std::process::exit(2);}
        return;
    }
    let hands: usize = arg(&args,"--hands",1000);
    let seed: u64 = arg(&args,"--seed",42);
    let samples: usize = arg(&args,"--samples",64);
    let verbose = args.iter().any(|a|a=="--verbose");
    if hands==0 || samples==0 { eprintln!("--hands and --samples must be positive"); std::process::exit(2); }
    let mut shuffle_rng=Rng::new(seed);
    let mut bot_rng=Rng::new(seed ^ 0xd1b54a32d192ed03);
    let mut stats=[Stats::default(); 2];
    let start=Instant::now();
    for hand in 0..hands {
        let mut t=Table::new(&mut shuffle_rng,hand%2);
        if verbose { println!("Hand {} (SB=bot{})",hand+1,hand%2); }
        let (win,winner,showdown)=t.play(&mut bot_rng,samples,verbose);
        let net=[0,1].map(|p|win[p]-t.invested[p]);
        // A refund may add to winnings but invested already excludes the refunded amount.
        for p in 0..2 {
            stats[p].net += net[p];
            stats[p].actions += t.actions[p];
            if winner==Some(p) {stats[p].won+=1;}
            if showdown {stats[p].showdowns+=1;}
            if t.folded==Some(p) {stats[p].folds+=1;}
        }
        assert_eq!(net[0]+net[1],0,"chip conservation");
        if verbose { println!("Net: bot0={} bot1={}\n",net[0],net[1]); }
    }
    let secs=start.elapsed().as_secs_f64();
    println!("Hands: {hands} | Seed: {seed} | MC samples/action: {samples}");
    println!("Elapsed: {:.3}s | Throughput: {:.1} hands/s",secs,hands as f64/secs.max(1e-9));
    for p in 0..2 {
        let s=stats[p];
        let bb100=s.net as f64 / BIG_BLIND as f64 * 100.0 / hands as f64;
        println!("Bot{p}: net={} chips | {:+.3} bb/100 | wins={} | folds={} | showdowns={} | actions={}",
            s.net,bb100,s.won,s.folds,s.showdowns,s.actions);
    }
    println!("Note: Monte Carlo equity versus random hands is not a GTO solver.");
}
#[cfg(test)]
mod tests {
    use super::*;
    fn c(r:usize,s:usize)->u8 { (s*13+r-2) as u8 }
    #[test] fn straight_flush_beats_quads() {
        let sf=score5(&[c(10,0),c(11,0),c(12,0),c(13,0),c(14,0)]);
        let quad=score5(&[c(9,0),c(9,1),c(9,2),c(9,3),c(14,0)]);
        assert!(sf>quad);
    }
    #[test] fn wheel_is_straight() {
        let wheel=score5(&[c(14,0),c(2,1),c(3,2),c(4,0),c(5,3)]);
        let pair=score5(&[c(14,0),c(14,1),c(9,2),c(4,0),c(5,3)]);
        assert!(wheel>pair);
    }
    #[test] fn seven_card_best_five() {
        let cards=[c(14,0),c(13,0),c(12,0),c(11,0),c(10,0),c(2,1),c(3,2)];
        assert_eq!(score7(&cards),score5(&[cards[0],cards[1],cards[2],cards[3],cards[4]]));
    }
    #[test] fn deterministic_game_and_chip_conservation() {
        let mut a=Rng::new(123);
        let mut b=Rng::new(456);
        for i in 0..50 {
            let mut table=Table::new(&mut a,i%2);
            let (win,_,_)=table.play(&mut b,8,false);
            assert_eq!(win[0]+win[1]-table.pot(),0);
        }
    }
}

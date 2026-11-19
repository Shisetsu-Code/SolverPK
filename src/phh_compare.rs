//! Frozen self-play policies vs historical PHH decisions (not counterfactual EV).
//! Evaluates only Pluribus actions by default; supports --all-actors diagnostic.
//! The 6-max -> heads-up mismatch is tracked explicitly.
use crate::{Rng,Table};
use crate::selfplay::HistoricalPolicy;
use std::fs::File;
use std::io::{self,Read,BufReader,Write};
use std::time::Instant;
const RECORD:usize=96;
fn invalid(s:&str)->io::Error {io::Error::new(io::ErrorKind::InvalidData,s)}
fn val(r:&[u8;RECORD],p:usize)->i64 {
    i64::from_le_bytes(r[p..p+8].try_into().unwrap())
}
#[derive(Default,Clone)]
struct Acc {
    n:u64,correct:u64,log_loss:f64,uniform_loss:f64,mc_loss:f64,mc_correct:u64,
    by_street:[u64;4],
    buckets:[u64;3],
}
impl Acc {
    fn add(&mut self,action:usize,p:[f32;3],legal:[bool;3],street:usize,mc:usize){
        self.n+=1;
        self.by_street[street]+=1;
        self.buckets[action]+=1;
        self.log_loss-=(p[action] as f64).max(1e-12).ln();
        self.uniform_loss+=(legal.iter().filter(|&&v|v).count() as f64).ln();
        self.mc_loss-=if action==mc {(0.98f64).ln()} else {(0.01f64).ln()};
        let best=(0..3).filter(|&a|legal[a]).max_by(|&a,&b|p[a].total_cmp(&p[b])).unwrap();
        self.correct+=u64::from(best==action);
        self.mc_correct+=u64::from(mc==action);
    }
    fn report(&self,label:&str){
        let n=self.n.max(1) as f64;
        println!("HISTORICAL model={} decisions={} accuracy={:.5} log_loss={:.5} uniform_legal_log_loss={:.5} mc_accuracy={:.5} action_counts={:?} streets={:?}",
            label,self.n,self.correct as f64/n,self.log_loss/n,self.uniform_loss/n,
            self.mc_correct as f64/n,self.buckets,self.by_street);
    }
}
pub fn run(args:&[String])->io::Result<()>{
    let source=args.windows(2).find(|w|w[0]=="--source").map(|w|w[1].as_str()).ok_or_else(||invalid("missing --source"))?;
    let left=args.windows(2).find(|w|w[0]=="--model-10m").map(|w|w[1].as_str()).ok_or_else(||invalid("missing --model-10m"))?;
    let right=args.windows(2).find(|w|w[0]=="--model-20m").map(|w|w[1].as_str()).ok_or_else(||invalid("missing --model-20m"))?;
    let samples:usize=super::arg(args,"--samples",32);
    let seed:u64=super::arg(args,"--seed",4157);
    let all=args.iter().any(|x|x=="--all-actors");
    if samples==0{return Err(invalid("--samples must be positive"));}
    let policies=[HistoricalPolicy::load(left)?,HistoricalPolicy::load(right)?];
    let mut report=[Acc::default(),Acc::default()];
    let file=File::open(source)?;
    let mut reader=BufReader::new(file);
    let mut magic=[0u8;8];reader.read_exact(&mut magic)?;
    if &magic!=b"SPKOBS02"{return Err(invalid("incorrect observation version"));}
    let start=Instant::now();
    let mut total=0u64;let mut filtered=0u64;let mut missing=0u64;
    let mut mc_seed=seed;
    loop{
        let mut r=[0u8;RECORD];
        if reader.read(&mut r[0..1])?==0{break;}
        reader.read_exact(&mut r[1..])?;
        total+=1;
        if !all && r[88]!=1{continue;}
        let action=r[12] as usize;
        let visible=match r[11] {0=>0usize,1=>3,2=>4,3=>5,_=>return Err(invalid("invalid street"))};
        let cards=[r[16],r[17]];
        if cards.iter().any(|&c|c>=52) {missing+=1;continue;}
        let board=&r[18..18+visible];
        if board.iter().any(|&c|c>=52){missing+=1;continue;}
        let pot=val(&r,24);
        let call=val(&r,32);
        let blind=u32::from_le_bytes(r[89..93].try_into().unwrap()) as i64;
        if blind<=0 {return Err(invalid("missing blind scale"));}
        let scale=|amount:i64|((amount as f64/blind as f64)*2.0).round() as i64;
        let pot_scaled=scale(pot).max(1);
        let call_scaled=scale(call).max(0);
        let legal=std::array::from_fn(|a|r[15]&(1<<a)!=0);
        if action>=3 || !legal[action]{return Err(invalid("illegal historical action"));}
        // Monte Carlo uses ONLY acting player's hole cards and visible board,
        // against one uniformly random opposing hand; no opponent-card leakage.
        let mut deal_rng=Rng::new(123);
        let mut table=Table::new(&mut deal_rng,0);
        table.hole[0]=cards;
        table.visible=visible;
        for (i,&c) in board.iter().enumerate(){table.board[i]=c;}
        mc_seed=mc_seed.wrapping_add(0x9e3779b97f4a7c15);
        let hand_id=u64::from_le_bytes(r[0..8].try_into().unwrap());
        let ord=u16::from_le_bytes(r[8..10].try_into().unwrap());
        let mut rng=Rng::new(mc_seed^hand_id.wrapping_mul(0xbf58476d1ce4e5b9)^(ord as u64));
        let equity=table.equity(0,samples,&mut rng);
        let odds=call as f64/(pot+call).max(1) as f64;
        let mc=if legal[0] && equity+0.03<odds{0}else if legal[2] && equity>if call>0{0.72}else{0.61}{2}else{1};
        for i in 0..2 {
            let probs=policies[i].predict(r[11] as usize,equity,pot_scaled,call_scaled,r[10]==r[14],legal);
            report[i].add(action,probs,legal,r[11] as usize,mc);
        }
        filtered+=1;
    }
    println!("HISTORICAL_SOURCE total_decisions={} selected={} skipped_unknown={} filter={} samples={} elapsed={:.3}s",
        total,filtered,missing,if all{"all"}else{"Pluribus"},samples,start.elapsed().as_secs_f64());
    report[0].report("selfplay10M");report[1].report("selfplay20M");
    if filtered==0{return Err(invalid("no eligible historical decisions"));}
    if report[0].log_loss<report[1].log_loss {
        println!("HISTORICAL lower_negative_log_loss=selfplay10M");
    } else {
        println!("HISTORICAL lower_negative_log_loss=selfplay20M");
    }
    println!("HISTORICAL_NOTE action agreement is not poker EV, GTO exploitability, or head-to-head winrate; 6-max contexts differ from Heads-Up training");
    io::stdout().flush()
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn record_amount_scale(){
        let blind=10000i64;
        assert_eq!(((30000f64/blind as f64)*2.0).round() as i64,6);
    }
}

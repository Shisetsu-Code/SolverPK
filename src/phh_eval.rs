//! Frozen PHH imitation versus the Monte Carlo analysis bot, Heads-Up only.
//! Mirrored hole cards and button rotate together, with fixed deal seeds and
//! no policy updates. The PHH model was learned from six-player poker.
use crate::{BIG_BLIND,Rng,Table,score7};
use crate::imitation::FrozenPolicy;
use std::io;
use std::time::Instant;

#[derive(Clone,Copy)]
enum Candidate { Phh, Uniform, MonteCarlo }
fn sample(probabilities:[f32;3],rng:&mut Rng)->usize{
    let x=rng.next() as f64/u64::MAX as f64;
    let mut sum=0.0;
    for (i,p) in probabilities.iter().enumerate(){
        sum+=*p as f64;
        if x<sum{return i;}
    }
    (0..3).rev().find(|&i|probabilities[i]>0.0).unwrap_or(1)
}
fn mc_action(table:&Table, actor:usize, samples:usize, rng:&mut Rng,
             legal:[bool;3], call:i64)->[f32;3]{
    let eq=table.equity(actor,samples,rng);
    let odds=call as f64/(table.pot()+call).max(1) as f64;
    let fold=legal[0] && eq+0.03<odds;
    let raise=legal[2] && eq>if call>0{0.72}else{0.61};
    let choice=if fold {0}else if raise{2}else{1};
    std::array::from_fn(|a|if a==choice{1.0}else{0.0})
}
fn run_hand(
    policy:&FrozenPolicy,mode:Candidate,pair_seed:u64,seat_rotation:usize,
    samples:usize,
)->(i64,u64,u64){
    let mut deal_rng=Rng::new(pair_seed);
    let mut table=Table::new(&mut deal_rng,seat_rotation);
    // Opposite seat receives the same cards in the other half of a pair.
    if seat_rotation==1 {table.hole.swap(0,1);}
    let mut decision_rng=Rng::new(pair_seed ^ 0xf5bf35a081e94327 ^ seat_rotation as u64);
    let mut seen=0u64;let mut total=0u64;
    for street in 0..4 {
        if street>0{
            table.street=[0;2];
            table.visible=match street {1=>3,2=>4,_=>5};
        }
        let mut actor=if street==0{table.pos}else{1-table.pos};
        let mut pending=2usize;let mut min_raise=BIG_BLIND;let mut steps=0;
        while pending>0{
            if table.stack[actor]==0{break;}
            if table.stack[1-actor]==0 && table.call_amount(actor)==0{break;}
            steps+=1;assert!(steps<=100,"unbounded round");
            let call=table.call_amount(actor);
            let max_extra=table.stack[actor].min(call+table.stack[1-actor]);
            let can_raise=max_extra>call && max_extra-call>=min_raise && steps<20;
            let legal=[call>0,true,can_raise];
            let probs=if actor==0 {
                match mode{
                    Candidate::Phh=>{
                        let (p,covered)=policy.predict(street,table.hole[actor],
                            table.pot(),call,table.pos==actor,legal);
                        seen+=covered as u64;total+=1;p
                    }
                    Candidate::Uniform=>{
                        let n=legal.iter().filter(|&&x|x).count() as f32;
                        std::array::from_fn(|i|if legal[i]{1.0/n}else{0.0})
                    }
                    Candidate::MonteCarlo=>mc_action(&table,actor,samples,&mut decision_rng,legal,call),
                }
            }else{mc_action(&table,actor,samples,&mut decision_rng,legal,call)};
            let action=sample(probs,&mut decision_rng);
            if action==0 {
                table.folded=Some(actor);break;
            }else if action==2 {
                let raise=(table.pot()/2).max(min_raise);
                let high=table.street[1-actor];
                table.put(actor,(call+raise).min(max_extra));
                min_raise=table.street[actor]-high;
                pending=1;
            }else {
                table.put(actor,call);
                pending-=1;
            }
            actor=1-actor;
        }
        if table.folded.is_some(){break;}
    }
    let win=if table.folded==Some(0){
        -table.invested[0]
    }else if table.folded==Some(1){
        table.invested[1]
    }else{
        let a=score7(&[table.hole[0][0],table.hole[0][1],table.board[0],
            table.board[1],table.board[2],table.board[3],table.board[4]]);
        let b=score7(&[table.hole[1][0],table.hole[1][1],table.board[0],
            table.board[1],table.board[2],table.board[3],table.board[4]]);
        let matched=table.invested[0].min(table.invested[1]);
        if a>b {matched}
        else if b>a{-matched}
        else {0}
    };
    (win,seen,total)
}
pub fn run(args:&[String])->io::Result<()>{
    let path=args.windows(2).find(|w|w[0]=="--model").map(|w|w[1].as_str())
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --model"))?;
    let mode=match args.windows(2).find(|w|w[0]=="--candidate").map(|w|w[1].as_str()).unwrap_or("phh"){
        "phh"=>Candidate::Phh,"uniform"=>Candidate::Uniform,"mc"=>Candidate::MonteCarlo,
        _=>return Err(io::Error::new(io::ErrorKind::InvalidInput,"candidate must be phh, uniform, or mc")),
    };
    let hands:usize=super::arg(args,"--hands",100000);
    let samples:usize=super::arg(args,"--samples",32);
    let seed:u64=super::arg(args,"--seed",98712);
    if hands<2 || hands%2!=0 || samples==0{return Err(io::Error::new(io::ErrorKind::InvalidInput,"hands must be positive and even; samples >=1"));}
    let policy=FrozenPolicy::load(path)?;
    let mut sum=0f64;let mut pair_square_sum=0f64;let mut covered=0u64;let mut total=0u64;
    let start=Instant::now();
    for pair in 0..hands/2 {
        let deal_seed=seed ^ (pair as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let mut paired=0f64;
        for seat in 0..2{
            let (net,seen,decisions)=run_hand(&policy,mode,deal_seed,seat,samples);
            paired+=net as f64/BIG_BLIND as f64;
            covered+=seen;total+=decisions;
        }
        sum+=paired;pair_square_sum+=paired*paired;
    }
    let pairs=(hands/2) as f64;
    let mean_pair=sum/pairs;
    let variance=(pair_square_sum/pairs-mean_pair*mean_pair).max(0.0);
    let bb100=mean_pair/2.0*100.0;
    let half_width=1.96*(variance/pairs).sqrt()/2.0*100.0;
    let title=match mode{Candidate::Phh=>"phh",Candidate::Uniform=>"uniform",Candidate::MonteCarlo=>"mc"};
    println!("PHH_DUEL candidate={} opponent=mc hands={} samples={} seed={} winner_ev_bb100={:+.3} ci95_pair_half_width={:.3} covered_decisions={} total_candidate_decisions={} covered_fraction={:.4} elapsed={:.3}s",
        title,hands,samples,seed,bb100,half_width,covered,total,
        if total>0{covered as f64/total as f64}else{0.0},
        start.elapsed().as_secs_f64());
    println!("PHH_DUEL warning: trained on six-player hands, evaluated Heads-Up using pooled player-count buckets; transferred behavior, not native six-player ability or a GTO measure");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn two_same_mc_policies_close_to_zero(){
        // The confidence interval of a very small sample is wide;
        // check settlement and no massive positional bias.
        let f=std::env::temp_dir().join(format!("phh-eval-empty-{}.bin",std::process::id()));
        use std::io::Write;
        let mut file=std::fs::File::create(&f).unwrap();
        file.write_all(b"SPKIMT01").unwrap();
        file.write_all(&(4u32*5*2*4*3*2).to_le_bytes()).unwrap();
        file.write_all(&vec![0u8;4*5*2*4*3*2*3*8]).unwrap();
        drop(file);
        let policy=FrozenPolicy::load(f.to_str().unwrap()).unwrap();
        std::fs::remove_file(f).unwrap();
        let mut total=0i64;
        for pair in 0..50{
            let seed=pair as u64+120;
            for seat in 0..2 {
                total+=run_hand(&policy,Candidate::MonteCarlo,seed,seat,8).0;
            }
        }
        assert!(total.abs()<=20000,"unexpectedly extreme MC self-match result");
    }
}

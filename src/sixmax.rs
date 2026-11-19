//! Playable six-handed No-Limit Hold'em cash table and lightweight self-play.
//! No external dependencies. Separate from historical PHH replay and HU arena.
use crate::{deck, score7, Rng};
use crate::imitation::FrozenPolicy;
use std::fs::File;
use std::io::{self,Read,Write};
use std::time::Instant;

const N:usize=6;
const STACK:i64=200;
const BB:i64=2;
const STATES:usize=4*5*3*3*3*4*3*3;
const ACTIONS:usize=6;
const MAGIC:&[u8;8]=b"SPK6MAX3";

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Action {Fold,Call,Raise(i64)} // Raise to street-total
#[derive(Clone)]
struct Table6 {
    hole:[[u8;2];N],board:[u8;5],visible:usize,
    stack:[i64;N],street:[i64;N],total:[i64;N],
    folded:[bool;N],pending:[bool;N],acted:[bool;N],
    button:usize,sb:usize,bb:usize,actor:usize,
    high:i64,min_raise:i64,pot:i64,round:u8,
    raise_count:u8,raise_per_seat:[u8;N],
    estimated_ranges:[[f32;3];N],
}
impl Table6 {
    fn new(seed:u64,button:usize)->Self{
        let mut rng=Rng::new(seed);
        let d=deck(&mut rng);
        let sb=(button+1)%N;let bb=(button+2)%N;
        let mut t=Self{
            hole:std::array::from_fn(|p|[d[2*p],d[2*p+1]]),
            board:[d[12],d[13],d[14],d[15],d[16]],visible:0,
            stack:[STACK;N],street:[0;N],total:[0;N],folded:[false;N],
            pending:[true;N],acted:[false;N],button,sb,bb,actor:(bb+1)%N,
            high:0,min_raise:BB,pot:0,round:0,
            raise_count:0,raise_per_seat:[0;N],
            estimated_ranges:[[0.15,0.35,0.50];N],
        };
        t.pay(sb,1);t.pay(bb,BB);t.high=BB;
        t
    }
    fn update_public_range(&mut self,p:usize,kind:usize){
        // Three hand-strength classes: premium, medium, weak.
        // Fixed public-action likelihoods are a heuristic Bayesian prior,
        // not an oracle or a learned 169-combo posterior.
        let likelihood=match kind{
            0=>[0.08f32,0.20,0.60], // fold
            1=>[0.25f32,0.52,0.32], // call/check
            _=>[0.69f32,0.30,0.09], // bet/raise
        };
        let mut posterior=[0f32;3];
        for i in 0..3{
            posterior[i]=self.estimated_ranges[p][i]*likelihood[i];
        }
        let sum=posterior.iter().sum::<f32>();
        if sum>0.0 {
            for i in 0..3{self.estimated_ranges[p][i]=posterior[i]/sum;}
        }
    }
    fn opponent_range_bucket(&self,p:usize)->usize{
        let mut strongest=0f32;
        for i in 0..N {
            if i!=p && !self.folded[i] {
                strongest=strongest.max(self.estimated_ranges[i][0]);
            }
        }
        if strongest<0.25{0}else if strongest<0.55{1}else{2}
    }
    fn pay(&mut self,p:usize,amount:i64){
        assert!((0..=self.stack[p]).contains(&amount));
        self.stack[p]-=amount;self.street[p]+=amount;
        self.total[p]+=amount;self.pot+=amount;
    }
    fn alive(&self)->usize{(0..N).filter(|&p|!self.folded[p]).count()}
    fn live_bettors(&self)->usize{(0..N).filter(|&p|!self.folded[p] && self.stack[p]>0).count()}
    fn call(&self,p:usize)->i64{(self.high-self.street[p]).max(0).min(self.stack[p])}
    fn raise_to(&self,p:usize,action:usize)->i64{
        let max_to=self.street[p]+self.stack[p];
        let desired=match action{
            2=>self.high+(self.pot/3).max(self.min_raise),
            3=>self.high+((self.pot*3)/4).max(self.min_raise),
            4=>self.high+self.pot.max(self.min_raise),
            5=>max_to,
            _=>panic!("invalid bet-size action"),
        };
        desired.min(max_to)
    }
    fn legal(&self,p:usize)->[bool;ACTIONS]{
        let due=self.high-self.street[p];
        let opponent_live=(0..N).any(|q|q!=p && !self.folded[q] && self.stack[q]>0);
        let possible=!self.acted[p] && opponent_live && self.stack[p]>due;
        let mut result=[false;ACTIONS];
        result[0]=due>0;
        result[1]=true;
        for action in 2..ACTIONS {
            if possible {
                let to=self.raise_to(p,action);
                let incr=to-self.high;
                result[action]=incr>0 && (incr>=self.min_raise || to==self.street[p]+self.stack[p]);
            }
        }
        result
    }
    fn first_pending_after(&self,after:usize)->Option<usize>{
        (1..=N).map(|k|(after+k)%N).find(|&p|self.pending[p] && !self.folded[p] && self.stack[p]>0)
    }
    fn completed(&self)->bool{
        self.alive()==1 || (0..N).all(|p|!self.pending[p] || self.folded[p] || self.stack[p]==0)
    }
    fn act(&mut self,action:Action)->Result<(),String>{
        if self.completed(){return Err("round already complete".into());}
        let p=self.actor;
        if !self.pending[p] || self.folded[p] || self.stack[p]==0{return Err("actor not eligible".into());}
        let call=self.call(p);
        let legal=self.legal(p);
        match action {
            Action::Fold if legal[0] =>{self.folded[p]=true;self.pending[p]=false;self.acted[p]=true;self.update_public_range(p,0);}
            Action::Call=>{
                self.pay(p,call);
                self.pending[p]=false;
                self.acted[p]=true;
                self.update_public_range(p,1);
            }
            Action::Raise(to) if legal[2..].iter().any(|&v|v) =>{
                let delta=to-self.street[p];
                if delta<=call || delta>self.stack[p] || to<=self.high {return Err("invalid raise amount".into());}
                let increment=to-self.high;
                let allin=delta==self.stack[p];
                if increment<self.min_raise && !allin{return Err("short raise not all-in".into());}
                self.pay(p,delta);
                self.high=to;
                self.raise_count=self.raise_count.saturating_add(1);
                self.raise_per_seat[p]=self.raise_per_seat[p].saturating_add(1);
                self.pending[p]=false;
                self.update_public_range(p,2);
                if increment>=self.min_raise {
                    self.min_raise=increment;
                    self.acted=[false;N];
                    self.acted[p]=true;
                    for q in 0..N {
                        if q!=p && !self.folded[q] && self.stack[q]>0 {self.pending[q]=true;}
                    }
                }else{
                    // Short all-in does not reopen betting for actors that
                    // already acted, but everyone owing chips must respond.
                    self.acted[p]=true;
                    for q in 0..N {
                        if q!=p && !self.folded[q] && self.stack[q]>0 && self.street[q]<self.high {
                            self.pending[q]=true;
                        }
                    }
                }
            }
            _=>return Err("illegal action".into()),
        }
        if self.alive()>1 {
            if let Some(next)=self.first_pending_after(p){self.actor=next;}
        }
        Ok(())
    }
    fn next_street(&mut self)->Result<(),String>{
        if !self.completed(){return Err("outstanding betting".into());}
        if self.round>=3{return Err("already at river".into());}
        self.round+=1;
        self.visible=match self.round{1=>3,2=>4,_=>5};
        self.street=[0;N];self.high=0;self.min_raise=BB;
        self.acted=[false;N];
        self.pending=std::array::from_fn(|p|!self.folded[p] && self.stack[p]>0);
        self.actor=self.first_pending_after(self.button).unwrap_or(self.button);
        Ok(())
    }
    fn settle(&self)->[i64;N]{
        let mut winnings=[0i64;N];
        let mut levels=self.total.to_vec();
        levels.sort_unstable();levels.dedup();levels.retain(|&x|x>0);
        let mut previous=0;
        for level in levels{
            let funders:Vec<usize>=(0..N).filter(|&p|self.total[p]>=level).collect();
            let pot=(level-previous)*funders.len() as i64;
            previous=level;
            let eligible:Vec<usize>=funders.into_iter().filter(|&p|!self.folded[p]).collect();
            assert!(!eligible.is_empty(),"unclaimable pot");
            let winners=if eligible.len()==1{eligible}else{
                let mut best=0;
                let mut results=Vec::new();
                for p in eligible {
                    let score=score7(&[
                        self.hole[p][0],self.hole[p][1],
                        self.board[0],self.board[1],self.board[2],self.board[3],self.board[4]
                    ]);
                    if score>best{best=score;results.clear();results.push(p);}
                    else if score==best {results.push(p);}
                }
                results
            };
            let share=pot/winners.len() as i64;
            let extra=pot%winners.len() as i64;
            // Odd chips distributed clockwise starting immediately after the button.
            let mut ordered=winners;
            ordered.sort_by_key(|&p|(p+N-self.button-1)%N);
            for (i,p) in ordered.into_iter().enumerate(){
                winnings[p]+=share+if (i as i64)<extra{1}else{0};
            }
        }
        assert_eq!(winnings.iter().sum::<i64>(),self.pot);
        winnings
    }
    fn net(&self)->[i64;N]{
        let w=self.settle();
        std::array::from_fn(|p|w[p]-self.total[p])
    }
    fn texture(&self)->usize{
        if self.visible<3{return 0;}
        let mut ranks=[0u8;13];let mut suits=[0u8;4];
        for &card in &self.board[..self.visible]{
            ranks[(card%13) as usize]+=1;suits[(card/13) as usize]+=1;
        }
        if ranks.iter().any(|&n|n>=2){1}
        else if suits.iter().any(|&n|n>=3){2}
        else{3}
    }
    fn view(&self,p:usize)->View{
        let mut visible_board=[255u8;5];
        visible_board[..self.visible].copy_from_slice(&self.board[..self.visible]);
        View{hole:self.hole[p],board:visible_board,visible:self.visible,
            pot:self.pot,call:self.call(p),stack:self.stack[p],street:self.round,
            on_button:p==self.button,active:self.alive(),legal:self.legal(p),
            pressure:self.raise_count.min(2),texture:self.texture(),
            range_bucket:self.opponent_range_bucket(p)}
    }
}
#[derive(Clone,Copy)]
struct View{
    hole:[u8;2],board:[u8;5],visible:usize,pot:i64,call:i64,stack:i64,
    street:u8,on_button:bool,active:usize,legal:[bool;ACTIONS],
    pressure:u8,texture:usize,range_bucket:usize,
}
fn state(v:View)->usize{
    let a=v.hole[0]%13;
    let b=v.hole[1]%13;
    let strength=if a==b{0}else if a.max(b)>=11{1}else if a.max(b)>=8{2}else if v.hole[0]/13==v.hole[1]/13{3}else{4};
    let players=if v.active<=2{0}else if v.active<=4{1}else{2};
    let ratio=v.call as f64/(v.pot+v.call).max(1) as f64;
    let price=if v.call==0{0}else if ratio<0.28{1}else{2};
    let pos=if v.on_button{0}else if v.street==0{1}else{2};
    let mut key=v.street as usize;
    for (factor,bucket) in [
        (5,strength),(3,players),(3,price),(3,pos),
        (4,v.texture),(3,v.pressure as usize),(3,v.range_bucket)
    ] {
        key=key*factor+bucket;
    }
    key
}
#[derive(Clone,Copy)]
struct PolicyRow{weights:[f32;ACTIONS],baseline:f32,visits:u32}
impl Default for PolicyRow{
    fn default()->Self{Self{weights:[-0.8,0.5,-0.3,-0.5,-0.7,-1.2],baseline:0.0,visits:0}}
}
#[derive(Clone)]
struct Bot{table:Box<[PolicyRow]>}
impl Bot {fn new()->Self{Self{table:vec![PolicyRow::default();STATES].into_boxed_slice()}}}
fn probs(row:&PolicyRow,legal:[bool;ACTIONS])->[f32;ACTIONS]{
    let max=(0..ACTIONS).filter(|&i|legal[i]).map(|i|row.weights[i]).fold(f32::NEG_INFINITY,f32::max);
    let mut out=[0f32;ACTIONS];let mut total=0f32;
    for i in 0..ACTIONS{if legal[i]{out[i]=(row.weights[i]-max).exp();total+=out[i];}}
    for x in &mut out{*x/=total;}
    out
}
fn draw(prob:[f32;ACTIONS],rng:&mut Rng)->usize{
    let x=rng.next() as f64/u64::MAX as f64;
    let mut sum=0f64;
    for (a,p) in prob.iter().enumerate(){sum+=*p as f64;if x<sum{return a;}}
    (0..ACTIONS).rev().find(|&i|prob[i]>0.0).unwrap_or(1)
}
#[derive(Clone)]
struct Visit{key:usize,action:usize,probs:[f32;ACTIONS]}
fn reinforce(bot:&mut Bot,history:&[Visit],net:i64,rate:f32){
    if rate<=0.0{return;}
    let reward=(net as f32/(BB as f32*20.0)).clamp(-5.0,5.0);
    for visit in history{
        let row=&mut bot.table[visit.key];
        let adv=(reward-row.baseline).clamp(-5.0,5.0);
        for a in 0..ACTIONS{
            let chosen=if a==visit.action{1.0}else{0.0};
            row.weights[a]=(row.weights[a]+rate*adv*(chosen-visit.probs[a])).clamp(-10.0,10.0);
        }
        row.baseline+=0.02*(reward-row.baseline);
        row.visits=row.visits.saturating_add(1);
    }
}
fn play(bots:&mut [Bot;N],seed:u64,button:usize,rate:f32,random_opponent:bool,phh_opponent:Option<&FrozenPolicy>,phh_candidate:bool,action_counts:&mut [u64;ACTIONS])->[i64;N]{
    let mut t=Table6::new(seed,button);
    let mut rng=Rng::new(seed^0xbeecafefeed55eed);
    let mut history:[Vec<Visit>;N]=std::array::from_fn(|_|Vec::new());
    for street in 0..4{
        if street>0 {
            if t.alive()==1{break;}
            t.next_street().expect("betting transition");
        }
        let mut steps=0;
        while !t.completed(){
            steps+=1;
            assert!(steps<=200,"betting loop exceeds cap");
            let p=t.actor;
            let v=t.view(p);
            let key=state(v);
            let values=if let Some(model)=phh_opponent {
                if p!=0 || phh_candidate {
                    let coarse=[v.legal[0],v.legal[1],v.legal[2..].iter().any(|&x|x)];
                    let raw=model.predict_multi(v.street as usize,v.hole,v.pot,v.call,v.on_button,v.active,coarse).0;
                    let mut spread=[0.0f32;ACTIONS];
                    spread[0]=raw[0];spread[1]=raw[1];
                    let weights=[0.30f32,0.35,0.25,0.10];
                    let normalizer=(2..ACTIONS).filter(|&a|v.legal[a])
                        .map(|a|weights[a-2]).sum::<f32>();
                    for a in 2..ACTIONS {
                        if v.legal[a] && normalizer>0.0 {spread[a]=raw[2]*weights[a-2]/normalizer;}
                    }
                    spread
                }else{probs(&bots[p].table[key],v.legal)}
            }else if random_opponent && p!=0 {
                let n=v.legal.iter().filter(|&&x|x).count() as f32;
                std::array::from_fn(|a|if v.legal[a]{1.0/n}else{0.0})
            }else{probs(&bots[p].table[key],v.legal)};
            let choice=draw(values,&mut rng);
            action_counts[choice]+=1;
            let action=match choice{
                0=>Action::Fold,
                1=>Action::Call,
                a=>Action::Raise(t.raise_to(p,a)),
            };
            t.act(action).expect("policy proposed legal action");
            history[p].push(Visit{key,action:choice,probs:values});
        }
        if t.alive()==1{break;}
    }
    let net=t.net();
    assert_eq!(net.iter().sum::<i64>(),0,"net zero sum");
    for p in 0..N {
        reinforce(&mut bots[p],&history[p],net[p],rate);
    }
    net
}
fn save(path:&str,bots:&[Bot;N])->io::Result<()>{
    let mut file=File::create(path)?;
    file.write_all(MAGIC)?;
    for b in bots {
        for r in &b.table{
            for x in r.weights {file.write_all(&x.to_le_bytes())?;}
            file.write_all(&r.baseline.to_le_bytes())?;
            file.write_all(&r.visits.to_le_bytes())?;
        }
    }
    Ok(())
}
fn load(path:&str)->io::Result<[Bot;N]>{
    let mut file=File::open(path)?;
    let mut magic=[0u8;8];file.read_exact(&mut magic)?;
    if &magic!=MAGIC{return Err(io::Error::new(io::ErrorKind::InvalidData,"bad 6max checkpoint"));}
    let mut bots=std::array::from_fn(|_|Bot::new());
    for bot in &mut bots {
        for row in &mut bot.table {
            for x in &mut row.weights{
                let mut b=[0u8;4];file.read_exact(&mut b)?;*x=f32::from_le_bytes(b);
                if !x.is_finite(){return Err(io::Error::new(io::ErrorKind::InvalidData,"nonfinite weight"));}
            }
            let mut b=[0u8;4];file.read_exact(&mut b)?;row.baseline=f32::from_le_bytes(b);
            let mut v=[0u8;4];file.read_exact(&mut v)?;row.visits=u32::from_le_bytes(v);
        }
    }
    Ok(bots)
}
pub fn run(args:&[String])->io::Result<()>{
    let hands:usize=super::arg(args,"--hands",100000);
    let seed:u64=super::arg(args,"--seed",42);
    let rate:f32=super::arg(args,"--learning-rate",0.025);
    let path=args.windows(2).find(|w|w[0]=="--checkpoint").map(|w|w[1].as_str()).unwrap_or("sixmax.bin");
    let resume=args.iter().any(|a|a=="--resume");
    if hands==0 || !rate.is_finite() || !(0.0..=1.0).contains(&rate){return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid hands/learning-rate"));}
    let mut bots=if resume{load(path)?}else{std::array::from_fn(|_|Bot::new())};
    let start=Instant::now();
    let mut totals=[0i64;N];
    let mut action_counts=[0u64;ACTIONS];
    for i in 0..hands{
        let s=seed^((i as u64).wrapping_mul(0x9e3779b97f4a7c15));
        let result=play(&mut bots,s,i%N,rate,false,None,false,&mut action_counts);
        for p in 0..N{totals[p]+=result[p];}
    }
    save(path,&bots)?;
    println!("SIXMAX_ACTIONS total={} fold={} call={} bet33={} bet75={} pot={} allin={}",
        action_counts.iter().sum::<u64>(),action_counts[0],action_counts[1],
        action_counts[2],action_counts[3],action_counts[4],action_counts[5]);
    println!("SIXMAX hands={} time={:.3}s hands_per_sec={:.1} checkpoint={}",hands,
        start.elapsed().as_secs_f64(),hands as f64/start.elapsed().as_secs_f64().max(1e-9),path);
    for p in 0..N {println!("SIXMAX player={} net={} bb100={:+.3} visited_states={}",p,totals[p],
        totals[p] as f64/BB as f64*100.0/hands as f64,
        bots[p].table.iter().filter(|r|r.visits>0).count());}
    Ok(())
}
pub fn evaluate(args:&[String])->io::Result<()>{
    let path=args.windows(2).find(|w|w[0]=="--checkpoint").map(|w|w[1].as_str())
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing checkpoint"))?;
    let hands:usize=super::arg(args,"--hands",100000);
    let seed:u64=super::arg(args,"--seed",42);
    if hands==0 || hands%N!=0{return Err(io::Error::new(io::ErrorKind::InvalidInput,"hands must be divisible by 6"));}
    let original=load(path)?;
    let opponent=args.windows(2).find(|w|w[0]=="--opponent").map(|w|w[1].as_str()).unwrap_or("random");
    if opponent!="random" && opponent!="phh" && opponent!="clone" {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"opponent must be random, phh or clone"));
    }
    let phh_path=args.windows(2).find(|w|w[0]=="--phh-model").map(|w|w[1].as_str());
    let phh=if opponent=="phh" || args.iter().any(|w|w=="--candidate-phh") {
        Some(FrozenPolicy::load(phh_path.ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --phh-model"))?)?)
    }else{None};
    let candidate_phh=args.iter().any(|w|w=="--candidate-phh");
    if candidate_phh && opponent!="phh" {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"--candidate-phh requires --opponent phh"));
    }
    // Candidate policy stays seat 0, with button rotating across six positions.
    let champion=original[0].clone();
    let mut bots=std::array::from_fn(|_|Bot::new());
    bots[0]=champion.clone();
    if opponent=="clone" {for bot in &mut bots[1..] {*bot=champion.clone();}}
    let groups=hands/N;
    let mut totals=0f64;
    let mut group_sq=0f64;
    let mut action_counts=[0u64;ACTIONS];
    for group in 0..groups {
        let pair_seed=seed ^ (group as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let mut group_result=0f64;
        for position in 0..N {
            let net=play(&mut bots,pair_seed,position,0.0,opponent=="random",phh.as_ref(),candidate_phh,&mut action_counts)[0] as f64/BB as f64;
            group_result+=net;
        }
        totals+=group_result;
        group_sq+=group_result*group_result;
    }
    let mean=totals/hands as f64;
    let group_mean=totals/groups as f64;
    let group_var=(group_sq/groups as f64-group_mean*group_mean).max(0.0);
    let half=1.96*(group_var/groups as f64).sqrt()*100.0/N as f64;
    println!("SIXMAX_EVAL_ACTIONS total={} fold={} call={} bet33={} bet75={} pot={} allin={}",
        action_counts.iter().sum::<u64>(),action_counts[0],action_counts[1],
        action_counts[2],action_counts[3],action_counts[4],action_counts[5]);
    println!("SIXMAX_EVAL hands={} bb100={:+.3} ci95_six_hand_cluster_half={:.3} opponent={} candidate={} eval_seed={}",hands,mean*100.0,half,opponent,if candidate_phh{"phh"}else{"selfplay"},seed);
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn blind_and_preflop_order(){
        let t=Table6::new(7,0);
        assert_eq!((t.sb,t.bb,t.actor),(1,2,3));
        assert_eq!(t.pot,3);
    }
    #[test]fn blind_rotation(){
        for i in 0..N{
            let t=Table6::new(11,i);
            assert_eq!(t.sb,(i+1)%N);
            assert_eq!(t.bb,(i+2)%N);
        }
    }
    #[test]fn fold_everyone_else_and_conserve(){
        let mut t=Table6::new(4,0);
        while t.alive()>1 {
            let p=t.actor;
            if p==t.bb {t.act(Action::Call).unwrap();}
            else if t.legal(p)[0] {t.act(Action::Fold).unwrap();}
            else{t.act(Action::Call).unwrap();}
            if t.completed(){break;}
        }
        assert_eq!(t.net().iter().sum::<i64>(),0);
    }
    #[test]fn illegal_bet_rejected(){
        let mut t=Table6::new(1,0);
        assert!(t.act(Action::Raise(3)).is_err());
        assert_eq!(t.pot,3);
    }
    #[test]fn full_raise_reopens_action(){
        let mut t=Table6::new(11,0);
        for _ in 0..5{t.act(Action::Call).unwrap();}
        assert_eq!(t.actor,2);
        t.act(Action::Raise(8)).unwrap();
        assert!(t.pending[3]);
    }
    #[test]fn sidepot_conserves(){
        let mut t=Table6::new(9,0);
        t.total=[10,20,30,30,40,50];
        t.pot=t.total.iter().sum();
        t.stack=std::array::from_fn(|p|STACK-t.total[p]);
        t.folded=[false,false,true,false,true,false];
        let w=t.settle();
        assert_eq!(w.iter().sum::<i64>(),t.pot);
    }
    #[test]fn complete_table_reference_isolated(){
        let mut t=Table6::new(5,1);
        assert_eq!(t.hole.iter().flatten().copied().collect::<std::collections::HashSet<_>>().len(),12);
        let _=t.view(0);
    }
    #[test]fn future_board_is_hidden_in_observations(){
        let mut t=Table6::new(13,0);
        let before=t.view(3);
        assert_eq!(before.board,[255;5]);
        t.visible=3;
        let flop=t.view(3);
        assert_eq!(flop.board[3..],[255,255]);
        assert_eq!(flop.board[..3],t.board[..3]);
    }
    #[test]fn public_range_update_normalized(){
        let mut t=Table6::new(9,0);
        t.update_public_range(3,2);
        let r=t.estimated_ranges[3];
        assert!((r.iter().sum::<f32>()-1.0).abs()<1e-6);
        assert!(r[0]>0.15);
        assert_eq!(t.view(1).range_bucket,1);
    }
    #[test]fn six_bet_size_actions_are_legal(){
        let t=Table6::new(9,0);
        let actions=t.legal(t.actor);
        assert_eq!(actions.len(),6);
        assert!(actions[2..].iter().any(|&a|a));
        for a in 2..ACTIONS {
            if actions[a]{
                let to=t.raise_to(t.actor,a);
                assert!(to>t.high);
            }
        }
    }
    #[test]fn short_allin_does_not_reopen_raising(){
        let mut t=Table6::new(23,0);
        t.act(Action::Raise(20)).unwrap(); // seat 3 raises from BB=2
        assert_eq!(t.actor,4);
        t.stack[4]=30;
        t.act(Action::Raise(30)).unwrap(); // short all-in, only +10 over 20
        while t.actor!=3 && !t.completed(){
            t.act(Action::Call).unwrap();
        }
        assert_eq!(t.actor,3);
        let legal=t.legal(3);
        assert!(legal[1]);
        assert!(!legal[2..].iter().any(|&x|x),"short raise reopened betting");
        t.act(Action::Call).unwrap();
        assert!(t.completed());
    }
    #[test]fn deterministic_three_way_sidepot_payout(){
        let mut t=Table6::new(5,0);
        t.total=[10,20,30,0,0,0];
        t.pot=60;
        t.stack=[190,180,170,200,200,200];
        t.folded=[false,false,false,true,true,true];
        // Main pot: five-high club straight flush (seat 0);
        // next pot: trip nines (seat 1); unmatched last tier (seat 2).
        t.hole[0]=[12,3]; // Ac,5c
        t.hole[1]=[46,20]; // 9s,9d
        t.hole[2]=[44,18]; // 7s,7d
        t.board=[0,1,2,16,33]; // 2c 3c 4c 5d 9h
        let paid=t.settle();
        assert_eq!(paid,[30,20,10,0,0,0]);
    }
    #[test]fn seeded_games_conserve(){
        let mut bots=std::array::from_fn(|_|Bot::new());
        let mut counts=[0u64;ACTIONS];
        for i in 0..500 {
            let result=play(&mut bots,i*137+4,(i as usize)%N,0.03,false,None,false,&mut counts);
            assert_eq!(result.iter().sum::<i64>(),0);
        }
    }
}

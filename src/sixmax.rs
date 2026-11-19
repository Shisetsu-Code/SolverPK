//! Playable six-handed No-Limit Hold'em cash table and lightweight self-play.
//! No external dependencies. Separate from historical PHH replay and HU arena.
use crate::{deck, score7, Rng};
use std::fs::File;
use std::io::{self,Read,Write};
use std::time::Instant;

const N:usize=6;
const STACK:i64=200;
const BB:i64=2;
const STATES:usize=4*5*3*3*3;
const ACTIONS:usize=3;
const MAGIC:&[u8;8]=b"SPK6MAX1";

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Action {Fold,Call,Raise(i64)} // Raise to street-total
#[derive(Clone)]
struct Table6 {
    hole:[[u8;2];N],board:[u8;5],visible:usize,
    stack:[i64;N],street:[i64;N],total:[i64;N],
    folded:[bool;N],pending:[bool;N],acted:[bool;N],
    button:usize,sb:usize,bb:usize,actor:usize,
    high:i64,min_raise:i64,pot:i64,round:u8,
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
        };
        t.pay(sb,1);t.pay(bb,BB);t.high=BB;
        t
    }
    fn pay(&mut self,p:usize,amount:i64){
        assert!((0..=self.stack[p]).contains(&amount));
        self.stack[p]-=amount;self.street[p]+=amount;
        self.total[p]+=amount;self.pot+=amount;
    }
    fn alive(&self)->usize{(0..N).filter(|&p|!self.folded[p]).count()}
    fn live_bettors(&self)->usize{(0..N).filter(|&p|!self.folded[p] && self.stack[p]>0).count()}
    fn call(&self,p:usize)->i64{(self.high-self.street[p]).max(0).min(self.stack[p])}
    fn legal(&self,p:usize)->[bool;3]{
        let due=self.high-self.street[p];
        let opponent_live=(0..N).any(|q|q!=p && !self.folded[q] && self.stack[q]>0);
        [due>0,true,!self.acted[p] && opponent_live && self.stack[p]>due && self.street[p]+self.stack[p]>self.high]
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
            Action::Fold if legal[0] =>{self.folded[p]=true;self.pending[p]=false;self.acted[p]=true;}
            Action::Call=>{
                self.pay(p,call);
                self.pending[p]=false;
                self.acted[p]=true;
            }
            Action::Raise(to) if legal[2] =>{
                let delta=to-self.street[p];
                if delta<=call || delta>self.stack[p] || to<=self.high {return Err("invalid raise amount".into());}
                let increment=to-self.high;
                let allin=delta==self.stack[p];
                if increment<self.min_raise && !allin{return Err("short raise not all-in".into());}
                self.pay(p,delta);
                self.high=to;
                self.pending[p]=false;
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
    fn view(&self,p:usize)->View{
        View{hole:self.hole[p],board:self.board,visible:self.visible,
            pot:self.pot,call:self.call(p),stack:self.stack[p],street:self.round,
            on_button:p==self.button,active:self.alive(),legal:self.legal(p)}
    }
}
#[derive(Clone,Copy)]
struct View{
    hole:[u8;2],board:[u8;5],visible:usize,pot:i64,call:i64,stack:i64,
    street:u8,on_button:bool,active:usize,legal:[bool;3],
}
fn state(v:View)->usize{
    let a=v.hole[0]%13;
    let b=v.hole[1]%13;
    let strength=if a==b{0}else if a.max(b)>=11{1}else if a.max(b)>=8{2}else if v.hole[0]/13==v.hole[1]/13{3}else{4};
    let players=if v.active<=2{0}else if v.active<=4{1}else{2};
    let ratio=v.call as f64/(v.pot+v.call).max(1) as f64;
    let price=if v.call==0{0}else if ratio<0.28{1}else{2};
    let pos=if v.on_button{0}else if v.street==0{1}else{2};
    (((v.street as usize*5+strength)*3+players)*3+price)*3+pos
}
#[derive(Clone,Copy)]
struct PolicyRow{weights:[f32;3],baseline:f32,visits:u32}
impl Default for PolicyRow{
    fn default()->Self{Self{weights:[-0.8,0.5,0.0],baseline:0.0,visits:0}}
}
#[derive(Clone)]
struct Bot{table:[PolicyRow;STATES]}
impl Bot {fn new()->Self{Self{table:[PolicyRow::default();STATES]}}}
fn probs(row:&PolicyRow,legal:[bool;3])->[f32;3]{
    let max=(0..3).filter(|&i|legal[i]).map(|i|row.weights[i]).fold(f32::NEG_INFINITY,f32::max);
    let mut out=[0f32;3];let mut total=0f32;
    for i in 0..3{if legal[i]{out[i]=(row.weights[i]-max).exp();total+=out[i];}}
    for x in &mut out{*x/=total;}
    out
}
fn draw(prob:[f32;3],rng:&mut Rng)->usize{
    let x=rng.next() as f64/u64::MAX as f64;
    let mut sum=0f64;
    for (a,p) in prob.iter().enumerate(){sum+=*p as f64;if x<sum{return a;}}
    (0..3).rev().find(|&i|prob[i]>0.0).unwrap_or(1)
}
#[derive(Clone)]
struct Visit{key:usize,action:usize,probs:[f32;3]}
fn reinforce(bot:&mut Bot,history:&[Visit],net:i64,rate:f32){
    if rate<=0.0{return;}
    let reward=(net as f32/(BB as f32*20.0)).clamp(-5.0,5.0);
    for visit in history{
        let row=&mut bot.table[visit.key];
        let adv=(reward-row.baseline).clamp(-5.0,5.0);
        for a in 0..3{
            let chosen=if a==visit.action{1.0}else{0.0};
            row.weights[a]=(row.weights[a]+rate*adv*(chosen-visit.probs[a])).clamp(-10.0,10.0);
        }
        row.baseline+=0.02*(reward-row.baseline);
        row.visits=row.visits.saturating_add(1);
    }
}
fn play(bots:&mut [Bot;N],seed:u64,button:usize,rate:f32,random_opponent:bool)->[i64;N]{
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
            let values=if random_opponent && p!=0 {
                let n=v.legal.iter().filter(|&&x|x).count() as f32;
                std::array::from_fn(|a|if v.legal[a]{1.0/n}else{0.0})
            }else{probs(&bots[p].table[key],v.legal)};
            let choice=draw(values,&mut rng);
            let action=match choice{
                0=>Action::Fold,
                1=>Action::Call,
                _=>{
                    let min_to=t.high+t.min_raise;
                    let target=(t.high+v.call+(t.pot/2).max(t.min_raise))
                        .max(min_to).min(t.street[p]+t.stack[p]);
                    Action::Raise(target)
                }
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
    for i in 0..hands{
        let s=seed^((i as u64).wrapping_mul(0x9e3779b97f4a7c15));
        let result=play(&mut bots,s,i%N,rate,false);
        for p in 0..N{totals[p]+=result[p];}
    }
    save(path,&bots)?;
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
    // Candidate policy always bot 0, opposing seats uniform-random legal policies.
    let champion=original[0].clone();
    let mut totals=0f64;let mut total_sq=0f64;
    for i in 0..hands {
        let mut bots=std::array::from_fn(|_|Bot::new());
        bots[0]=champion.clone();
        let pair_seed=seed ^ ((i/N) as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let net=play(&mut bots,pair_seed,i%N,0.0,true)[0] as f64/BB as f64;
        totals+=net;total_sq+=net*net;
    }
    let mean=totals/hands as f64;
    let half=1.96*((total_sq/hands as f64-mean*mean).max(0.0)/hands as f64).sqrt()*100.0;
    println!("SIXMAX_EVAL hands={} bb100={:+.3} ci95_approx_half={:.3} baseline=uniform_random_five_opponents",hands,mean*100.0,half);
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
    #[test]fn seeded_games_conserve(){
        let mut bots=std::array::from_fn(|_|Bot::new());
        for i in 0..500 {
            let result=play(&mut bots,i*137+4,(i as usize)%N,0.03,false);
            assert_eq!(result.iter().sum::<i64>(),0);
        }
    }
}

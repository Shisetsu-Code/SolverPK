//! PHH import and multiplayer action-state reconstruction (stdlib-only).
//! This is a replay validator, not the existing two-player game engine.
use std::collections::VecDeque;
use std::fs::{self,File};
use std::io::{self,Read,BufReader,BufWriter,Write};
use std::path::{Path,PathBuf};
const MAGIC: &[u8;8]=b"SPKOBS02";
const RECORD_BYTES: usize=96;
const UNKNOWN:u8=255;
#[derive(Debug,Default)]
struct Hand {
    variant:String,
    players:usize,
    player_names:Vec<String>,
    starting:Vec<i64>,
    finishing:Option<Vec<i64>>,
    blinds:Vec<i64>,
    antes:Vec<i64>,
    min_bet:i64,
    actions:Vec<String>,
}
#[derive(Default)]
struct Stats{files:u64,accepted:u64,rejected:u64,decisions:u64,by_action:[u64;3]}
fn invalid(msg:&str)->io::Error{io::Error::new(io::ErrorKind::InvalidData,msg)}
fn quoted_items(text:&str)->Result<Vec<String>,String>{
    let first=text.find('[').ok_or("missing [")?;
    let last=text.rfind(']').ok_or("missing ]")?;
    let c:Vec<char>=text[first+1..last].chars().collect();
    let mut i=0;
    let mut out=Vec::new();
    while i<c.len(){
        while i<c.len() && (c[i].is_whitespace() || c[i]==','){i+=1;}
        if i==c.len(){break;}
        if c[i]=='#'{while i<c.len() && c[i]!='\n'{i+=1;}continue;}
        let q=c[i];
        if q!='\'' && q!='"'{return Err("expected quoted item".into());}
        i+=1;let mut value=String::new();let mut done=false;
        while i<c.len(){
            let ch=c[i];i+=1;
            if ch==q{done=true;break;}
            if ch=='\\' && i<c.len(){value.push(c[i]);i+=1;}
            else{value.push(ch);}
        }
        if !done{return Err("unterminated quoted item".into());}
        out.push(value);
    }
    Ok(out)
}
fn numeric_items(text:&str)->Result<Vec<i64>,String>{
    let a=text.find('[').ok_or("missing numeric [")?;
    let b=text.rfind(']').ok_or("missing numeric ]")?;
    text[a+1..b].split(',').filter(|s|!s.trim().is_empty()).map(parse_money).collect()
}
fn parse_money(s:&str)->Result<i64,String>{
    let cleaned=s.trim().trim_matches('"').trim_matches('\'');
    let f=cleaned.parse::<f64>().map_err(|_|format!("bad amount {cleaned}"))?;
    if !f.is_finite() || f<0.0 || f>(i64::MAX as f64)/100.0{return Err(format!("amount out of bounds {cleaned}"));}
    let cents=(f*100.0).round();
    if ((f*100.0)-cents).abs()>0.001{return Err("amount has more than 2 decimal places".into());}
    Ok(cents as i64)
}
fn parse_hand(input:&str)->Result<Hand,String>{
    let mut h=Hand::default();let mut buffer=String::new();
    let mut key=String::new();let mut collecting=false;
    fn put(h:&mut Hand,key:&str,value:&str)->Result<(),String>{
        match key{
            "variant"=>h.variant=value.trim().trim_matches(|c|c=='"'||c=='\'').to_string(),
            "players"=>{h.player_names=quoted_items(value)?;h.players=h.player_names.len();},
            "starting_stacks"=>h.starting=numeric_items(value)?,
            "finishing_stacks"=>h.finishing=Some(numeric_items(value)?),
            "antes"=>h.antes=numeric_items(value)?,
            "blinds_or_straddles"=>h.blinds=numeric_items(value)?,
            "min_bet"=>h.min_bet=parse_money(value)?,
            "actions"=>h.actions=quoted_items(value)?,
            _=>{},
        }
        Ok(())
    }
    for line in input.lines(){
        let trimmed=line.trim();
        if collecting{
            buffer.push('\n');buffer.push_str(line);
            if trimmed.starts_with(']') || trimmed.contains(']'){
                put(&mut h,&key,&buffer)?;collecting=false;buffer.clear();
            }
            continue;
        }
        let Some((k,v))=trimmed.split_once('=') else {continue};
        let k=k.trim();let v=v.trim();
        if ["players","starting_stacks","finishing_stacks","antes","blinds_or_straddles","actions"].contains(&k)
            && !v.contains(']'){
            collecting=true;key=k.to_string();buffer=v.to_string();continue;
        }
        put(&mut h,k,v)?;
    }
    if collecting{return Err("unterminated multiline field".into());}
    if h.variant!="NT"{return Err("only no-limit Texas Hold'em (NT) is supported".into());}
    if !(2..=10).contains(&h.players) || h.starting.len()!=h.players ||
       h.blinds.len()!=h.players || h.antes.len()!=h.players{
        return Err("player and forced-bet array lengths inconsistent".into());
    }
    if h.actions.is_empty() || h.min_bet==0{return Err("missing actions or min_bet".into());}
    if let Some(ref f)=h.finishing {if f.len()!=h.players{return Err("finishing stack count mismatch".into());}}
    Ok(h)
}
fn card_id(card:&str)->Result<u8,String>{
    let b=card.as_bytes();
    if b.len()!=2{return Err(format!("invalid card {card}"));}
    let r=b"23456789TJQKA".iter().position(|&c|c==b[0]).ok_or("unknown rank")?;
    let s=b"cdhs".iter().position(|&c|c==b[1]).ok_or("unknown suit")?;
    Ok((s*13+r) as u8)
}
fn decode_cards(input:&str)->Result<Vec<u8>,String>{
    if input.len()%2!=0{return Err("odd card string".into());}
    input.as_bytes().chunks_exact(2).map(|chunk|{
        let s=std::str::from_utf8(chunk).map_err(|e|e.to_string())?;
        card_id(s)
    }).collect()
}
#[derive(Clone,Debug)]
struct State{
    stacks:Vec<i64>,start:Vec<i64>,total:Vec<i64>,street_paid:Vec<i64>,
    folded:Vec<bool>,hole:Vec<[u8;2]>,board:[u8;5],board_len:usize,
    pot:i64,street:u8,current:i64,min_raise:i64,min_bet:i64,
    queue:VecDeque<usize>,button:usize,
}
fn ordered_after(seat:usize,n:usize,eligible:impl Fn(usize)->bool)->VecDeque<usize>{
    (1..=n).map(|k|(seat+k)%n).filter(|&p|eligible(p)).collect()
}
impl State{
    fn new(hand:&Hand)->Result<Self,String>{
        let n=hand.players;let mut s=Self{
            stacks:hand.starting.clone(),start:hand.starting.clone(),total:vec![0;n],
            street_paid:vec![0;n],folded:vec![false;n],hole:vec![[UNKNOWN;2];n],
            board:[UNKNOWN;5],board_len:0,pot:0,street:0,current:0,
            min_raise:hand.min_bet,min_bet:hand.min_bet,queue:VecDeque::new(),button:0,
        };
        for p in 0..n {
            let ante=hand.antes[p].min(s.stacks[p]);
            s.stacks[p]-=ante;s.total[p]+=ante;s.pot+=ante;
            let blind=hand.blinds[p].min(s.stacks[p]);
            s.stacks[p]-=blind;s.total[p]+=blind;s.street_paid[p]+=blind;s.pot+=blind;
        }
        s.current=*s.street_paid.iter().max().unwrap();
        let sb=hand.blinds.iter().position(|&b|b>0).ok_or("no small blind")?;
        let bb=hand.blinds.iter().rposition(|&b|b>0).ok_or("no big blind")?;
        s.button=if n==2 {sb} else {(sb+n-1)%n};
        let previous=if n==2 {bb} else {bb};
        s.queue=ordered_after(previous,n,|p|s.stacks[p]>0 && !s.folded[p]);
        Ok(s)
    }
    fn next_street(&mut self, cards:&[u8])->Result<(),String>{
        if !self.queue.is_empty() && self.stacks.iter().enumerate().filter(|(i,v)|**v>0 && !self.folded[*i]).count()>1{
            return Err(format!("premature board transition; outstanding actions {:?}",self.queue));
        }
        let expected=match self.board_len{0=>3,3=>1,4=>1,_=>return Err("board already complete".into())};
        if cards.len()!=expected{return Err("unexpected board size".into());}
        for &c in cards{self.board[self.board_len]=c;self.board_len+=1;}
        self.street=match self.board_len{3=>1,4=>2,5=>3,_=>unreachable!()};
        self.street_paid.fill(0);self.current=0;self.min_raise=self.min_bet;
        self.queue=ordered_after(self.button,self.stacks.len(),|p|!self.folded[p] && self.stacks[p]>0);
        Ok(())
    }
    fn observe(&self,actor:usize,action:u8,amount:i64,hand_id:u64,index:u16,is_pluribus:bool)->[u8;96]{
        let mut b=[0u8;RECORD_BYTES];
        b[0..8].copy_from_slice(&hand_id.to_le_bytes());
        b[8..10].copy_from_slice(&index.to_le_bytes());
        b[10]=actor as u8;b[11]=self.street;b[12]=action;
        b[13]=self.stacks.len() as u8;b[14]=self.button as u8;
        let to_call=(self.current-self.street_paid[actor]).max(0);
        b[15]=(if to_call>0{1}else{0}) | 2 |
            (if self.stacks[actor]>to_call && self.stacks.iter().enumerate().any(|(p,&v)|p!=actor && !self.folded[p] && v>0){4}else{0});
        b[16..18].copy_from_slice(&self.hole[actor]);
        b[88]=u8::from(is_pluribus); // actor identity, not a model feature
        let big_blind=self.min_bet.clamp(0,u32::MAX as i64) as u32;
        b[89..93].copy_from_slice(&big_blind.to_le_bytes());
        b[18..23].copy_from_slice(&self.board);
        for (offset,value) in [(24,self.pot),(32,to_call),(40,self.stacks[actor]),
            (48,self.min_raise),(56,amount),(64,self.street_paid[actor]),
            (72,self.stacks.iter().enumerate().filter(|(p,_)|!self.folded[*p]).count() as i64),
            (80,self.stacks.iter().enumerate().filter(|(p,_)|!self.folded[*p] && *p!=actor).map(|(_,v)|*v).max().unwrap_or(0))]{
            b[offset..offset+8].copy_from_slice(&value.to_le_bytes());
        }
        b
    }
    fn action(&mut self,p:usize,kind:u8,amount:i64)->Result<(),String>{
        if self.queue.front().copied()!=Some(p){return Err(format!("out of turn player {} expected {:?}",p+1,self.queue.front().map(|i|i+1)));}
        if self.folded[p] || self.stacks[p]==0{return Err("folded/all-in player cannot act".into());}
        let to_call=(self.current-self.street_paid[p]).max(0);
        self.queue.pop_front();
        match kind{
            0=>{
                if to_call==0{return Err("cannot fold facing a free check in strict replay".into());}
                self.folded[p]=true;
            }
            1=>{
                let payment=to_call.min(self.stacks[p]);
                self.stacks[p]-=payment;self.street_paid[p]+=payment;self.total[p]+=payment;self.pot+=payment;
            }
            2=>{
                // PHH cbr amount is the TARGET total wager for this street,
                // not the increment paid on this action. A SB that raises to
                // 300 with 50 already posted pays 250 more.
                let target=amount;
                let payment=target-self.street_paid[p];
                if payment<=0 || payment>self.stacks[p]{return Err("invalid raise target".into());}
                if target<=self.current{return Err("cbr does not exceed prior bet".into());}
                let raise=target-self.current;
                if raise<self.min_raise && payment!=self.stacks[p]{return Err(format!("under-minimum non-all-in raise: {raise}")); }
                self.stacks[p]-=payment;self.total[p]+=payment;self.street_paid[p]=target;self.pot+=payment;
                self.current=target;
                if raise>=self.min_raise {self.min_raise=raise;}
                // Short all-in raises do not reopen action for players who have
                // already acted; all others must still respond.
                self.queue=ordered_after(p,self.stacks.len(),|i|!self.folded[i] && i!=p && self.stacks[i]>0 && self.street_paid[i]<target);
            }
            _=>return Err("invalid action kind".into())
        }
        Ok(())
    }
    fn settle(&self,hand:&Hand)->Result<(),String>{
        // Folded hands are settled exactly; full showdowns are settled only when
        // all competing hands and a five-card board are available.
        let eligible:Vec<usize>=(0..self.stacks.len()).filter(|&p|!self.folded[p] && self.total[p]>0).collect();
        if eligible.is_empty(){return Err("no remaining player".into());}
        let can_score=self.board_len==5 && eligible.iter().all(|&p|self.hole[p].iter().all(|&c|c!=UNKNOWN));
        if eligible.len()>1 && !can_score {return Ok(());}
        // side pots: contributions at successive levels; folded players fund pots
        // but cannot win them. Unmatched wagers automatically return as sole-eligible tiers.
        let mut levels=self.total.clone();levels.sort();levels.dedup();levels.retain(|&x|x>0);
        let mut payouts=vec![0i64;self.stacks.len()];
        let mut previous=0;
        for level in levels{
            let members:Vec<usize>=(0..self.total.len()).filter(|&p|self.total[p]>=level).collect();
            let pot=(level-previous)*members.len() as i64;previous=level;
            let allowed:Vec<usize>=members.into_iter().filter(|&p|!self.folded[p]).collect();
            if allowed.is_empty(){return Err("unclaimable side pot".into());}
            let winners=if allowed.len()==1{allowed}else{
                let best=allowed.iter().map(|&p|crate::score7(&[
                    self.hole[p][0],self.hole[p][1],
                    self.board[0],self.board[1],self.board[2],self.board[3],self.board[4],
                ])).max().unwrap();
                allowed.into_iter().filter(|&p|crate::score7(&[
                    self.hole[p][0],self.hole[p][1],
                    self.board[0],self.board[1],self.board[2],self.board[3],self.board[4],
                ])==best).collect::<Vec<_>>()
            };
            let part=pot/winners.len() as i64;let remainder=pot%winners.len() as i64;
            for (index,p) in winners.into_iter().enumerate(){
                payouts[p]+=part+if (index as i64)<remainder{1}else{0};
            }
        }
        if payouts.iter().sum::<i64>()!=self.pot{return Err("pot not conserved".into());}
        if let Some(ref finish)=hand.finishing{
            for p in 0..finish.len(){
                let computed=self.stacks[p]+payouts[p];
                if computed!=finish[p]{return Err(format!("finishing stack mismatch p{} expected {} actual {}",p+1,finish[p],computed));}
            }
        }
        Ok(())
    }
}
fn reconstruct(h:&Hand,id:u64)->Result<Vec<[u8;96]>,String>{
    let mut s=State::new(h)?;let mut seen=[false;52];let mut out=Vec::new();
    for raw in &h.actions{
        let t:Vec<&str>=raw.split_whitespace().collect();
        if t.len()<2{return Err("short action".into());}
        if t[0]=="d"{
            match t[1]{
                "dh"=>{
                    if t.len()!=4{return Err("invalid private deal".into());}
                    let p=t[2].strip_prefix('p').ok_or("bad dealt player")?.parse::<usize>().map_err(|_|"bad player index")?;
                    if p==0 || p>h.players{return Err("dealt player out of range".into());}
                    let c=decode_cards(t[3])?;
                    if c.len()!=2{return Err("private deal needs 2 cards".into());}
                    for &card in &c{if seen[card as usize]{return Err("duplicate card".into());}seen[card as usize]=true;}
                    s.hole[p-1]=[c[0],c[1]];
                }
                "db"=>{
                    if t.len()!=3{return Err("invalid board deal".into());}
                    let cards=decode_cards(t[2])?;
                    for &c in &cards{if seen[c as usize]{return Err("duplicate board card".into());}seen[c as usize]=true;}
                    s.next_street(&cards)?;
                }
                _=>return Err("unsupported deal type".into()),
            }
            continue;
        }
        let p=t[0].strip_prefix('p').ok_or("missing player prefix")?.parse::<usize>().map_err(|_|"bad actor")?;
        if p==0 || p>h.players{return Err("actor out of range".into());}
        if t[1]=="sm"{continue;}
        let kind=match t[1]{"f"=>0,"cc"=>1,"cbr"=>2,_=>return Err(format!("unsupported action {}",t[1]))};
        let amount=if kind==2{parse_money(t.get(2).copied().ok_or("missing bet")?)?}else{0};
        let index=u16::try_from(out.len()).map_err(|_|"more than 65535 decisions")?;
        let record=s.observe(p-1,kind,amount,id,index,h.player_names[p-1]=="Pluribus");
        s.action(p-1,kind,amount)?;
        out.push(record);
    }
    if !s.queue.is_empty() && s.folded.iter().filter(|&&f|!f).count()>1{
        return Err("hand stopped with outstanding betting actions".into());
    }
    if s.folded.iter().filter(|&&f|!f).count()>1 && s.board_len<5{
        return Err("hand stopped before complete board".into());
    }
    s.settle(h)?;
    Ok(out)
}
fn gather(path:&Path,files:&mut Vec<PathBuf>)->io::Result<()>{
    if path.is_file(){if path.extension().is_some_and(|e|e=="phh"){files.push(path.to_path_buf());}return Ok(());}
    for f in fs::read_dir(path)?{
        let f=f?;
        if f.file_type()?.is_dir(){gather(&f.path(),files)?;}
        else if f.path().extension().is_some_and(|e|e=="phh"){files.push(f.path());}
    }
    Ok(())
}
pub fn run(args:&[String])->io::Result<()>{
    let source=args.windows(2).find(|v|v[0]=="--source").map(|v|v[1].as_str()).ok_or_else(||invalid("missing --source"))?;
    let output=args.windows(2).find(|v|v[0]=="--output").map(|v|v[1].as_str()).unwrap_or("phh-observations.bin");
    let max_files:usize=super::arg(args,"--max-files",usize::MAX);
    let mut paths=Vec::new();gather(Path::new(source),&mut paths)?;paths.sort();
    let mut writer=BufWriter::new(File::create(output)?);writer.write_all(MAGIC)?;
    let mut stats=Stats::default();
    for path in paths.iter().take(max_files){
        stats.files+=1;
        let mut text=String::new();BufReader::new(File::open(path)?).read_to_string(&mut text)?;
        match parse_hand(&text).and_then(|h|reconstruct(&h,stats.files)){
            Ok(records)=>{
                for record in &records{
                    writer.write_all(record)?;
                    stats.by_action[record[12] as usize]+=1;
                }
                stats.decisions+=records.len() as u64;
                stats.accepted+=1;
            }
            Err(e)=>{stats.rejected+=1;eprintln!("REJECT {}: {e}",path.display());}
        }
    }
    writer.flush()?;
    println!("PHH files={} accepted={} rejected={} decisions={} folds={} calls_checks={} raises={} record_bytes={} output={}",
        stats.files,stats.accepted,stats.rejected,stats.decisions,stats.by_action[0],
        stats.by_action[1],stats.by_action[2],RECORD_BYTES,output);
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn pluribus_fold_hand_exact_stacks(){
        let t="variant='NT'\nplayers=['A','B','C','D','E','F']\nstarting_stacks=[10000,10000,10000,10000,10000,10000]\nfinishing_stacks=[10310,9900,10000,9790,10000,10000]\nantes=[0,0,0,0,0,0]\nblinds_or_straddles=[50,100,0,0,0,0]\nmin_bet=100\nactions=['d dh p1 TcQc','d dh p2 8s4c','d dh p3 9c3d','d dh p4 Ah4h','d dh p5 Th5s','d dh p6 6c7s','p3 f','p4 cbr 210','p5 f','p6 f','p1 cc','p2 f','d db 7d5h9d','p1 cc','p4 cc','d db 7c','p1 cc','p4 cc','d db Qh','p1 cbr 230','p4 f']";
        let h=parse_hand(t).unwrap();
        let o=reconstruct(&h,1).unwrap();
        assert_eq!(o.len(),12);
        assert_eq!(o[0][10],2);
        assert_eq!(i64::from_le_bytes(o[0][24..32].try_into().unwrap()),15000);
        assert_eq!(o[0][16],7); // 9c is rank index 7
    }
    #[test] fn blind_raise_is_to_total_not_increment(){
        let t="variant='NT'\nplayers=['A','B','C','D','E','F']\nstarting_stacks=[10000,10000,10000,10000,10000,10000]\nfinishing_stacks=[9700,10300,10000,10000,10000,10000]\nantes=[0,0,0,0,0,0]\nblinds_or_straddles=[50,100,0,0,0,0]\nmin_bet=100\nactions=['d dh p1 Ac5d','d dh p2 Jh8h','d dh p3 KhTs','d dh p4 Qh9s','d dh p5 Th5s','d dh p6 7c2d','p3 f','p4 f','p5 f','p6 f','p1 cbr 300','p2 cc','d db 2sQs9c','p1 cc','p2 cbr 450','p1 f']";
        let h=parse_hand(t).unwrap();
        let records=reconstruct(&h,19).unwrap();
        assert_eq!(records.len(),9);
    }
    #[test] fn incomplete_hand_rejected(){
        let t="variant='NT'\nplayers=['A','B']\nstarting_stacks=[100,100]\nantes=[0,0]\nblinds_or_straddles=[1,2]\nmin_bet=2\nactions=['p1 cc']";
        let h=parse_hand(t).unwrap();assert!(reconstruct(&h,1).is_err());
    }
    #[test] fn incomplete_hand_after_closed_betting_round_rejected(){
        let t="variant='NT'\nplayers=['A','B']\nstarting_stacks=[100,100]\nantes=[0,0]\nblinds_or_straddles=[1,2]\nmin_bet=2\nactions=['p1 cc','p2 cc']";
        let h=parse_hand(t).unwrap();
        assert!(reconstruct(&h,1).is_err(),"a closed preflop round does not complete a hand with two live players");
    }
    #[test] fn cards_and_amounts(){
        assert_eq!(card_id("As").unwrap(),51);
        assert_eq!(parse_money("0.25").unwrap(),25);
        assert_eq!(decode_cards("AcKd").unwrap().len(),2);
    }
}

//! Streaming-ish PHH file importer into compact, versioned decision records.
//! Supports the common single-quoted/double-quoted TOML-like PHH arrays,
//! including multiline 'actions'. Does not execute the text as code.
use std::fs::{self,File};
use std::io::{self,BufReader,BufWriter,Read,Write};
use std::path::{Path,PathBuf};
const MAGIC: &[u8;8] = b"SPKPHH01";
const RECORD_SIZE: usize = 16;
#[derive(Default)]
struct Hand {
    variant: String,
    players: usize,
    actions: Vec<String>,
    stacks: usize,
}
#[derive(Default)]
struct Stats { files: u64, hands: u64, decisions: u64, deals: u64, rejected: u64, by_action: [u64;5] }
fn invalid(message: &str) -> io::Error {io::Error::new(io::ErrorKind::InvalidData,message)}
fn quoted_items(input:&str) -> Result<Vec<String>,String> {
    let start=input.find('[').ok_or("missing array")?;
    let end=input.rfind(']').ok_or("unterminated array")?;
    let chars: Vec<char>=input[start+1..end].chars().collect();
    let mut items=Vec::new();
    let mut at=0;
    while at<chars.len() {
        while at<chars.len() && (chars[at].is_whitespace() || chars[at]==',') {at+=1;}
        if at==chars.len(){break;}
        if chars[at]=='#' {
            while at<chars.len() && chars[at]!='\n' {at+=1;}
            continue;
        }
        if chars[at]!='\'' && chars[at]!='"' {return Err("unexpected array element (expected quoted string)".into());}
        let quote=chars[at];at+=1;
        let mut current=String::new();
        let mut closed=false;
        while at<chars.len() {
            let ch=chars[at];at+=1;
            if ch==quote {closed=true;break;}
            if ch=='\\' && at<chars.len() {
                current.push(chars[at]);at+=1;
            } else {current.push(ch);}
        }
        if !closed {return Err("unterminated quoted item".into());}
        items.push(current);
    }
    Ok(items)
}
fn quoted_scalar(raw: &str)->String {
    raw.trim().trim_matches(|c|c=='\'' || c=='"').to_string()
}
fn parse_hand(text:&str)->Result<Hand,String> {
    let mut hand=Hand::default();
    let mut action_array=String::new();
    let mut collecting=false;
    let mut saw_actions=false;
    for line in text.lines() {
        let trimmed=line.trim();
        if collecting {
            action_array.push('\n');
            action_array.push_str(line);
            if trimmed.starts_with(']') {collecting=false;}
            continue;
        }
        let Some((key,value))=trimmed.split_once('=') else {continue};
        let key=key.trim();
        let value=value.trim();
        match key {
            "variant" => hand.variant=quoted_scalar(value),
            "players" => hand.players=quoted_items(value).map_err(|e|format!("players: {e}"))?.len(),
            "starting_stacks" => {
                hand.stacks=value.trim_start_matches('[').split(']').next().unwrap_or("").split(',')
                    .filter(|v| !v.trim().is_empty()).count();
            }
            "actions" => {
                saw_actions=true;
                action_array.push_str(value);
                if !value.contains(']') {collecting=true;}
            }
            _=>{}
        }
    }
    if collecting {return Err("unterminated actions array".into());}
    if !saw_actions {return Err("missing actions".into());}
    hand.actions=quoted_items(&action_array).map_err(|e|format!("actions: {e}"))?;
    if hand.players<2 || hand.players>10 {return Err("invalid player count".into());}
    if hand.stacks!=hand.players {return Err("starting_stacks mismatch".into());}
    if hand.actions.is_empty() {return Err("empty actions".into());}
    Ok(hand)
}
fn card_id(card: &str)->Option<u8> {
    let b=card.as_bytes();
    if b.len()!=2 {return None;}
    let rank=b"23456789TJQKA".iter().position(|&x|x==b[0])?;
    let suit=b"cdhs".iter().position(|&x|x==b[1])?;
    Some((suit*13+rank) as u8)
}
fn cards(input:&str)->Option<Vec<u8>> {
    if input.len()%2!=0 {return None;}
    input.as_bytes().chunks_exact(2).map(|chunk|card_id(std::str::from_utf8(chunk).ok()?)).collect()
}
fn process(hand:&Hand, writer:&mut BufWriter<File>,stats:&mut Stats, hand_id:u64)->Result<(),String> {
    // Only NT is accepted for policy-dataset import. Other variants are counted
    // as rejected rather than silently presented as Texas Hold'em.
    if hand.variant!="NT" {return Err(format!("unsupported variant {}",hand.variant));}
    let mut known=[false;52];
    let mut street=0u8;
    let mut board=0usize;
    let mut decision=0u16;
    for action in &hand.actions {
        let tok:Vec<&str>=action.split_whitespace().collect();
        if tok.is_empty(){continue;}
        if tok.len()>=4 && tok[0]=="d" && tok[1]=="dh" {
            let c=cards(tok[3]).ok_or("invalid hole cards")?;
            if c.len()!=2 {return Err("hole cards must have exactly two cards".into());}
            for card in c {if known[card as usize] {return Err("duplicate dealt card".into());}known[card as usize]=true;}
            stats.deals+=1;
            continue;
        }
        if tok.len()>=3 && tok[0]=="d" && tok[1]=="db" {
            let c=cards(tok[2]).ok_or("invalid community cards")?;
            for card in &c {if known[*card as usize] {return Err("duplicate community card".into());}known[*card as usize]=true;}
            board+=c.len();
            if board>5{return Err("too many board cards".into());}
            street=match board {0=>0,3=>1,4=>2,5=>3,_=>return Err("invalid board progression".into())};
            continue;
        }
        if !tok[0].starts_with('p') {return Err(format!("unknown action {action}"));}
        if tok.len()<2 {return Err("truncated player action".into());}
        let actor=tok[0][1..].parse::<usize>().map_err(|_|"invalid player index")?;
        if actor==0 || actor>hand.players{return Err("player index out of range".into());}
        // PHH: 'cc' check/call, 'cbr <amount>' completion/bet/raise,
        // 'f' fold; 'sm' showdown/muck is an observation, not a decision.
        let kind=match tok[1] {
            "f"=>0u8,
            "cc"=>1u8,
            "cbr"=>2u8,
            "sm"=> {continue;},
            other=>return Err(format!("unsupported action {other}")),
        };
        let amount=if kind==2 {
            if tok.len()<3 {return Err("missing bet amount".into());}
            let n=tok[2].parse::<f64>().map_err(|_|"bad bet amount")?;
            if !n.is_finite() || n<0.0 || n>u32::MAX as f64{return Err("bet amount out of range".into());}
            n.round() as u32
        } else {0};
        let mut record=[0u8;RECORD_SIZE];
        record[0..8].copy_from_slice(&hand_id.to_le_bytes());
        record[8..10].copy_from_slice(&decision.to_le_bytes());
        record[10]=actor as u8 - 1;
        record[11]=street;
        record[12]=kind;
        // u24 fixed currency/chip units; actions above this reject explicitly.
        if amount>0xFF_FFFF {return Err("bet amount exceeds u24".into());}
        record[13]=(amount&255) as u8;
        record[14]=((amount>>8)&255) as u8;
        record[15]=((amount>>16)&255) as u8;
        writer.write_all(&record).map_err(|e|e.to_string())?;
        decision=decision.checked_add(1).ok_or("too many actions")?;
        stats.decisions+=1;
        stats.by_action[kind as usize]+=1;
    }
    Ok(())
}
fn files_recursively(path:&Path,files:&mut Vec<PathBuf>)->io::Result<()> {
    if path.is_file() {
        if path.extension().is_some_and(|e|e=="phh"){files.push(path.to_path_buf());}
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry=entry?;
        if entry.file_type()?.is_dir() {files_recursively(&entry.path(),files)?;}
        else if entry.path().extension().is_some_and(|e|e=="phh"){files.push(entry.path());}
    }
    Ok(())
}
pub fn run(args:&[String])->io::Result<()> {
    let source=args.windows(2).find(|a|a[0]=="--source").map(|x|x[1].as_str())
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --source"))?;
    let output=args.windows(2).find(|a|a[0]=="--output").map(|x|x[1].as_str()).unwrap_or("phh-decisions.bin");
    let max_files:usize=super::arg(args,"--max-files",usize::MAX);
    let mut files=Vec::new();
    files_recursively(Path::new(source),&mut files)?;
    files.sort();
    let mut writer=BufWriter::new(File::create(output)?);
    writer.write_all(MAGIC)?;
    let mut stats=Stats::default();
    for path in files.iter().take(max_files) {
        let mut text=String::new();
        BufReader::new(File::open(path)?).read_to_string(&mut text)?;
        stats.files+=1;
        match parse_hand(&text) {
            Ok(hand)=> {
                let id=stats.files;
                if let Err(error)=process(&hand,&mut writer,&mut stats,id) {
                    stats.rejected+=1;
                    eprintln!("REJECT {}: {error}",path.display());
                } else {stats.hands+=1;}
            }
            Err(error)=>{stats.rejected+=1;eprintln!("REJECT {}: {error}",path.display());}
        }
    }
    writer.flush()?;
    println!("PHH files={} accepted={} rejected={} decisions={} deal_events={} folds={} checks_calls={} raises={} output={}",
        stats.files,stats.hands,stats.rejected,stats.decisions,stats.deals,
        stats.by_action[0],stats.by_action[1],stats.by_action[2],output);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn sample_pluribus(){
        let text="variant = 'NT'\nstarting_stacks = [10000,10000]\nplayers = ['A','B']\nactions = ['d dh p1 TcQc','d dh p2 8s4c','p1 cc','p2 cbr 200','p1 f']";
        let h=parse_hand(text).unwrap();
        assert_eq!(h.players,2);
        assert_eq!(h.actions.len(),5);
    }
    #[test] fn multiline_with_comments(){
        let h=parse_hand("variant = 'NT'\nstarting_stacks = [100, 100]\nplayers = ['a','b']\nactions = [\n 'd dh p1 AcKc', # comment\n 'd dh p2 JhJs',\n 'p1 cc',\n]").unwrap();
        assert_eq!(h.actions.len(),3);
    }
    #[test] fn card_codes(){
        assert_eq!(card_id("2c"),Some(0));
        assert_eq!(card_id("As"),Some(51));
    }
}

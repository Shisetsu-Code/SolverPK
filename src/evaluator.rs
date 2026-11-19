//! Allocation-free exact 5/7-card hand ranking.
//! Scores are directly comparable: category in the high nibble, followed by
//! up to five four-bit tie breakers (A=14).
fn straight_high(mask: u16) -> u8 {
    for high in (6..=14).rev() {
        let needed = 0b11111u16 << (high - 4);
        if mask & needed == needed { return high as u8; }
    }
    // Wheel: A-2-3-4-5, represented as a five-high straight.
    if (mask & ((1<<14)|(1<<2)|(1<<3)|(1<<4)|(1<<5)))
        == ((1<<14)|(1<<2)|(1<<3)|(1<<4)|(1<<5)) { return 5; }
    0
}
fn packed(category: u8, values: [u8; 5]) -> u64 {
    let mut score = category as u64;
    for value in values { score = (score << 4) | value as u64; }
    score
}
pub fn evaluate(cards: &[u8]) -> u64 {
    assert!((5..=7).contains(&cards.len()));
    let mut count = [0u8; 15];
    let mut suits = [0u8; 4];
    let mut suit_masks = [0u16; 4];
    let mut ranks = 0u16;
    for &card in cards {
        assert!(card < 52);
        let rank = card % 13 + 2;
        let suit = (card / 13) as usize;
        count[rank as usize] += 1;
        suits[suit] += 1;
        suit_masks[suit] |= 1u16 << rank;
        ranks |= 1u16 << rank;
    }
    for suit in 0..4 {
        if suits[suit] >= 5 {
            let high = straight_high(suit_masks[suit]);
            if high > 0 { return packed(8, [high,0,0,0,0]); }
        }
    }
    for r in (2..=14).rev() {
        if count[r] == 4 {
            let kicker = (2..=14).rev().find(|&x| x != r && count[x] > 0).unwrap() as u8;
            return packed(7, [r as u8,kicker,0,0,0]);
        }
    }
    let top_trips = (2..=14).rev().find(|&r| count[r] >= 3);
    if let Some(trips) = top_trips {
        if let Some(pair) = (2..=14).rev().find(|&r| r != trips && count[r] >= 2) {
            return packed(6, [trips as u8,pair as u8,0,0,0]);
        }
    }
    for suit in 0..4 {
        if suits[suit] >= 5 {
            let mut result=[0u8;5];
            let mut index=0;
            for r in (2..=14).rev() {
                if suit_masks[suit] & (1u16<<r) != 0 {
                    result[index]=r as u8;
                    index+=1;
                    if index==5 {break;}
                }
            }
            return packed(5,result);
        }
    }
    let straight=straight_high(ranks);
    if straight > 0 { return packed(4,[straight,0,0,0,0]); }
    if let Some(trips)=top_trips {
        let mut values=[trips as u8,0,0,0,0];
        let mut index=1;
        for r in (2..=14).rev() {
            if r != trips && count[r] > 0 {
                values[index]=r as u8;
                index+=1;
                if index==3 {break;}
            }
        }
        return packed(3,values);
    }
    let mut pairs=[0u8;3];
    let mut n=0;
    for r in (2..=14).rev() {
        if count[r] >= 2 && n<3 {pairs[n]=r as u8;n+=1;}
    }
    if n>=2 {
        let kicker=(2..=14).rev().find(|&r|r != pairs[0] as usize && r != pairs[1] as usize && count[r]>0).unwrap() as u8;
        return packed(2,[pairs[0],pairs[1],kicker,0,0]);
    }
    if n==1 {
        let mut values=[pairs[0],0,0,0,0];
        let mut index=1;
        for r in (2..=14).rev() {
            if r != pairs[0] as usize && count[r]>0 {
                values[index]=r as u8;
                index+=1;
                if index==4 {break;}
            }
        }
        return packed(1,values);
    }
    let mut values=[0u8;5];
    let mut index=0;
    for r in (2..=14).rev() {
        if count[r]>0 {
            values[index]=r as u8;
            index+=1;
            if index==5 {break;}
        }
    }
    packed(0,values)
}
pub fn score5(cards: &[u8;5]) -> u64 {evaluate(cards)}
pub fn score7(cards: &[u8;7]) -> u64 {evaluate(cards)}
#[cfg(test)]
#[path = "evaluator_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "evaluator_external_tests.rs"]
mod external_tests;

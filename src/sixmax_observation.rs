//! Versioned observations for the neural actor. Only the observing player's
//! hole cards, the visible board, and public betting information are read.
//!
//! SuitAwareV2 is 128D. Card indices use 13 ranks (2..A) within each suit
//! (clubs, diamonds, hearts, spades), matching the engine's 0..51 numbering.
//!   0..52:  hero card membership; 52..104: visible board membership.
//! Both planes preserve each card's rank/suit association and ignore card order.
//! Public fields 104..128, each bounded to [0,1] for legal table states:
//!   104 round/3; 105 alive/6; 106 pot/(6*STACK); 107 call/STACK;
//!   108 hero stack/STACK; 109 on button; 110 texture/3; 111 pressure/2;
//!   112 range bucket/2; 113 hero raise count/255; 114 high/STACK;
//!   115..118 mean active-opponent premium/medium/weak beliefs;
//!   118 folded/6; 119 hero total/STACK; 120 hero street/STACK;
//!   121 visible/5; 122 min_raise/STACK; 123 distance from button/5;
//!   124 live bettors/6; 125 hero acted; 126 max live opponent stack/STACK;
//!   127 eligible pending opponents/5.
use super::{Table6, N, STACK};
use crate::neural::{ObservationVersion, INPUT};

pub(super) fn input(t: &Table6, p: usize, version: ObservationVersion) -> [f32; INPUT] {
    match version {
        ObservationVersion::LegacyV1 => legacy_input(t, p),
        ObservationVersion::SuitAwareV2 => suit_aware_input(t, p),
    }
}

/// Freeze the historical first 64 coordinates exactly, including the original
/// card order and scaling. The unused tail is zero, and a V1 network consumes
/// only those first 64 coordinates. Historical weights are never reinterpreted
/// as weights for the V2 card planes.
fn legacy_input(t: &Table6, p: usize) -> [f32; INPUT] {
    let v=t.view(p);
    let mut x=[0f32;INPUT];
    x[(v.hole[0]%13) as usize]=1.0;
    x[13+(v.hole[1]%13) as usize]=1.0;
    x[26]=f32::from(v.hole[0]/13==v.hole[1]/13);
    x[27]=f32::from(v.hole[0]%13==v.hole[1]%13);
    for &card in &v.board[..v.visible] {
        x[28+(card%13) as usize]+=0.25;
        x[41+(card/13) as usize]+=0.20;
    }
    x[45]=v.street as f32/3.0;
    x[46]=v.active as f32/6.0;
    x[47]=(v.pot as f32/240.0).min(3.0);
    x[48]=(v.call as f32/200.0).min(1.0);
    x[49]=v.stack as f32/200.0;
    x[50]=f32::from(v.on_button);
    x[51]=v.texture as f32/3.0;
    x[52]=v.pressure as f32/2.0;
    x[53]=v.range_bucket as f32/2.0;
    x[54]=t.raise_per_seat[p] as f32/5.0;
    x[55]=(t.high as f32/200.0).min(3.0);
    let mut n=0f32;
    let mut posterior=[0f32;3];
    for q in 0..N {
        if q!=p && !t.folded[q] {
            n+=1.0;
            for k in 0..3 {posterior[k]+=t.estimated_ranges[q][k];}
        }
    }
    if n>0.0{for k in 0..3 {x[56+k]=posterior[k]/n;}}
    x[59]=t.folded.iter().filter(|&&f|f).count() as f32/6.0;
    x[60]=t.total[p] as f32/200.0;
    x[61]=t.street[p] as f32/200.0;
    x[62]=v.visible as f32/5.0;
    x[63]=t.min_raise as f32/200.0;
    x
}

fn suit_aware_input(t: &Table6, p: usize) -> [f32; INPUT] {
    let v = t.view(p);
    let mut x = [0.0f32; INPUT];
    for card in v.hole {
        x[card as usize] = 1.0;
    }
    for &card in &v.board[..v.visible] {
        x[52 + card as usize] = 1.0;
    }

    let stack = STACK as f32;
    let seats = N as f32;
    x[104] = v.street as f32 / 3.0;
    x[105] = v.active as f32 / seats;
    x[106] = v.pot as f32 / (stack * seats);
    x[107] = v.call as f32 / stack;
    x[108] = v.stack as f32 / stack;
    x[109] = f32::from(v.on_button);
    x[110] = v.texture as f32 / 3.0;
    x[111] = v.pressure as f32 / 2.0;
    x[112] = v.range_bucket as f32 / 2.0;
    x[113] = t.raise_per_seat[p] as f32 / u8::MAX as f32;
    x[114] = t.high as f32 / stack;

    let mut opponents = 0.0f32;
    let mut posterior = [0.0f32; 3];
    let mut max_live_stack = 0i64;
    let mut pending_opponents = 0usize;
    for q in 0..N {
        if q != p && !t.folded[q] {
            opponents += 1.0;
            for k in 0..3 {
                posterior[k] += t.estimated_ranges[q][k];
            }
            max_live_stack = max_live_stack.max(t.stack[q]);
            if t.pending[q] && t.stack[q] > 0 {
                pending_opponents += 1;
            }
        }
    }
    if opponents > 0.0 {
        for k in 0..3 {
            x[115 + k] = posterior[k] / opponents;
        }
    }
    x[118] = t.folded.iter().filter(|&&f| f).count() as f32 / seats;
    x[119] = t.total[p] as f32 / stack;
    x[120] = t.street[p] as f32 / stack;
    x[121] = v.visible as f32 / 5.0;
    x[122] = t.min_raise as f32 / stack;
    x[123] = ((p + N - t.button) % N) as f32 / (N - 1) as f32;
    x[124] = t.live_bettors() as f32 / seats;
    x[125] = f32::from(t.acted[p]);
    x[126] = max_live_stack as f32 / stack;
    x[127] = pending_opponents as f32 / (N - 1) as f32;
    x
}

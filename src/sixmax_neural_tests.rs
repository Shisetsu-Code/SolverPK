use super::*;

#[test]
fn monetary_return_keeps_large_losses_and_gains_on_one_linear_scale(){
    for (bb,want) in [(-100,-1.0f32),(20,0.2),(500,5.0)] {
        assert!((neural_reward(bb*BB_UNITS)-want).abs()<1e-6,
            "terminal utility changed the economic size of {bb} BB");
    }
}

#[test]
fn losing_lottery_is_not_given_a_positive_learning_return(){
    // Hand-calculated EV: 82% *20 BB -18% *100 BB = -1.6 BB.
    let mean=0.82*neural_reward(20*BB_UNITS)+0.18*neural_reward(-100*BB_UNITS);
    assert!((mean-(-0.016)).abs()<1e-6,"negative EV became utility {mean}");
}

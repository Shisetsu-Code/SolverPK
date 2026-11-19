use std::process::Command;

fn observation(hand_id:u64,action:u8,hole:[u8;2])->[u8;96]{
    let mut row=[0u8;96];
    row[..8].copy_from_slice(&hand_id.to_le_bytes());
    row[10]=1;
    row[12]=action;
    row[13]=2;
    row[15]=0b111;
    row[16..18].copy_from_slice(&hole);
    row[18..23].fill(255);
    for (offset,value) in [(24,3i64),(32,1),(40,100),(48,2),(72,2)]{
        row[offset..offset+8].copy_from_slice(&value.to_le_bytes());
    }
    row
}

#[test]
fn heldout_accuracy_does_not_count_every_tied_action_as_correct(){
    let dir=std::env::temp_dir().join(format!("solverpk-phh-tied-accuracy-{}",std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source=dir.join("observations.bin");
    let model=dir.join("model.bin");
    let mut data=Vec::from(*b"SPKOBS02");
    // Different hole-card buckets: the heldout row has no training examples.
    data.extend_from_slice(&observation(1,1,[12,24]));
    data.extend_from_slice(&observation(5,2,[0,1]));
    std::fs::write(&source,data).unwrap();
    let result=Command::new(env!("CARGO_BIN_EXE_solverpk"))
        .arg("--train-phh").arg("--source").arg(&source)
        .arg("--output").arg(&model).output().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    let stdout=String::from_utf8(result.stdout).unwrap();
    // All three probabilities tie at 1/3. Stable argmax selects fold (0),
    // whereas this example raises (2), so its top-1 accuracy must be zero.
    assert!(stdout.contains("heldout_accuracy=0.0000"),"{stdout}");
    assert!(stdout.contains("heldout_log_loss=1.09861"),"{stdout}");
}

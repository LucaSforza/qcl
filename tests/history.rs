use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn history_add_line_and_save_load_round_trip() {
    let marker = format!(
        ":history-regression-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    );
    assert_eq!(linenoise::history_set_max_len(100), 1);
    assert_eq!(linenoise::history_add(&marker), 1);

    let mut found = false;
    for index in 0..100 {
        if linenoise::history_line(index).as_deref() == Some(marker.as_str()) {
            found = true;
            break;
        }
    }
    assert!(found, "history line was not retained");

    let path = std::env::temp_dir().join(format!("qcl-history-{marker}.txt"));
    let filename = path.to_str().expect("temporary path is UTF-8");
    assert_eq!(linenoise::history_save(filename), 0);
    let saved = fs::read_to_string(&path).expect("saved history");
    assert!(saved.lines().any(|line| line == marker));

    assert_eq!(linenoise::history_set_max_len(1), 1);
    assert_eq!(linenoise::history_add(":history-other"), 1);
    assert_eq!(linenoise::history_load(filename), 0);
    assert_eq!(linenoise::history_line(0).as_deref(), Some(marker.as_str()));

    fs::remove_file(path).expect("remove temporary history");
    assert_eq!(linenoise::history_set_max_len(100), 1);
}

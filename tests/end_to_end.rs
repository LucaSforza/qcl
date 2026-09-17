use std::path::PathBuf;

use qcl::repl::{Repl, ReplError};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn dispatcher_covers_load_validate_and_all_queries() {
    let mut repl = Repl::new();

    let loaded = repl
        .execute_line(&format!(":load {}", fixture("weak_playable.qcl").display()))
        .expect("fixture loads");
    assert_eq!(loaded.text, "loaded 2 states, 2 agents, 2 properties");

    assert_eq!(
        repl.execute_line(":validate").expect("valid model").text,
        "model valid"
    );
    assert_eq!(
        repl.execute_line(":check s0 ready")
            .expect("check resolves")
            .text,
        "true"
    );
    assert_eq!(
        repl.execute_line(":check s1 ready")
            .expect("check resolves")
            .text,
        "false"
    );
    assert_eq!(
        repl.execute_line(":check s0 <any> true")
            .expect("existential resolves")
            .text,
        "true"
    );
    assert_eq!(
        repl.execute_line(":check s0 [any] ready")
            .expect("universal resolves")
            .text,
        "false"
    );
    assert_eq!(
        repl.execute_line(":states ready")
            .expect("states resolves")
            .text,
        "s0"
    );
    assert_eq!(
        repl.execute_line(":infer ready |- done")
            .expect("inference resolves")
            .text,
        "s0"
    );
    assert_eq!(
        repl.execute_line(":coalitions includes(alice)")
            .expect("coalition predicate resolves")
            .text,
        "{alice}\n{alice, bob}"
    );
    assert!(
        repl.execute_line(":help")
            .expect("help")
            .text
            .contains(":load FILE")
    );
    assert!(repl.execute_line(":quit").expect("quit").quit);
}

#[test]
fn majority_voting_fixture_supports_ability_and_inference_queries() {
    let mut repl = Repl::new();

    let loaded = repl
        .execute_line(&format!(
            ":load {}",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join("majority_voting.qcl")
                .display()
        ))
        .expect("majority fixture loads");
    assert_eq!(loaded.text, "loaded 2 states, 3 agents, 2 properties");
    assert_eq!(
        repl.execute_line(":validate")
            .expect("majority model valid")
            .text,
        "model valid"
    );

    assert_eq!(
        repl.execute_line(":coalitions size >= 2")
            .expect("cardinality predicate resolves")
            .text,
        "{alice, bob}\n{alice, carol}\n{bob, carol}\n{alice, bob, carol}"
    );
    assert_eq!(
        repl.execute_line(":check coffee_outcome <size >= 2> coffee")
            .expect("existential ability resolves")
            .text,
        "true"
    );
    assert_eq!(
        repl.execute_line(":check coffee_outcome [size >= 2] (coffee & tea)")
            .expect("universal ability resolves")
            .text,
        "false"
    );
    assert_eq!(
        repl.execute_line(":infer <size >= 2> coffee |- <size >= 2> (coffee | tea)")
            .expect("valid inference resolves")
            .text,
        "(none)"
    );
    assert_eq!(
        repl.execute_line(":infer <size >= 2> coffee |- coffee")
            .expect("invalid inference resolves")
            .text,
        "tea_outcome"
    );
}

#[test]
fn tutorial_is_a_concrete_no_model_starting_point() {
    let mut repl = Repl::new();
    let tutorial = repl.execute_line(":tutorial").expect("tutorial").text;

    for expected in [
        "# Tutorial: majority voting",
        "examples/majority_voting.qcl",
        "agents { alice, bob, carol };",
        "effectivity {",
        ":validate",
        ":coalitions size >= 2",
        ":check coffee_outcome [size >= 2] coffee",
        ":check tea_outcome <includes(alice)> coffee",
        ":infer coffee |- !tea",
        ":infer coffee |- tea",
        ":infer <any> coffee |- coffee",
    ] {
        assert!(tutorial.contains(expected), "tutorial missing `{expected}`");
    }
}

#[test]
fn dispatcher_reports_missing_files_and_invalid_models() {
    let mut repl = Repl::new();
    let missing = fixture("does-not-exist.qcl");
    let error = repl
        .execute_line(&format!(":load {}", missing.display()))
        .expect_err("missing file should fail");
    assert!(matches!(error, ReplError::Io { path, .. } if path == missing));

    let invalid = fixture("invalid.qcl");
    let error = repl
        .execute_line(&format!(":load {}", invalid.display()))
        .expect_err("malformed model should fail");
    assert!(matches!(error, ReplError::Parse(_)));
}

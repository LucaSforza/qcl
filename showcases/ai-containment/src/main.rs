use std::env;

use ai_containment::{Scenario, run_audit, run_simulation};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, flag, scenario] if command == "audit" && flag == "--scenario" => {
            scenario.parse::<Scenario>().map(run_audit)
        }
        [command, flag, scenario] if command == "simulate" && flag == "--scenario" => {
            scenario.parse::<Scenario>().map(run_simulation)
        }
        [command] if command == "audit" => Ok(run_audit(Scenario::Hardened)),
        [command] if command == "simulate" => Ok(run_simulation(Scenario::Hardened)),
        [] => Ok(run_simulation(Scenario::Hardened)),
        _ => Err(
            "usage: ai-containment audit|simulate --scenario <hardened|shared-service-bypass>"
                .to_owned(),
        ),
    };

    match result {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("ai-containment: {error}");
            std::process::exit(2);
        }
    }
}

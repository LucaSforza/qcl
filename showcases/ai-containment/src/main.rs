use std::env;

use ai_containment::{
    LiveScenario, Scenario, run_audit, run_live_audit, run_live_demo, run_live_extract,
    run_simulation,
};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, backend_flag, backend, scenario_flag, scenario]
            if command == "audit"
                && backend_flag == "--backend"
                && scenario_flag == "--scenario" =>
        {
            match backend.as_str() {
                "synthetic" => scenario.parse::<Scenario>().map(run_audit),
                "live" => scenario
                    .parse::<LiveScenario>()
                    .map_err(|error| error)
                    .and_then(|scenario| {
                        run_live_audit(scenario).map_err(|error| error.to_string())
                    }),
                _ => Err(format!("unknown backend `{backend}`")),
            }
        }
        [command, scenario_flag, scenario]
            if command == "extract" && scenario_flag == "--scenario" =>
        {
            scenario
                .parse::<LiveScenario>()
                .and_then(|scenario| run_live_extract(scenario).map_err(|error| error.to_string()))
        }
        [command, scenario_flag, scenario]
            if command == "demo" && scenario_flag == "--scenario" =>
        {
            scenario
                .parse::<LiveScenario>()
                .and_then(|scenario| run_live_demo(scenario).map_err(|error| error.to_string()))
        }
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
            "usage: ai-containment audit --backend <synthetic|live> --scenario <scenario> | \
             simulate --scenario <hardened|shared-service-bypass> | \
             extract|demo --scenario <hardened|shared-service-fetch>"
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

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, flag, scenario] if command == "audit" && flag == "--scenario" => {
            match scenario.parse() {
                Ok(scenario) => match safeops_k8s::run_audit(scenario) {
                    Ok(output) => println!("{output}"),
                    Err(error) => {
                        eprintln!("safeops-k8s audit failed: {error}");
                        std::process::exit(1);
                    }
                },
                Err(error) => {
                    eprintln!("safeops-k8s audit failed: {error}");
                    std::process::exit(2);
                }
            }
        }
        [command] if command == "interactive" => {
            if let Err(error) = safeops_k8s::run_interactive() {
                eprintln!("safeops-k8s interactive failed: {error}");
                std::process::exit(1);
            }
        }
        [] => match safeops_k8s::run_demo() {
            Ok(output) => println!("{output}"),
            Err(error) => {
                eprintln!("safeops-k8s demo failed: {error}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!(
                "usage: safeops-k8s [interactive] | audit --scenario <secure|privilege-escalation|break-glass-bypass>"
            );
            std::process::exit(2);
        }
    }
}

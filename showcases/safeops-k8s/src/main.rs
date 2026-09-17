fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if args.next().is_some() {
        eprintln!("usage: safeops-k8s [interactive]");
        std::process::exit(2);
    }
    match command.as_deref() {
        Some("interactive") => {
            if let Err(error) = safeops_k8s::run_interactive() {
                eprintln!("safeops-k8s interactive failed: {error}");
                std::process::exit(1);
            }
        }
        None => match safeops_k8s::run_demo() {
            Ok(output) => println!("{output}"),
            Err(error) => {
                eprintln!("safeops-k8s demo failed: {error}");
                std::process::exit(1);
            }
        },
        Some(_) => {
            eprintln!("usage: safeops-k8s [interactive]");
            std::process::exit(2);
        }
    }
}

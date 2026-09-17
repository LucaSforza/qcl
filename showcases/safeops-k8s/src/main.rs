fn main() {
    if std::env::args().nth(1).as_deref() == Some("interactive") {
        if let Err(error) = safeops_k8s::run_interactive() {
            eprintln!("safeops-k8s interactive failed: {error}");
            std::process::exit(1);
        }
    } else {
        match safeops_k8s::run_demo() {
            Ok(output) => println!("{output}"),
            Err(error) => {
                eprintln!("safeops-k8s demo failed: {error}");
                std::process::exit(1);
            }
        }
    }
}

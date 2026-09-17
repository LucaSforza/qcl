fn main() {
    match safeops_k8s::run_demo() {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("safeops-k8s demo failed: {error}");
            std::process::exit(1);
        }
    }
}

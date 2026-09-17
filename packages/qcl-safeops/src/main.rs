fn main() {
    match qcl_safeops::run_demo() {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("qcl-safeops demo failed: {error}");
            std::process::exit(1);
        }
    }
}

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use qcl::repl::Repl;

fn main() {
    let mut repl = Repl::new();
    linenoise::history_set_max_len(100);
    linenoise::set_callback(qcl::repl::command_completions);
    let history = qcl::repl::history_path();
    load_history(history.as_deref());

    while let Some(line) = linenoise::input("qcl> ") {
        if line.trim().is_empty() {
            continue;
        }
        linenoise::history_add(&line);
        match repl.execute_line(&line) {
            Ok(result) => {
                if !result.text.is_empty() {
                    println!("{}", result.text);
                }
                if result.quit {
                    break;
                }
            }
            Err(error) => {
                let _ = writeln!(io::stderr(), "error: {error}");
            }
        }
    }

    save_history(history.as_deref());
}

fn load_history(path: Option<&Path>) {
    let Some(path) = path else {
        return;
    };

    match fs::metadata(path) {
        Ok(_) => {
            let filename = path.to_string_lossy();
            if linenoise::history_load(&filename) != 0 {
                eprintln!("warning: cannot load history `{}`", path.display());
            }
        }
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            eprintln!(
                "warning: cannot inspect history `{}`: {error}",
                path.display()
            );
        }
        Err(_) => {}
    }
}

fn save_history(path: Option<&Path>) {
    let Some(path) = path else {
        return;
    };

    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        eprintln!(
            "warning: cannot create history directory `{}`: {error}",
            parent.display()
        );
        return;
    }

    let filename = path.to_string_lossy();
    if linenoise::history_save(&filename) != 0 {
        eprintln!("warning: cannot save history `{}`", path.display());
    }
}

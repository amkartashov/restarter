use restarter::{Config, run};
use std::process;

fn main() {
    let config = Config::new().unwrap_or_else(|err| {
        eprintln!("RESTARTER: configuration error: {}", err);
        process::exit(1);
    });

    match run(config) {
        Ok(code) => process::exit(code),
        Err(msg) => {
            eprintln!("RESTARTER: {}", msg);
            process::exit(1);
        }
    }

}

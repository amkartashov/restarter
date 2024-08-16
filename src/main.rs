use log::{error, info};
use restarter::{configure_logger, run, Config};
use std::process;

fn main() {
    if let Err(err) = configure_logger() {
        eprintln!("RESTARTER: failed to configure logger: {:?}", err);
        process::exit(1);
    };

    let config = Config::new().unwrap_or_else(|err| {
        error!("configuration error: {:?}", err);
        process::exit(1);
    });

    info!("{:?}", config);

    match run(config) {
        Ok(code) => process::exit(code),
        Err(err) => {
            error!("{:?}", err);
            process::exit(1);
        }
    }
}

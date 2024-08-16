use log::{debug, error};
use serde_json::json;
use signal_hook::{consts::signal::*, iterator::Signals};
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt::Debug;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::time::Instant;

pub fn configure_logger() -> Result<(), Box<dyn Error>> {
    let mut builder = env_logger::Builder::from_env("RESTARTER_LOG");

    let log_encoder = env::var_os("RESTARTER_LOG_ENCODER")
        .and_then(|os_string| os_string.into_string().ok())
        .and_then(|string| string.parse().ok())
        .unwrap_or("console".to_string());

    if log_encoder != "console" && log_encoder != "json" {
        Err("RESTARTER_LOG_ENCODER can be `json` or `console` (default).")?;
    };

    if log_encoder == "console" {
        builder.format(|buf, record| {
            let ts = buf.timestamp_nanos();
            match (record.module_path(), record.file(), record.line()) {
                (Some(m), Some(f), Some(l)) => writeln!(
                    buf,
                    "{} {}\t{}/{}:{} {}",
                    ts,
                    record.level(),
                    m,
                    f,
                    l,
                    record.args(),
                ),
                _ => writeln!(buf, "{} {}\t{}", ts, record.level(), record.args(),),
            }
        });
    } else {
        builder.format(|buf, record| {
            let ts = buf.timestamp_nanos();
            let level = match record.level().as_str() {
                "WARN" => "WARNING", // Google Cloud Logging expects WARNING: https://cloud.google.com/logging/docs/reference/v2/rest/v2/LogEntry#logseverity
                other => other,
            };
            // for details, see https://cloud.google.com/logging/docs/structured-logging
            match (record.module_path(), record.file(), record.line()) {
                (Some(m), Some(f), Some(l)) => writeln!(
                    buf,
                    "{}",
                    json!({
                        "severity": level,
                        "time": ts.to_string(),
                        "message": std::fmt::format(*record.args()),
                        "logging.googleapis.com/sourceLocation": {
                            "function": m, // actually, this is a module name, but we a limited here
                            "file": f,
                            "line": l,
                        },
                    })
                    .to_string(),
                ),
                _ => writeln!(
                    buf,
                    "{}",
                    json!({
                       "severity": level,
                       "time": ts.to_string(),
                       "message": std::fmt::format(*record.args()),
                    })
                    .to_string(),
                ),
            }
        });
    };
    builder.try_init()?;
    Ok(())
}

#[derive(Debug)]
pub struct Config {
    pub binary: OsString,
    pub args: Vec<OsString>,
    pub retry: usize,
    pub fast_fail_seconds: u64,
    pub reset_retries_seconds: u64,
    pub restart_signal: i32,
    pub restart_wait_seconds: u64,
}

impl Config {
    pub fn new() -> Result<Config, &'static str> {
        let mut args = env::args_os();
        args.next(); // remove itself

        let binary = args.next().ok_or("No binary to spawn")?;

        let args: Vec<OsString> = args.collect();

        let retry = env::var_os("RESTARTER_RETRIES")
            .and_then(|os_string| os_string.into_string().ok())
            .and_then(|string| string.parse().ok())
            .unwrap_or(3);

        let fast_fail_seconds = env::var_os("RESTARTER_FAST_FAIL_SECONDS")
            .and_then(|os_string| os_string.into_string().ok())
            .and_then(|string| string.parse().ok())
            .unwrap_or(1);

        let reset_retries_seconds = env::var_os("RESTARTER_RESET_RETRIES_SECONDS")
            .and_then(|os_string| os_string.into_string().ok())
            .and_then(|string| string.parse().ok())
            .unwrap_or(3600);

        let restart_signal = env::var_os("RESTARTER_RESTART_SIGNAL")
            .and_then(|os_string| os_string.into_string().ok())
            .and_then(|string| string.parse().ok())
            .unwrap_or(0);

        let restart_wait_seconds = env::var_os("RESTARTER_RESTART_WAIT_SECONDS")
            .and_then(|os_string| os_string.into_string().ok())
            .and_then(|string| string.parse().ok())
            .unwrap_or(30);

        Ok(Config {
            binary,
            args,
            retry,
            fast_fail_seconds,
            reset_retries_seconds,
            restart_signal,
            restart_wait_seconds,
        })
    }
}

pub fn run(config: Config) -> Result<i32, Box<dyn Error>> {
    let mut retry = config.retry;

    // all signals except for
    // SIGFPE, SIGILL, SIGSEGV, SIGBUS, SIGABRT, SIGTRAP, SIGSYS, SIGTTIN, SIGTTOU, SIGSTOP, SIGKILL
    let mut signals = Signals::new(&[
        SIGALRM, SIGCONT, SIGHUP, SIGINT, SIGIO, SIGPIPE, SIGPROF, SIGQUIT, SIGTERM, SIGTSTP,
        SIGURG, SIGUSR1, SIGUSR2, SIGVTALRM, SIGWINCH, SIGXCPU, SIGXFSZ, SIGCHLD,
    ])
    .unwrap();

    let mut child; // child process
    let mut child_pid; // process id
    let mut start; // process start time
    let mut elapsed; // process execution time
    let mut estatus; // process exit status
    let mut ecode; // process exit code

    // variables to handle forced restart
    let mut force_restart = false;
    let mut restart_signal_start = Instant::now(); // time restart signal was received

    // command retry loop
    loop {
        start = Instant::now();

        child = Command::new(&config.binary)
            .args(config.args.iter())
            .spawn()?;

        child_pid = child.id() as libc::pid_t;

        // wait and forward signals
        for sig in &mut signals {
            debug!("Received signal {:?}", sig);
            match sig {
                _x if _x == config.restart_signal => {
                    if config.restart_wait_seconds > 0 {
                        debug!(
                            "Received restart signal, will restart if child exits within next {:?} seconds",
                            config.restart_wait_seconds
                        );
                        restart_signal_start = Instant::now()
                    } else {
                        debug!("Received restart signal, will restart next time child exits");
                    }
                    force_restart = true
                }
                SIGCHLD => {
                    debug!("Child process finished");
                    break;
                }
                _ => {
                    debug!("Forwarding signal to {:?}", child_pid);
                    unsafe {
                        libc::kill(child_pid, sig); // ignoring errors
                    }
                }
            };
        }

        estatus = child.wait()?;
        elapsed = start.elapsed().as_secs();

        error!(
            "child exit status: {}, execution time: {}s",
            estatus, elapsed
        );

        if force_restart {
            if config.restart_wait_seconds > 0 {
                let restart_signal_elapsed = restart_signal_start.elapsed().as_secs();
                debug!(
                    "Child exits {} seconds after restart signal",
                    restart_signal_elapsed
                );
                if restart_signal_elapsed > config.restart_wait_seconds {
                    force_restart = false
                }
            }

            if force_restart {
                force_restart = false;
                debug!("Restarting child because of restart signal");
                continue;
            }
        }

        match estatus.code() {
            // normal exit
            Some(code) => ecode = code,

            // killed by signal
            // https://github.com/krallin/tini/blob/master/src/tini.c#L573
            None => {
                let sig_id = estatus.signal().unwrap();
                ecode = 128 + sig_id;
                // stop retrying if it was termination signal
                if signal_hook::consts::TERM_SIGNALS.contains(&sig_id) {
                    debug!("Killed with term signal {}, stop retrying", sig_id);
                    break;
                };
            }
        }

        if estatus.success() {
            break;
        }

        if config.fast_fail_seconds != 0 {
            if elapsed < config.fast_fail_seconds {
                error!("failing too fast, stop retrying");
                break;
            }
        }

        // retry logic here
        if config.retry > 0 {
            if config.reset_retries_seconds > 0 && elapsed > config.reset_retries_seconds {
                debug!(
                    "last running process was stable for quite long ({}s), reset retries counter",
                    elapsed
                );
                retry = config.retry;
            }

            retry = retry - 1;
            debug!("{} left to retry", retry);
            if retry == 0 {
                break;
            }
        }
    }

    Ok(ecode)
}

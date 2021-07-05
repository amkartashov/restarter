use signal_hook::{consts::signal::*, iterator::Signals};
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::time::Instant;

pub struct Config {
    pub binary: OsString,
    pub args: Vec<OsString>,
    pub retry: usize,
    pub fast_fail_seconds: u64,
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

        Ok(Config {
            binary,
            args,
            retry,
            fast_fail_seconds,
        })
    }
}

#[derive(Debug)]
pub struct RestarterError {
    msg: String,
}

impl RestarterError {
    fn new(msg: &str) -> RestarterError {
        RestarterError {
            msg: msg.to_string(),
        }
    }
}

impl fmt::Display for RestarterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl Error for RestarterError {
    fn description(&self) -> &str {
        &self.msg
    }
}

pub fn run(config: Config) -> Result<i32, RestarterError> {
    let mut retry = config.retry;

    // all signals except for
    // SIGFPE, SIGILL, SIGSEGV, SIGBUS, SIGABRT, SIGTRAP, SIGSYS, SIGTTIN, SIGTTOU, SIGCHLD, SIGSTOP, SIGKILL
    let mut signals = Signals::new(&[
        SIGALRM, SIGCONT, SIGHUP, SIGINT, SIGIO, SIGPIPE, SIGPROF, SIGQUIT, SIGTERM, SIGTSTP,
        SIGURG, SIGUSR1, SIGUSR2, SIGVTALRM, SIGWINCH, SIGXCPU, SIGXFSZ,
    ])
    .unwrap();

    let mut ecode;

    loop {
        let start = Instant::now();

        let mut child = Command::new(&config.binary)
            .args(config.args.iter())
            .spawn()
            .map_err(|err| {
                RestarterError::new(
                    ("failed to execute child: ".to_string() + err.to_string().as_str()).as_str(),
                )
            })?;

        let child_pid = child.id() as libc::pid_t;

        loop {
            // try wait and forward signals

            if let Some(_) = child.try_wait().map_err(|err| {
                RestarterError::new(
                    ("failed to wait on a child: ".to_string() + err.to_string().as_str()).as_str(),
                )
            })? {
                break;
            }

            for sig in signals.pending() {
                eprintln!("RESTARTER: Received signal {:?}", sig);
                eprintln!("RESTARTER: Sending kill to {:?}", child_pid);
                unsafe {
                    libc::kill(child_pid, sig); // ignoring errors
                }
            }
        }

        let estatus = child.wait().expect("RESTARTER: failed to wait on child");

        eprintln!("RESTARTER: child exist status: {}", estatus);

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
                    break;
                };
            }
        }

        if estatus.success() {
            break;
        }

        if config.fast_fail_seconds != 0 {
            if start.elapsed().as_secs() < config.fast_fail_seconds {
                eprintln!("RESTARTER: failing too fast, stop retrying");
                break;
            };
        };

        if retry != 0 {
            retry = retry - 1;
            eprintln!("RESTARTER: {} left to retry", retry);
            if retry == 0 {
                break;
            };
        }
    }

    Ok(ecode)
}

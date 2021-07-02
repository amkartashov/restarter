use signal_hook::{consts::signal::*, iterator::Signals};
use std::env;
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::process;
use std::process::Command;
use std::time::Instant;

fn main() {
    let mut args = env::args_os();
    args.next(); // remove itself

    let binary = args.next().expect("RESTARTER: No binary to spawn!");

    let args: Vec<OsString> = args.collect();

    let mut retry = env::var_os("RESTARTER_RETRIES")
        .and_then(|os_string| os_string.into_string().ok())
        .and_then(|string| string.parse().ok())
        .unwrap_or(3);

    let fast_fail_seconds = env::var_os("RESTARTER_FAST_FAIL_SECONDS")
        .and_then(|os_string| os_string.into_string().ok())
        .and_then(|string| string.parse().ok())
        .unwrap_or(1);

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

        let mut child = Command::new(&binary)
            .args(args.iter())
            .spawn()
            .expect("RESTARTER: failed to execute child");

        let child_pid = child.id() as libc::pid_t;

        loop {
            // try wait and forward signals

            if let Some(_) = child
                .try_wait()
                .expect("RESTARTER: failed to wait on a child")
            {
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

            // killed by signal, stop retrying
            // https://github.com/krallin/tini/blob/master/src/tini.c#L573
            None => {
                let sig_id = estatus.signal().unwrap();
                ecode = 128 + sig_id;
                if signal_hook::consts::TERM_SIGNALS.contains(&sig_id) {
                    break;
                };
            }
        }

        if estatus.success() {
            break;
        }

        if fast_fail_seconds != 0 {
            if start.elapsed().as_secs() < fast_fail_seconds {
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

    process::exit(ecode);
}

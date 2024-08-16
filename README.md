# Restarter

**Restarter** executes specified binary and restarts it in case it fails.

Useful in Kubernetes environment if you want your app restart fast without waiting for orchestrator to recreate container for you.

## Overview

Inspired by this [PR for tini](https://github.com/krallin/tini/pull/146) and this article [Implementing pid1 with Rust and async/await](https://www.fpcomplete.com/rust/pid1/)

The goal of this tool is to restart the service quickly in k8s environment, skipping default [restart](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#restart-policy) delay of 10 seconds.

> Note: **restarter** does not serve as Pid 1 service, f.e. it does not reap all zombies as tini does. If you want this functionality, use pid1 -> restarter -> service, where pid1 may be tini or kubernetes pause. Or just make sure that service takes care of its child processes.

**Restarter** forwards unix signal it receives to a service. So if k8s sends `SIGTERM` to container, you can be sure the service will get it. If service is killed with one of the termination signals (`SIGTERM`, `SIGINT`, `SIGQUIT`), **restarter** won't retry.

If **restarter** exits, it uses service exit code, or 128 + signal number which terminated the service.

## Usage and configuration

**Restarter** takes binary path and args from command line, f.e. `restarter /bin/cat /tmp/text.txt`.

**Restarter** tries to execute binary to completion, this means it will stop retrying as soon as the child process finishes with success.

By default, **restarter** will try to execute binary 3 times. This is controlled by the value of environment variable `RESTARTER_RETRIES`. Set it to `0` to retry indefinitely.

**Restarter** resets retries counter if running process lasts more than 1 hour. You can change this value with `RESTARTER_RESET_RETRIES_SECONDS` and disable resetting with `RESTARTER_RESET_RETRIES_SECONDS=0`.

Also, **restarter** will stop trying if the process fails to fast. By default, if process fails less than in 1 second, **restarter** will exit. You can change this behavior with environment variable `RESTARTER_FAST_FAIL_SECONDS`. If you set it to `0`, **restarter** will not check for fast failures.

---

To sum up, **restater** will stop in below cases:

1. number of retries is over;
2. service fails too fast;
3. service is killed with termination signal;
4. service finished successfully.

### Force restart

Sometimes you want force application restart. But application may be stopped with termination signal or with zero exit code and **restarter** will stop.

With environment variables `RESTARTER_RESTART_SIGNAL` (signal number, not set by default) and `RESTARTER_RESTART_WAIT_SECONDS` (30 by default, 0 means forever) you can ask **restarter** to ignore application exit status and restart it anyway.

F.e. you set `RESTARTER_RESTART_SIGNAL=10` (10 is the number of `SIGUSR1`, see `man 7 signal`) and `RESTARTER_RESTART_WAIT_SECONDS=60`.

You send `SIGUSR1` to **restarter**. Now to stop application you can send `SIGTERM` to **restarter**, it will be forwarded to application, application will stop and **restarter** will start it again.

However, if application stops with zero exit code or by termination signal after 60 seconds, **restarter** won't continue.

This restart won't be counted against `RESTARTER_RETRIES`.

### Logging

Logging is configured via env variables `RESTARTER_LOG` and `RESTARTER_LOG_ENCODER`.

Possible values for `RESTARTER_LOG` can be checked in [env_logger docs](https://docs.rs/env_logger/0.8.4/env_logger/#enabling-logging), you can think that it is `warn` by default. You can disable **restarter** log messages with `RESTARTER_LOG=off` and enable debug with `RESTARTER_LOG=debug`.

`RESTARTER_LOG_ENCODER` allows to set log formatting, by default it is `console` (single line messages). To emit Json messages suitable for Google Cloud Logging, set it to `json`.

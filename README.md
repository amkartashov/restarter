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

Also, **restarter** will stop trying if the process fails to fast. By default, if process fails less than in 1 second, **restarter** will exit. You can change this behavior with environment variable `RESTARTER_FAST_FAIL_SECONDS`. If you set it to `0`, **restarter** will not check for fast failures.

----
To sum up, **restater** will stop in below cases:

1) number of retries is over;
2) service fails too fast;
3) service is killed with termination signal;
4) service finished successfully.

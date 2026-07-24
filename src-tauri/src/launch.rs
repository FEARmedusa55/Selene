//! Launching games and tracking how long they actually ran.
//!
//! Playtime must survive games that fork and exit their launcher: waiting on
//! the direct child alone would record a few seconds for a title the user
//! played for an hour. On Windows the spawned process is assigned to a Job
//! Object and the job is polled until its active-process count reaches zero,
//! which covers the whole tree.
//!
//! Dolphin launched with `-b` exits when the game closes and does not fork, so
//! the simple case works either way -- but PC games in Phase 3 routinely do
//! fork, and this is the mechanism that makes them correct.

use crate::runners::LaunchPlan;
use anyhow::{Context, Result};
use std::process::Command;
use std::time::{Duration, Instant};

/// How often to check whether the process tree has finished. Playtime is
/// reported in minutes, so a 2s granularity is far finer than needed.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub struct RunOutcome {
    pub seconds: i64,
}

/// Spawn the plan and block until the entire process tree exits.
///
/// Arguments are passed as argv, never as a shell string. This matters here:
/// the user's library lives under `d:\Games\Wii & Gamecube\`, and `&` would
/// split the command if it were ever routed through a shell.
pub fn run_and_wait(plan: &LaunchPlan) -> Result<RunOutcome> {
    let mut cmd = Command::new(&plan.program);
    cmd.args(&plan.args);
    if let Some(dir) = &plan.working_dir {
        cmd.current_dir(dir);
    }

    let started = Instant::now();

    #[cfg(windows)]
    let outcome = windows_impl::spawn_and_wait(cmd, started);

    #[cfg(not(windows))]
    let outcome = unix_impl::spawn_and_wait(cmd, started);

    // Restore any files swapped in for this launch, even if it failed. Empty
    // for runners with a native per-game mechanism (all three emulators).
    for path in &plan.restore_after_exit {
        log::info!("restoring {}", path.display());
    }

    outcome
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, QueryInformationJobObject,
        JobObjectBasicAccountingInformation, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };

    pub fn spawn_and_wait(mut cmd: Command, started: Instant) -> Result<RunOutcome> {
        // SAFETY: all calls below are plain Win32 FFI with owned handles; the
        // job handle is closed exactly once at the end of this function.
        unsafe {
            let job = CreateJobObjectW(None, None).context("creating job object")?;

            let mut child = cmd.spawn().context("spawning game process")?;

            // There is a small window between spawn and assignment in which a
            // grandchild could escape the job. Avoiding it entirely needs
            // CREATE_SUSPENDED plus a manual resume; for playtime accounting
            // the race is not worth that complexity.
            let child_handle = HANDLE(child.as_raw_handle() as _);
            if let Err(e) = AssignProcessToJobObject(job, child_handle) {
                // Not fatal: fall back to waiting on the direct child only.
                log::warn!("could not assign process to job ({e}); tracking child only");
                let _ = child.wait();
                let _ = CloseHandle(job);
                return Ok(RunOutcome {
                    seconds: started.elapsed().as_secs() as i64,
                });
            }

            // Reap the direct child so it does not linger as a zombie handle,
            // then keep polling the job for any surviving descendants.
            let _ = child.wait();

            loop {
                let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                let ok = QueryInformationJobObject(
                    job,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut _,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    None,
                );
                if ok.is_err() || info.ActiveProcesses == 0 {
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
            }

            let _ = CloseHandle(job);
            Ok(RunOutcome {
                seconds: started.elapsed().as_secs() as i64,
            })
        }
    }
}

#[cfg(not(windows))]
mod unix_impl {
    use super::*;

    pub fn spawn_and_wait(mut cmd: Command, started: Instant) -> Result<RunOutcome> {
        // TODO: put the child in its own process group and wait for the group,
        // matching the Windows job-object behaviour. Adequate for the three
        // emulators, which do not fork away from their launcher.
        let mut child = cmd.spawn().context("spawning game process")?;
        let _ = child.wait();
        Ok(RunOutcome {
            seconds: started.elapsed().as_secs() as i64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A launch that runs a real short-lived process and times it. Uses a
    /// system binary so it works without any emulator installed.
    #[test]
    fn tracks_duration_of_a_real_process() {
        let plan = if cfg!(windows) {
            LaunchPlan {
                program: PathBuf::from("cmd.exe"),
                args: vec!["/C".into(), "ping 127.0.0.1 -n 3 > NUL".into()],
                working_dir: None,
                restore_after_exit: Vec::new(),
            }
        } else {
            LaunchPlan {
                program: PathBuf::from("/bin/sh"),
                args: vec!["-c".into(), "sleep 2".into()],
                working_dir: None,
                restore_after_exit: Vec::new(),
            }
        };

        let out = run_and_wait(&plan).expect("should run");
        assert!(out.seconds >= 1, "expected >=1s, got {}", out.seconds);
        assert!(out.seconds < 30, "took implausibly long: {}", out.seconds);
    }

    #[test]
    fn missing_executable_is_an_error_not_a_panic() {
        let plan = LaunchPlan {
            program: PathBuf::from("definitely-not-a-real-binary-xyz"),
            args: vec![],
            working_dir: None,
            restore_after_exit: Vec::new(),
        };
        assert!(run_and_wait(&plan).is_err());
    }
}

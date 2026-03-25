// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! Child process manager — spawns and manages the user's web application
//! as a subprocess. Inspired by the Go Worker's child process spawning
//! and the Lambda Web Adapter's extension model.

use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{error, info, warn};

/// Manages the lifecycle of the user's web application process.
pub struct ProcessManager {
    child: Option<Child>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self { child: None }
    }

    /// Spawn the user's web application.
    ///
    /// The `command` string is parsed as a shell command.
    /// Additional environment variables can be passed via `env_vars`.
    pub async fn spawn(
        &mut self,
        command: &str,
        working_dir: Option<&str>,
        env_vars: HashMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(command = %command, "spawning web application");

        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Err("empty startup command".into());
        }

        let program = parts[0];
        let args = &parts[1..];

        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        // Inherit current environment and overlay with provided vars
        cmd.envs(std::env::vars());
        cmd.envs(env_vars);

        let child = cmd.spawn()?;

        info!(
            pid = child.id().unwrap_or(0),
            command = %command,
            "web application process started"
        );

        self.child = Some(child);
        Ok(())
    }

    /// Send a graceful shutdown signal (SIGTERM on Unix, terminate on Windows).
    pub async fn shutdown(&mut self) {
        if let Some(ref mut child) = self.child {
            let pid = child.id().unwrap_or(0);
            info!(pid, "shutting down web application");

            #[cfg(unix)]
            {
                use tokio::signal::unix::SignalKind;
                if let Some(id) = child.id() {
                    unsafe {
                        libc::kill(id as i32, libc::SIGTERM);
                    }
                }
                // Give the process time to shut down gracefully
                tokio::select! {
                    _ = child.wait() => {
                        info!(pid, "web application exited gracefully");
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                        warn!(pid, "web application did not exit in time, killing");
                        let _ = child.kill().await;
                    }
                }
            }

            #[cfg(windows)]
            {
                // On Windows, kill is the primary mechanism
                let _ = child.kill().await;
                info!(pid, "web application terminated");
            }
        }
        self.child = None;
    }

    /// Check if the child process is still running.
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Process has exited
                    false
                }
                Ok(None) => {
                    // Still running
                    true
                }
                Err(e) => {
                    warn!(error = %e, "error checking child process status");
                    false
                }
            }
        } else {
            false
        }
    }

    /// Wait for the child process to exit and return the exit code.
    pub async fn wait(&mut self) -> Option<i32> {
        if let Some(ref mut child) = self.child {
            match child.wait().await {
                Ok(status) => {
                    info!(
                        exit_code = status.code().unwrap_or(-1),
                        "web application process exited"
                    );
                    status.code()
                }
                Err(e) => {
                    error!(error = %e, "error waiting for child process");
                    None
                }
            }
        } else {
            None
        }
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            // Best-effort kill on drop
            let _ = child.start_kill();
        }
    }
}

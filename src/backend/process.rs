use super::BrowserBackend;
use crate::contracts::{ActionPlan, AdapterRequest, AdapterResponse, ExecutionResult, RawSnapshot};
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::process::{Command, Stdio};

pub struct ProcessBackend {
    command: String,
    args: Vec<String>,
}

impl ProcessBackend {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }

    fn request(&self, request: AdapterRequest) -> Result<AdapterResponse> {
        let mut child = Command::new(&self.command)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to start browser adapter {}", self.command))?;
        serde_json::to_writer(child.stdin.as_mut().expect("piped stdin"), &request)?;
        child
            .stdin
            .as_mut()
            .expect("piped stdin")
            .write_all(b"\n")?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "browser adapter failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
        }
        serde_json::from_slice(&output.stdout).context("browser adapter returned invalid JSON")
    }
}

impl BrowserBackend for ProcessBackend {
    fn snapshot(&mut self) -> Result<RawSnapshot> {
        match self.request(AdapterRequest::Snapshot)? {
            AdapterResponse::Snapshot(value) => Ok(value),
            AdapterResponse::Error { code, message } => bail!("adapter {code}: {message}"),
            _ => bail!("adapter returned wrong response kind"),
        }
    }
    fn execute(&mut self, plan: &ActionPlan) -> Result<ExecutionResult> {
        match self.request(AdapterRequest::Execute {
            plan: Box::new(plan.clone()),
        })? {
            AdapterResponse::Executed(value) => Ok(value),
            AdapterResponse::Error { code, message } => bail!("adapter {code}: {message}"),
            _ => bail!("adapter returned wrong response kind"),
        }
    }
}

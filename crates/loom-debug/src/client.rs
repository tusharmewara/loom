//! DAP client — manages one debug adapter session lifecycle.

use crate::transport::DapTransport;
use anyhow::Result;

/// Debug adapter type.
#[derive(Debug, Clone)]
pub enum DebugAdapterType {
    Lldb,
    Dlv,
    DebugPy,
}

impl DebugAdapterType {
    pub fn command(&self) -> (&'static str, Vec<String>) {
        match self {
            DebugAdapterType::Lldb => ("lldb-dap", vec![]),
            DebugAdapterType::Dlv => ("dlv", vec!["dap".to_string()]),
            DebugAdapterType::DebugPy => (
                "debugpy",
                vec!["--listen".to_string(), "0.0.0.0:5678".to_string()],
            ),
        }
    }
}

/// A high-level DAP client wrapping a transport connection.
pub struct DapClient {
    transport: DapTransport,
}

impl DapClient {
    /// Connect to a debug adapter.
    pub fn connect(adapter_type: &DebugAdapterType) -> Result<Self> {
        let (cmd, args) = adapter_type.command();
        let transport = DapTransport::spawn(cmd, &args)?;
        Ok(Self { transport })
    }

    /// Connect with custom command/args.
    pub fn connect_with(command: &str, args: &[String]) -> Result<Self> {
        let transport = DapTransport::spawn(command, args)?;
        Ok(Self { transport })
    }

    /// Initialize the debug session.
    pub fn initialize(&mut self, client_name: &str, adapter_id: &str) -> Result<serde_json::Value> {
        let params = serde_json::json!({
            "clientID": "loom",
            "clientName": client_name,
            "adapterID": adapter_id,
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "supportsVariableType": true,
            "supportsVariablePaging": true,
            "supportsRunInTerminalRequest": true,
            "locale": "en-US",
        });
        self.transport.request("initialize", params)
    }

    /// Launch a debug session with the given configuration.
    pub fn launch(&mut self, config: serde_json::Value) -> Result<()> {
        let _: serde_json::Value = self.transport.request("launch", config)?;
        Ok(())
    }

    /// Attach to a running process.
    pub fn attach(&mut self, config: serde_json::Value) -> Result<()> {
        let _: serde_json::Value = self.transport.request("attach", config)?;
        Ok(())
    }

    /// Signal that configuration is complete.
    pub fn configuration_done(&mut self) -> Result<()> {
        let _: serde_json::Value = self
            .transport
            .request("configurationDone", serde_json::json!({}))?;
        Ok(())
    }

    /// Set breakpoints in a source file.
    pub fn set_breakpoints(
        &mut self,
        source_path: &str,
        lines: &[u32],
    ) -> Result<Vec<serde_json::Value>> {
        let params = serde_json::json!({
            "source": { "path": source_path },
            "breakpoints": lines.iter().map(|l| serde_json::json!({ "line": l })).collect::<Vec<_>>(),
            "lines": lines,
        });
        let result: serde_json::Value = self.transport.request("setBreakpoints", params)?;
        let breakpoints = result["breakpoints"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(breakpoints)
    }

    /// Continue execution.
    pub fn r#continue(&mut self, thread_id: u64) -> Result<()> {
        let _: serde_json::Value = self.transport.request(
            "continue",
            serde_json::json!({
                "threadId": thread_id,
            }),
        )?;
        Ok(())
    }

    /// Step over.
    pub fn next(&mut self, thread_id: u64) -> Result<()> {
        let _: serde_json::Value = self.transport.request(
            "next",
            serde_json::json!({
                "threadId": thread_id,
            }),
        )?;
        Ok(())
    }

    /// Step in.
    pub fn step_in(&mut self, thread_id: u64) -> Result<()> {
        let _: serde_json::Value = self.transport.request(
            "stepIn",
            serde_json::json!({
                "threadId": thread_id,
            }),
        )?;
        Ok(())
    }

    /// Step out.
    pub fn step_out(&mut self, thread_id: u64) -> Result<()> {
        let _: serde_json::Value = self.transport.request(
            "stepOut",
            serde_json::json!({
                "threadId": thread_id,
            }),
        )?;
        Ok(())
    }

    /// Pause execution.
    pub fn pause(&mut self, thread_id: u64) -> Result<()> {
        let _: serde_json::Value = self.transport.request(
            "pause",
            serde_json::json!({
                "threadId": thread_id,
            }),
        )?;
        Ok(())
    }

    /// Get stack trace for a thread.
    pub fn stack_trace(
        &mut self,
        thread_id: u64,
        start_frame: Option<u64>,
        levels: Option<u64>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut params = serde_json::json!({
            "threadId": thread_id,
        });
        if let Some(sf) = start_frame {
            params["startFrame"] = serde_json::json!(sf);
        }
        if let Some(lv) = levels {
            params["levels"] = serde_json::json!(lv);
        }
        let result: serde_json::Value = self.transport.request("stackTrace", params)?;
        let frames = result["stackFrames"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(frames)
    }

    /// Get scopes for a stack frame.
    pub fn scopes(&mut self, frame_id: u64) -> Result<Vec<serde_json::Value>> {
        let result: serde_json::Value = self.transport.request(
            "scopes",
            serde_json::json!({
                "frameId": frame_id,
            }),
        )?;
        let scopes = result["scopes"].as_array().cloned().unwrap_or_default();
        Ok(scopes)
    }

    /// Get variables for a scope or variable reference.
    pub fn variables(&mut self, variables_reference: u64) -> Result<Vec<serde_json::Value>> {
        let result: serde_json::Value = self.transport.request(
            "variables",
            serde_json::json!({
                "variablesReference": variables_reference,
            }),
        )?;
        let vars = result["variables"].as_array().cloned().unwrap_or_default();
        Ok(vars)
    }

    /// Evaluate an expression.
    pub fn evaluate(
        &mut self,
        expression: &str,
        frame_id: Option<u64>,
    ) -> Result<serde_json::Value> {
        let mut params = serde_json::json!({
            "expression": expression,
        });
        if let Some(fid) = frame_id {
            params["frameId"] = serde_json::json!(fid);
        }
        self.transport.request("evaluate", params)
    }

    /// Disconnect from the debug session.
    pub fn disconnect(&mut self) -> Result<()> {
        let _: serde_json::Value = self.transport.request(
            "disconnect",
            serde_json::json!({
                "terminateDebuggee": true,
            }),
        )?;
        Ok(())
    }

    /// Wait for a stopped event.
    pub fn wait_for_stopped(&mut self) -> Result<serde_json::Value> {
        loop {
            let event = self.transport.read_event()?;
            if event["event"] == "stopped" {
                return Ok(event);
            }
        }
    }

    /// Wait for a terminated event.
    pub fn wait_for_terminated(&mut self) -> Result<serde_json::Value> {
        loop {
            let event = self.transport.read_event()?;
            if event["event"] == "terminated" || event["event"] == "exited" {
                return Ok(event);
            }
        }
    }
}

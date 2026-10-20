use serde::{Deserialize, Serialize};

/// Version of the JSON-line IPC protocol implemented by this crate.
pub const IPC_PROTOCOL_VERSION: u32 = 1;

/// Version assumed when a peer omits the `version` field (older daemon).
fn default_protocol_version() -> u32 {
    IPC_PROTOCOL_VERSION
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    /// Protocol version of the sender. Older daemons ignore this field.
    #[serde(default = "default_protocol_version")]
    pub version: u32,
    pub request_id: String,
    pub command: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    /// Protocol version of the responder. Absent in older daemons — defaults.
    #[serde(default = "default_protocol_version")]
    pub version: u32,
    pub request_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcRequest {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            version: IPC_PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            command: command.into(),
            params: serde_json::Value::Null,
        }
    }

    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }

    pub fn to_json_line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap();
        s.push('\n');
        s
    }
}

impl IpcResponse {
    pub fn ok(request_id: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            version: IPC_PROTOCOL_VERSION,
            request_id: request_id.into(),
            status: "ok".to_string(),
            data: Some(data),
            error: None,
        }
    }

    pub fn error(request_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            version: IPC_PROTOCOL_VERSION,
            request_id: request_id.into(),
            status: "error".to_string(),
            data: None,
            error: Some(error.into()),
        }
    }

    pub fn to_json_line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap();
        s.push('\n');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_with_version() {
        let req = IpcRequest::new("health.check");
        assert_eq!(req.version, IPC_PROTOCOL_VERSION);
        let parsed: IpcRequest = serde_json::from_str(req.to_json_line().trim()).unwrap();
        assert_eq!(parsed.version, IPC_PROTOCOL_VERSION);
        assert_eq!(parsed.command, "health.check");
    }

    #[test]
    fn responses_carry_version() {
        let ok = IpcResponse::ok("id-1", serde_json::json!({"a": 1}));
        assert_eq!(ok.version, IPC_PROTOCOL_VERSION);
        let err = IpcResponse::error("id-2", "boom");
        assert_eq!(err.version, IPC_PROTOCOL_VERSION);
    }

    #[test]
    fn response_without_version_defaults_to_current() {
        // An older daemon replies without a "version" field.
        let raw = r#"{"request_id":"id-1","status":"ok","data":"pong"}"#;
        let parsed: IpcResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.version, IPC_PROTOCOL_VERSION);
        assert_eq!(parsed.status, "ok");
    }

    #[test]
    fn request_without_version_defaults_to_current() {
        let raw = r#"{"request_id":"id-1","command":"health.check"}"#;
        let parsed: IpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.version, IPC_PROTOCOL_VERSION);
        assert_eq!(parsed.command, "health.check");
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    pub request_id: String,
    pub command: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
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
            request_id: request_id.into(),
            status: "ok".to_string(),
            data: Some(data),
            error: None,
        }
    }

    pub fn error(request_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
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

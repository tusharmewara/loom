//! LSP client — manages one LSP server session lifecycle.

use crate::transport::LspTransport;
use anyhow::Result;
use lsp_types::*;
use serde_json::Value;

/// A high-level LSP client wrapping a transport connection.
pub struct LspClient {
    transport: LspTransport,
    #[allow(dead_code)]
    server_capabilities: Option<ServerCapabilities>,
}

impl LspClient {
    pub async fn connect(command: &str, args: &[String], root_uri: &str) -> Result<Self> {
        let mut transport = LspTransport::spawn(command, args)?;

        let init_params = serde_json::json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {}
        });
        let result: Value = transport.request("initialize", init_params).await?;
        let caps: InitializeResult = serde_json::from_value(result)?;
        let server_capabilities = Some(caps.capabilities);

        transport
            .notify("initialized", serde_json::json!({}))
            .await?;

        Ok(Self {
            transport,
            server_capabilities,
        })
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.transport
            .request::<Value>("shutdown", serde_json::json!(null))
            .await?;
        self.transport
            .notify("exit", serde_json::json!(null))
            .await?;
        Ok(())
    }

    pub async fn rename(
        &mut self,
        uri: &str,
        line: u32,
        column: u32,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
            "newName": new_name,
        });
        self.transport
            .request::<Value>("textDocument/rename", params)
            .await?;
        Ok(None)
    }

    pub async fn find_references(
        &mut self,
        uri: &str,
        line: u32,
        column: u32,
    ) -> Result<Vec<Location>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
            "context": { "includeDeclaration": true },
        });
        let result: Value = self
            .transport
            .request("textDocument/references", params)
            .await?;
        let locations: Vec<Location> = serde_json::from_value(result)?;
        Ok(locations)
    }

    pub async fn code_action(
        &mut self,
        uri: &str,
        range: Range,
    ) -> Result<Vec<CodeActionOrCommand>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": range.start.line, "character": range.start.character },
                "end": { "line": range.end.line, "character": range.end.character },
            },
            "context": { "diagnostics": [], "triggerKind": 2 },
        });
        let result: Value = self
            .transport
            .request("textDocument/codeAction", params)
            .await?;
        let actions: Vec<CodeActionOrCommand> = serde_json::from_value(result)?;
        Ok(actions)
    }

    pub async fn goto_definition(
        &mut self,
        uri: &str,
        line: u32,
        column: u32,
    ) -> Result<Option<Location>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        let result: Value = self
            .transport
            .request("textDocument/definition", params)
            .await?;
        if let Ok(loc) = serde_json::from_value::<Location>(result.clone()) {
            Ok(Some(loc))
        } else if let Ok(locs) = serde_json::from_value::<Vec<Location>>(result) {
            Ok(locs.into_iter().next())
        } else {
            Ok(None)
        }
    }

    pub async fn document_symbols(&mut self, uri: &str) -> Result<Vec<SymbolInformation>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
        });
        let result: Value = self
            .transport
            .request("textDocument/documentSymbol", params)
            .await?;
        let symbols: Vec<SymbolInformation> = serde_json::from_value(result)?;
        Ok(symbols)
    }
}

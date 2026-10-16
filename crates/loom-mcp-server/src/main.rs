//! Loom MCP Server binary.
//!
//! Run as: loom-mcp-server --loom-dir ~/.loom
//!
//! Communicates via MCP protocol over stdio.
//! Use by registering this as an MCP server in any MCP-compatible tool.

use clap::Parser;

/// Loom MCP Server: exposes the loom ecosystem over the MCP protocol via stdio.
#[derive(Parser)]
#[command(name = "loom-mcp-server")]
struct Args {
    /// The loom agents directory to serve.
    #[arg(long, env = "LOOM_AGENTS_DIR", default_value_t = loom_core::loom_dir())]
    loom_dir: String,
}

fn main() {
    let loom_dir = Args::parse().loom_dir;
    let server = loom_mcp_server::LoomMcpServer::new(&loom_dir);
    if let Err(e) = server.run() {
        eprintln!("loom-mcp-server error: {e}");
        std::process::exit(1);
    }
}

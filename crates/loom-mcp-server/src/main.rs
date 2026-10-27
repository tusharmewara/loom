//! Loom MCP Server binary.
//!
//! Run as: loom-mcp-server --loom-dir ~/.loom
//!
//! Communicates via MCP protocol over stdio.
//! Use by registering this as an MCP server in any MCP-compatible tool.

use clap::Parser;
use loom_core::ControlPlaneTransport;

/// Loom MCP Server: exposes the loom ecosystem over the MCP protocol.
/// Default transport is stdio (existing behavior, unchanged).
/// With --features mcp-control-plane, http and grpc transports are available.
#[derive(Parser)]
#[command(name = "loom-mcp-server")]
struct Args {
    /// The loom agents directory to serve.
    #[arg(long, env = "LOOM_AGENTS_DIR", default_value_t = loom_core::loom_dir())]
    loom_dir: String,
    /// MCP transport: stdio (default), http, grpc.
    #[arg(long, env = "LOOM_MCP_TRANSPORT", default_value = "stdio")]
    transport: String,
}

fn main() {
    let args = Args::parse();
    let loom_dir = args.loom_dir;
    let server = loom_mcp_server::LoomMcpServer::new(&loom_dir);

    let transport: ControlPlaneTransport = match args.transport.to_lowercase().as_str() {
        "stdio" => ControlPlaneTransport::Stdio,
        "http" => ControlPlaneTransport::Http,
        "grpc" => ControlPlaneTransport::Grpc,
        other => {
            eprintln!("loom-mcp-server error: unknown transport '{other}' (use stdio, http, or grpc)");
            std::process::exit(2);
        }
    };

    match transport {
        ControlPlaneTransport::Stdio => {
            if let Err(e) = server.run() {
                eprintln!("loom-mcp-server error: {e}");
                std::process::exit(1);
            }
        }
        ControlPlaneTransport::Http => {
            eprintln!("loom-mcp-server: http transport not yet implemented (use stdio)");
            std::process::exit(3);
        }
        ControlPlaneTransport::Grpc => {
            eprintln!("loom-mcp-server: grpc transport not yet implemented (use stdio)");
            std::process::exit(3);
        }
    }
}

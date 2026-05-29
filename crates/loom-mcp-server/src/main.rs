//! Loom MCP Server binary.
//!
//! Run as: loom-mcp-server --loom-dir ~/.loom
//!
//! Communicates via MCP protocol over stdio.
//! Use by registering this as an MCP server in any MCP-compatible tool.

fn main() {
    let loom_dir = std::env::var("LOOM_AGENTS_DIR")
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| format!("{}/.loom", h))
                .unwrap_or_else(|_| "~/.loom".to_string())
        });

    // Also check for --loom-dir argument
    let args: Vec<String> = std::env::args().collect();
    let loom_dir = if let Some(pos) = args.iter().position(|a| a == "--loom-dir") {
        args.get(pos + 1)
            .cloned()
            .unwrap_or(loom_dir)
    } else {
        loom_dir
    };

    let server = loom_mcp_server::LoomMcpServer::new(&loom_dir);
    if let Err(e) = server.run() {
        eprintln!("loom-mcp-server error: {e}");
        std::process::exit(1);
    }
}

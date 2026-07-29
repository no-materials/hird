// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The `hird-mcp` binary: serves MCP over stdio (newline-delimited
//! JSON-RPC) until the client disconnects.

use std::io::{BufRead as _, Write as _};

use hird_mcp::Server;

/// Reads messages from stdin and writes responses to stdout, one per line.
fn main() {
    let stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut server = Server::new();
    for line in stdin.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_message(&line)
            && writeln!(stdout, "{response}")
                .and_then(|()| stdout.flush())
                .is_err()
        {
            break;
        }
    }
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The `hird-lsp` binary: serves the Hirð language server over stdio.

use hird_lsp::Backend;
use tower_lsp::{LspService, Server};

/// Runs the language server over stdin/stdout until the client disconnects.
#[tokio::main]
async fn main() {
    let (service, socket) = LspService::new(Backend::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}

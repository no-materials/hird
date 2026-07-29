// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The tower-lsp backend: document tracking and the LSP request handlers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, OneOf, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::analysis::Analysis;

/// One tracked document: the live buffer and its cached compilation.
#[derive(Debug)]
struct Document {
    /// The current buffer contents (kept in sync by full-document changes).
    source: String,
    /// The cached compilation of `source`; `None` after an edit invalidated
    /// it, rebuilt on the next query.
    analysis: Option<Arc<Analysis>>,
}

/// The Hirð language server.
#[derive(Debug)]
pub struct Backend {
    /// Handle for server-to-client messages (diagnostics).
    client: Client,
    /// All open documents, keyed by URI.
    documents: Mutex<HashMap<Url, Document>>,
}

impl Backend {
    /// Creates a backend publishing through `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    /// The document's analysis, compiling it first if the cache is stale.
    fn analysis(&self, uri: &Url) -> Option<Arc<Analysis>> {
        let mut documents = self.documents.lock().expect("document map poisoned");
        let document = documents.get_mut(uri)?;
        if document.analysis.is_none() {
            document.analysis = Some(Arc::new(Analysis::new(
                &module_name_of(uri),
                document.source.clone(),
            )));
        }
        document.analysis.clone()
    }

    /// Compiles the document and publishes its diagnostics.
    async fn publish(&self, uri: Url) {
        let Some(analysis) = self.analysis(&uri) else {
            return;
        };
        let diagnostics = analysis.diagnostics(&uri);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: String::from("hird-lsp"),
                version: Some(String::from(env!("CARGO_PKG_VERSION"))),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..TextDocumentSyncOptions::default()
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents
            .lock()
            .expect("document map poisoned")
            .insert(
                uri.clone(),
                Document {
                    source: params.text_document.text,
                    analysis: None,
                },
            );
        self.publish(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Full sync: the last change carries the whole new text.
        let Some(change) = params.content_changes.into_iter().next_back() else {
            return;
        };
        let mut documents = self.documents.lock().expect("document map poisoned");
        if let Some(document) = documents.get_mut(&params.text_document.uri) {
            document.source = change.text;
            document.analysis = None;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish(params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents
            .lock()
            .expect("document map poisoned")
            .remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position_params = params.text_document_position_params;
        Ok(self
            .analysis(&position_params.text_document.uri)
            .and_then(|analysis| analysis.hover(position_params.position)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let position_params = params.text_document_position_params;
        let uri = position_params.text_document.uri;
        Ok(self
            .analysis(&uri)
            .and_then(|analysis| analysis.definition(&uri, position_params.position)))
    }
}

/// The module name a document URI derives, from its file stem: each
/// `_`/`-`-separated segment capitalized and concatenated
/// (`repo_utils.hird` → `RepoUtils`), matching the CLI's derivation.
fn module_name_of(uri: &Url) -> String {
    let path = uri.path();
    let stem = path.rsplit('/').next().unwrap_or(path);
    let stem = stem.strip_suffix(".hird").unwrap_or(stem);
    stem.split(['_', '-'])
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

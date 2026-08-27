// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The tower-lsp backend: document tracking, per-directory program caching,
//! and the LSP request handlers.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
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

use crate::analysis::Program;

/// The server's mutable state: live buffers and compiled programs.
#[derive(Debug, Default)]
struct State {
    /// All open documents' current buffers (kept in sync by full-document
    /// changes), keyed by URI.
    documents: HashMap<Url, String>,
    /// The cached compilation of each directory with an open document,
    /// keyed by directory; an entry is dropped when any of the directory's
    /// documents opens, changes, or closes, and rebuilt on the next query.
    programs: HashMap<PathBuf, Arc<Program>>,
}

/// The Hirð language server.
#[derive(Debug)]
pub struct Backend {
    /// Handle for server-to-client messages (diagnostics).
    client: Client,
    /// Documents and programs.
    state: Mutex<State>,
}

impl Backend {
    /// Creates a backend publishing through `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Mutex::new(State::default()),
        }
    }

    /// The program of the open document `uri`'s directory, compiling it
    /// first if the cache is stale. `None` for a document that is not open.
    fn program(&self, uri: &Url) -> Option<Arc<Program>> {
        let mut state = self.state.lock().expect("server state poisoned");
        if !state.documents.contains_key(uri) {
            return None;
        }
        let dir = directory_of(uri);
        if let Some(program) = state.programs.get(&dir) {
            return Some(Arc::clone(program));
        }
        let program = Arc::new(Program::new(members_of(&dir, &state.documents)));
        state.programs.insert(dir, Arc::clone(&program));
        Some(program)
    }

    /// Drops the cached program of `uri`'s directory.
    fn invalidate(&self, uri: &Url) {
        self.state
            .lock()
            .expect("server state poisoned")
            .programs
            .remove(&directory_of(uri));
    }

    /// Compiles the document's directory and publishes the diagnostics of
    /// `uri` — and, with `siblings`, of every other open document in the
    /// same directory, whose diagnostics the same program decides.
    async fn publish(&self, uri: Url, siblings: bool) {
        let Some(program) = self.program(&uri) else {
            return;
        };
        let mut targets = vec![uri.clone()];
        if siblings {
            let dir = directory_of(&uri);
            let state = self.state.lock().expect("server state poisoned");
            targets.extend(
                state
                    .documents
                    .keys()
                    .filter(|other| **other != uri && directory_of(other) == dir)
                    .cloned(),
            );
        }
        for target in targets {
            let diagnostics = program.diagnostics(&target);
            self.client
                .publish_diagnostics(target, diagnostics, None)
                .await;
        }
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
        self.state
            .lock()
            .expect("server state poisoned")
            .documents
            .insert(uri.clone(), params.text_document.text);
        self.invalidate(&uri);
        self.publish(uri, false).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Full sync: the last change carries the whole new text.
        let Some(change) = params.content_changes.into_iter().next_back() else {
            return;
        };
        let uri = params.text_document.uri;
        let mut state = self.state.lock().expect("server state poisoned");
        if let Some(source) = state.documents.get_mut(&uri) {
            *source = change.text;
            state.programs.remove(&directory_of(&uri));
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish(params.text_document.uri, true).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.state
            .lock()
            .expect("server state poisoned")
            .documents
            .remove(&uri);
        self.invalidate(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position_params = params.text_document_position_params;
        let uri = position_params.text_document.uri;
        Ok(self
            .program(&uri)
            .and_then(|program| program.hover(&uri, position_params.position)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let position_params = params.text_document_position_params;
        let uri = position_params.text_document.uri;
        Ok(self
            .program(&uri)
            .and_then(|program| program.definition(&uri, position_params.position)))
    }
}

/// The directory whose `.hird` files form `uri`'s program: the parent of a
/// `file:` URI's path. A URI with no file path (an untitled buffer) gets a
/// directory of its own, so it compiles alone.
fn directory_of(uri: &Url) -> PathBuf {
    uri.to_file_path()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(uri.as_str()))
}

/// The members of `dir`'s program with their text, in URI order: every
/// `.hird` file in the directory (an open document's live buffer standing in
/// for its text on disk) plus every open document of the directory that is
/// not on disk.
fn members_of(dir: &Path, documents: &HashMap<Url, String>) -> Vec<(Url, String)> {
    let mut members: Vec<(Url, String)> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if !(path.is_file() && path.extension().is_some_and(|ext| ext == "hird")) {
                continue;
            }
            let Ok(uri) = Url::from_file_path(&path) else {
                continue;
            };
            let source = match documents.get(&uri) {
                Some(live) => live.clone(),
                None => match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(_) => continue,
                },
            };
            members.push((uri, source));
        }
    }
    for (uri, source) in documents {
        if directory_of(uri) == dir && !members.iter().any(|(u, _)| u == uri) {
            members.push((uri.clone(), source.clone()));
        }
    }
    members.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
    members
}

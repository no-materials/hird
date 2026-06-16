// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The checking pass over one source file: declaration registration,
//! dependency-ordered function checking, and result assembly.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{AstNode, Decl, ExternDecl, FnDecl, SourceFile, SyntaxNode, TypeDecl};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use hird_types::{Name, Subst, Type, unify};

use crate::diag::{CheckCode, CheckDiagnostic};
use crate::env::Env;
use crate::registry::{CtorInfo, Registry};
use crate::{CheckedFile, NodeKey, expr_span, name_token_span, node_span};

/// Marker: the current declaration's check stopped after an error. The
/// triggering diagnostic has already been recorded.
#[derive(Debug)]
pub(crate) struct Aborted;

/// Result of a checking step within one declaration.
pub(crate) type Checked<T> = Result<T, Aborted>;

/// Mutable state of one checking run.
pub(crate) struct Checker {
    /// Unification state.
    pub(crate) subst: Subst,
    /// Value environment.
    pub(crate) env: Env,
    /// Declared types and constructors.
    pub(crate) registry: Registry,
    /// Diagnostics accumulated across the whole file.
    pub(crate) diags: Vec<CheckDiagnostic>,
    /// Per-node types, recorded raw and resolved in [`Checker::finish`].
    pub(crate) types: Vec<(NodeKey, Type)>,
    /// Top-level bindings in registration order, resolved in
    /// [`Checker::finish`].
    bindings: Vec<(String, Type)>,
    /// Source file id used for spans.
    pub(crate) source_id: u32,
}

impl Checker {
    /// A fresh checker for the file identified by `source_id`.
    pub(crate) fn new(source_id: u32) -> Self {
        let registry = Registry::new();
        let mut env = Env::new();
        // The seeded Bool constructors are values too.
        env.insert_root("True", Type::bool());
        env.insert_root("False", Type::bool());
        Self {
            subst: Subst::new(),
            env,
            registry,
            diags: Vec::new(),
            types: Vec::new(),
            bindings: Vec::new(),
            source_id,
        }
    }

    /// Checks `file` and assembles the result.
    pub(crate) fn run(mut self, file: &SourceFile) -> CheckedFile {
        let mut type_decls = Vec::new();
        let mut fn_decls = Vec::new();
        let mut extern_decls = Vec::new();
        for decl in file.declarations() {
            match decl {
                Decl::Type(d) => type_decls.push(d),
                Decl::Fn(d) => fn_decls.push(d),
                Decl::Extern(d) => extern_decls.push(d),
                // Modules and imports are the module system's pass; effects,
                // tools, actors, and supervisors are later phases.
                _ => {}
            }
        }

        // Headers first so constructor fields can reference any declared
        // type, including mutually recursive ones.
        for decl in &type_decls {
            self.register_adt_header(decl);
        }
        for decl in &type_decls {
            // Per-declaration error isolation: a bad constructor field stops
            // this declaration only.
            let _ = self.register_constructors(decl);
        }
        for decl in &extern_decls {
            let _ = self.declare_extern(decl);
        }
        self.check_functions(&fn_decls);
        self.finish()
    }

    // ── type declarations ───────────────────────────────────────

    /// Registers a type declaration's name, arity, and constructor list.
    fn register_adt_header(&mut self, decl: &TypeDecl) {
        let Some(name) = decl.name() else { return };
        let params: Vec<&str> = decl.type_params().collect();
        for (i, param) in params.iter().enumerate() {
            if params[..i].contains(param) {
                let span = name_token_span(decl.syntax(), self.source_id);
                self.diags.push(CheckDiagnostic::error(
                    CheckCode::C0013,
                    span,
                    format!("type parameter `{param}` is declared twice"),
                ));
            }
        }
        let ctors = decl
            .constructors()
            .filter_map(|c| c.name().map(Name::new))
            .collect();
        // Arity counts distinct parameters so a (already reported) duplicate
        // stays consistent with the constructor result type built below.
        let arity = distinct_params(decl).len();
        self.registry.declare_adt(Name::new(name), arity, ctors);
    }

    /// Elaborates a type declaration's constructors into value schemes and
    /// registers them.
    fn register_constructors(&mut self, decl: &TypeDecl) -> Checked<()> {
        let Some(type_name) = decl.name() else {
            return Ok(());
        };
        let mut scope = BTreeMap::new();
        self.subst.enter_level();
        let mut param_args = Vec::new();
        for param in distinct_params(decl) {
            let ty = self.subst.fresh_type();
            scope.insert(param, ty.clone());
            param_args.push(ty);
        }
        let result_ty = Type::con(type_name, param_args);

        // Collect before generalising so `exit_level` runs on both paths.
        let mut collected = Vec::new();
        let mut failed = false;
        for ctor in decl.constructors() {
            let Some(ctor_name) = ctor.name() else {
                continue;
            };
            let mut fields = Vec::new();
            for field in ctor.fields() {
                match self.elaborate_closed(&field, &mut scope) {
                    Ok(ty) => fields.push(ty),
                    Err(Aborted) => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                break;
            }
            let ctor_ty = if fields.is_empty() {
                result_ty.clone()
            } else {
                Type::func(fields, result_ty.clone())
            };
            collected.push((String::from(ctor_name), ctor_ty, ctor.syntax().clone()));
        }
        self.subst.exit_level();
        for (ctor_name, ctor_ty, node) in collected {
            let scheme = self.subst.generalize(&ctor_ty);
            self.registry.declare_ctor(
                Name::new(ctor_name.as_str()),
                CtorInfo {
                    scheme: scheme.clone(),
                },
            );
            self.env.insert_root(&ctor_name, scheme.clone());
            self.types.push((NodeKey::of_node(&node), scheme.clone()));
            self.bindings.push((ctor_name, scheme));
        }
        if failed { Err(Aborted) } else { Ok(()) }
    }

    // ── externs ─────────────────────────────────────────────────

    /// Elaborates an extern declaration's signature and binds it.
    fn declare_extern(&mut self, decl: &ExternDecl) -> Checked<()> {
        let Some(name) = decl.name() else {
            return Ok(());
        };
        let params: Vec<_> = decl.params().collect();
        let annotated = params.iter().all(|p| p.ty().is_some());
        let Some(ret) = decl.return_type() else {
            return Err(self.incomplete_extern(name, decl.syntax()));
        };
        if !annotated {
            return Err(self.incomplete_extern(name, decl.syntax()));
        }
        let mut param_types = Vec::new();
        for param in &params {
            param_types.push(param.ty().expect("checked above"));
        }
        let scheme = self.signature_scheme(&param_types, &ret)?;
        self.env.insert_root(name, scheme.clone());
        self.types
            .push((NodeKey::of_node(decl.syntax()), scheme.clone()));
        self.bindings.push((String::from(name), scheme));
        Ok(())
    }

    /// Reports an extern declaration missing parts of its signature.
    fn incomplete_extern(&mut self, name: &str, node: &SyntaxNode) -> Aborted {
        let span = name_token_span(node, self.source_id);
        self.error(
            CheckCode::C0014,
            span,
            format!("extern `{name}` requires a fully annotated signature"),
        )
    }

    /// Elaborates a full signature (every parameter type plus the return
    /// type) into a generalised scheme. Surface type variables are implicitly
    /// quantified.
    fn signature_scheme(
        &mut self,
        params: &[hird_ast::TypeExpr],
        ret: &hird_ast::TypeExpr,
    ) -> Checked<Type> {
        let mut scope = BTreeMap::new();
        self.subst.enter_level();
        let mut tys = Vec::new();
        let mut result = Ok(());
        for param in params {
            match self.elaborate_fresh(param, &mut scope) {
                Ok(ty) => tys.push(ty),
                Err(a) => {
                    result = Err(a);
                    break;
                }
            }
        }
        let ret_ty = match result {
            Ok(()) => self.elaborate_fresh(ret, &mut scope),
            Err(a) => Err(a),
        };
        self.subst.exit_level();
        let ret_ty = ret_ty?;
        Ok(self.subst.generalize(&Type::func(tys, ret_ty)))
    }

    // ── functions ───────────────────────────────────────────────

    /// Checks every function declaration in dependency order.
    ///
    /// Fully annotated functions contribute their schemes up front (so they
    /// may be used polymorphically anywhere, including their own strongly
    /// connected component). The rest are checked one component at a time:
    /// each member gets a monomorphic placeholder, bodies are inferred, and
    /// the placeholders generalise once the whole component is done.
    fn check_functions(&mut self, fns: &[FnDecl]) {
        let annotated: Vec<bool> = fns.iter().map(is_fully_annotated).collect();
        let mut sig_ok: Vec<bool> = annotated.clone();

        for (i, decl) in fns.iter().enumerate() {
            if !annotated[i] {
                continue;
            }
            let Some(name) = decl.name() else {
                sig_ok[i] = false;
                continue;
            };
            let params: Vec<_> = decl.params().filter_map(|p| p.ty()).collect();
            let ret = decl.return_type().expect("fully annotated");
            match self.signature_scheme(&params, &ret) {
                Ok(scheme) => {
                    self.env.insert_root(name, scheme.clone());
                    self.types
                        .push((NodeKey::of_node(decl.syntax()), scheme.clone()));
                    self.bindings.push((String::from(name), scheme));
                }
                Err(Aborted) => sig_ok[i] = false,
            }
        }

        let graph = reference_graph(fns);
        for component in tarjan(&graph) {
            self.subst.enter_level();
            let mut placeholders: BTreeMap<usize, Type> = BTreeMap::new();
            for &i in &component {
                if annotated[i] {
                    continue;
                }
                if let Some(name) = fns[i].name() {
                    let placeholder = self.subst.fresh_type();
                    self.env.insert_root(name, placeholder.clone());
                    placeholders.insert(i, placeholder);
                }
            }
            for &i in &component {
                if annotated[i] {
                    // A failed signature already produced its diagnostics;
                    // re-elaborating the body against it would duplicate them.
                    if sig_ok[i] {
                        let _ = self.check_annotated_fn(&fns[i]);
                    }
                } else if let Some(placeholder) = placeholders.get(&i) {
                    let placeholder = placeholder.clone();
                    let _ = self.check_inferred_fn(&fns[i], &placeholder);
                }
            }
            self.subst.exit_level();
            for (&i, placeholder) in &placeholders {
                let Some(name) = fns[i].name() else { continue };
                let scheme = self.subst.generalize(placeholder);
                self.env.insert_root(name, scheme.clone());
                self.types
                    .push((NodeKey::of_node(fns[i].syntax()), scheme.clone()));
                self.bindings.push((String::from(name), scheme));
            }
        }
    }

    /// Checks a fully annotated function's body against its signature.
    ///
    /// Signature type variables become rigid skolem constants (lowercase
    /// constructor names, which user code cannot declare), so a body that
    /// needs them concrete fails with a mismatch naming the variable.
    fn check_annotated_fn(&mut self, decl: &FnDecl) -> Checked<()> {
        let Some(body) = decl.body() else {
            return Ok(());
        };
        let mut scope = BTreeMap::new();
        let params: Vec<_> = decl.params().collect();
        let mut param_tys = Vec::new();
        for param in &params {
            let ty_expr = param.ty().expect("fully annotated");
            param_tys.push(self.elaborate_skolem(&ty_expr, &mut scope)?);
        }
        let ret = decl.return_type().expect("fully annotated");
        let ret_ty = self.elaborate_skolem(&ret, &mut scope)?;

        self.env.push_scope();
        for (param, ty) in params.iter().zip(&param_tys) {
            self.bind_param(param, ty.clone());
        }
        let body_ty = self.infer_expr(&body);
        self.env.pop_scope();
        let body_ty = body_ty?;
        let span = expr_span(&body, self.source_id);
        self.unify_at(&ret_ty, &body_ty, span)
    }

    /// Infers a function body, threading partial annotations as hints, and
    /// unifies the result with the component placeholder.
    fn check_inferred_fn(&mut self, decl: &FnDecl, placeholder: &Type) -> Checked<()> {
        let Some(body) = decl.body() else {
            return Ok(());
        };
        let mut scope = BTreeMap::new();
        let params: Vec<_> = decl.params().collect();
        let mut param_tys = Vec::new();
        for param in &params {
            let ty = match param.ty() {
                Some(ty_expr) => self.elaborate_fresh(&ty_expr, &mut scope)?,
                None => self.subst.fresh_type(),
            };
            param_tys.push(ty);
        }
        self.env.push_scope();
        for (param, ty) in params.iter().zip(&param_tys) {
            self.bind_param(param, ty.clone());
        }
        let body_ty = self.infer_expr(&body);
        self.env.pop_scope();
        let mut body_ty = body_ty?;
        if let Some(ret) = decl.return_type() {
            let ret_ty = self.elaborate_fresh(&ret, &mut scope)?;
            let span = expr_span(&body, self.source_id);
            self.unify_at(&ret_ty, &body_ty, span)?;
            body_ty = ret_ty;
        }
        let fn_ty = Type::func(param_tys, body_ty);
        let span = node_span(decl.syntax(), self.source_id);
        self.unify_at(placeholder, &fn_ty, span)
    }

    /// Binds a parameter, warning on shadowing and recording its type.
    fn bind_param(&mut self, param: &hird_ast::Param, ty: Type) {
        let Some(name) = param.name() else { return };
        self.types
            .push((NodeKey::of_node(param.syntax()), ty.clone()));
        let span = name_token_span(param.syntax(), self.source_id);
        self.bind_value(name, ty, span);
    }

    // ── shared helpers ──────────────────────────────────────────

    /// Binds `name` in the innermost scope, warning when it shadows.
    pub(crate) fn bind_value(&mut self, name: &str, ty: Type, span: Span) {
        if self.env.insert(name, ty) {
            self.diags.push(CheckDiagnostic::warning(
                CheckCode::C0011,
                span,
                format!("binding `{name}` shadows an outer binding"),
            ));
        }
    }

    /// Records an error diagnostic and returns the abort marker.
    pub(crate) fn error(&mut self, code: CheckCode, span: Span, message: String) -> Aborted {
        self.diags.push(CheckDiagnostic::error(code, span, message));
        Aborted
    }

    /// Unifies two types, converting failure into a diagnostic plus abort.
    pub(crate) fn unify_at(&mut self, expected: &Type, got: &Type, span: Span) -> Checked<()> {
        unify(&mut self.subst, expected, got, span).map_err(|err| {
            self.diags.push(CheckDiagnostic::from_type_error(&err));
            Aborted
        })
    }

    /// Assembles the result: resolves recorded types, sorts diagnostics into
    /// source order, and snapshots the ADT table.
    fn finish(mut self) -> CheckedFile {
        let types = self
            .types
            .iter()
            .map(|(key, ty)| (*key, self.subst.resolve(ty)))
            .collect();
        let bindings = self
            .bindings
            .iter()
            .map(|(name, ty)| (name.clone(), self.subst.resolve(ty)))
            .collect();
        let adts = self
            .registry
            .adt_summaries()
            .map(|(name, ctors)| (name.clone(), ctors.clone()))
            .collect();
        self.diags
            .sort_by_key(|d| (d.span.start, d.span.end, d.severity, d.code));
        CheckedFile {
            types,
            bindings,
            adts,
            diagnostics: self.diags,
        }
    }
}

/// Whether every parameter and the return type carry annotations.
fn is_fully_annotated(decl: &FnDecl) -> bool {
    decl.return_type().is_some() && decl.params().all(|p| p.ty().is_some())
}

/// The declaration's type parameters with duplicates removed, first
/// occurrence winning.
fn distinct_params(decl: &TypeDecl) -> Vec<String> {
    let mut params: Vec<String> = Vec::new();
    for param in decl.type_params() {
        if !params.iter().any(|p| p == param) {
            params.push(String::from(param));
        }
    }
    params
}

/// Builds the top-level reference graph: an edge `i → j` whenever the body
/// of function `i` mentions the name of function `j`.
///
/// References are collected purely lexically (any identifier token), so a
/// shadowed use over-approximates into a spurious edge; that can only merge
/// components, which is sound but may reduce polymorphism in pathological
/// shadowing. Precise resolution is not worth the second walk.
fn reference_graph(fns: &[FnDecl]) -> Vec<Vec<usize>> {
    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, decl) in fns.iter().enumerate() {
        if let Some(name) = decl.name() {
            index.entry(name).or_insert(i);
        }
    }
    fns.iter()
        .map(|decl| {
            let mut idents = Vec::new();
            if let Some(body) = decl.body() {
                match body.syntax() {
                    Some(node) => collect_idents(node, &mut idents),
                    None => {
                        if let hird_ast::Expr::Name(name) = &body {
                            idents.push(String::from(name.text()));
                        }
                    }
                }
            }
            let mut edges: Vec<usize> = idents
                .iter()
                .filter_map(|name| index.get(name.as_str()).copied())
                .collect();
            edges.sort_unstable();
            edges.dedup();
            edges
        })
        .collect()
}

/// Pushes the text of every identifier token under `node` into `out`.
fn collect_idents(node: &SyntaxNode, out: &mut Vec<String>) {
    for element in node.children_with_tokens() {
        if let Some(token) = element.into_token() {
            if token.kind() == SyntaxKind::IDENT {
                out.push(String::from(token.text()));
            }
        } else if let Some(child) = element.into_node() {
            collect_idents(child, out);
        }
    }
}

/// Tarjan's strongly-connected-components algorithm.
///
/// Components are emitted callees-first (reverse topological order of the
/// condensation), which is exactly the order generalisation needs.
fn tarjan(graph: &[Vec<usize>]) -> Vec<Vec<usize>> {
    /// Mutable traversal state shared by the recursive walk.
    struct State<'g> {
        /// Adjacency lists.
        graph: &'g [Vec<usize>],
        /// Discovery index per node; `u32::MAX` marks unvisited.
        index: Vec<u32>,
        /// Smallest index reachable from each node.
        lowlink: Vec<u32>,
        /// Whether each node is currently on the component stack.
        on_stack: Vec<bool>,
        /// The component stack.
        stack: Vec<usize>,
        /// Next discovery index.
        next: u32,
        /// Completed components.
        out: Vec<Vec<usize>>,
    }

    /// Visits `v`, emitting its component once the root is closed.
    fn connect(state: &mut State<'_>, v: usize) {
        state.index[v] = state.next;
        state.lowlink[v] = state.next;
        state.next += 1;
        state.stack.push(v);
        state.on_stack[v] = true;
        for &w in &state.graph[v] {
            if state.index[w] == u32::MAX {
                connect(state, w);
                state.lowlink[v] = state.lowlink[v].min(state.lowlink[w]);
            } else if state.on_stack[w] {
                state.lowlink[v] = state.lowlink[v].min(state.index[w]);
            }
        }
        if state.lowlink[v] == state.index[v] {
            let mut component = Vec::new();
            loop {
                let w = state.stack.pop().expect("component stack underflow");
                state.on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            component.sort_unstable();
            state.out.push(component);
        }
    }

    let n = graph.len();
    let mut state = State {
        graph,
        index: alloc::vec![u32::MAX; n],
        lowlink: alloc::vec![0; n],
        on_stack: alloc::vec![false; n],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if state.index[v] == u32::MAX {
            connect(&mut state, v);
        }
    }
    state.out
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::tarjan;

    /// A self-loop is its own one-node component, not an error.
    #[test]
    fn self_loop_is_one_component() {
        let graph: Vec<Vec<usize>> = vec![vec![0]];
        let expected: Vec<Vec<usize>> = vec![vec![0]];
        assert_eq!(tarjan(&graph), expected);
    }

    /// Edgeless nodes are each their own component.
    #[test]
    fn isolated_nodes_are_singletons() {
        let graph: Vec<Vec<usize>> = vec![vec![], vec![], vec![]];
        let expected: Vec<Vec<usize>> = vec![vec![0], vec![1], vec![2]];
        assert_eq!(tarjan(&graph), expected);
    }

    /// A 2-cycle (`0 → 1 → 0`) collapses to a single component, held
    /// ascending.
    #[test]
    fn mutual_recursion_is_one_component() {
        let graph: Vec<Vec<usize>> = vec![vec![1], vec![0]];
        let expected: Vec<Vec<usize>> = vec![vec![0, 1]];
        assert_eq!(tarjan(&graph), expected);
    }

    /// `0 → 1` with `1` a leaf emits the callee first: generalisation must
    /// close over `1`'s scheme before `0` is checked.
    #[test]
    fn callee_is_emitted_before_caller() {
        let graph: Vec<Vec<usize>> = vec![vec![1], vec![]];
        let expected: Vec<Vec<usize>> = vec![vec![1], vec![0]];
        assert_eq!(tarjan(&graph), expected);
    }

    /// A diamond (`0 → {1, 2} → 3`) emits the shared sink `3` first and the
    /// root `0` last — reverse-topological over the four singletons.
    #[test]
    fn diamond_orders_sink_first_root_last() {
        let graph: Vec<Vec<usize>> = vec![vec![1, 2], vec![3], vec![3], vec![]];
        let result = tarjan(&graph);
        assert_eq!(result.first(), Some(&vec![3]), "sink first");
        assert_eq!(result.last(), Some(&vec![0]), "root last");
        assert_eq!(result.len(), 4, "four singleton components");
    }

    /// A cycle with an external dependant (`0 ↔ 1`, `2 → 0`) emits the
    /// cycle as one component before its caller `2`.
    #[test]
    fn cycle_precedes_its_dependant() {
        let graph: Vec<Vec<usize>> = vec![vec![1], vec![0], vec![0]];
        let expected: Vec<Vec<usize>> = vec![vec![0, 1], vec![2]];
        assert_eq!(tarjan(&graph), expected);
    }
}

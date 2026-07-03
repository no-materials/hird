// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The checking pass over one source file: declaration registration,
//! dependency-ordered function checking, and result assembly.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem;

use hird_ast::{
    AstNode, Decl, EffectAnn, EffectDecl, ExternDecl, FnDecl, Param, SourceFile, SyntaxNode,
    ToolDecl, TypeDecl,
};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use hird_types::{Effect, EffectRow, Label, Name, Subst, Type, TypeError, unify, unify_row};

use crate::diag::{CheckCode, CheckDiagnostic};
use crate::elaborate::Scope;
use crate::env::Env;
use crate::program::{ExportedType, ModuleInterface};
use crate::registry::{CtorInfo, Registry};
use crate::{CheckedFile, ModuleName, NodeKey, expr_span, name_token_span, node_span};

/// Marker: the current declaration's check stopped after an error. The
/// triggering diagnostic has already been recorded.
#[derive(Debug)]
pub(crate) struct Aborted;

/// Result of a checking step within one declaration.
pub(crate) type Checked<T> = Result<T, Aborted>;

/// One effect introduced while inferring a function body, paired with the span
/// of the application that introduced it.
///
/// Recorded during the body walk and consulted when a body's inferred effect
/// row fails to match the declared one, so the diagnostic can point at the call
/// that brought in the offending effect rather than the whole signature. The
/// effect's type arguments carry the capability the effect is linked to, so the
/// pair records the capability-to-call provenance.
pub(crate) struct EffectIntro {
    /// The effect introduced (resolved when matched against an offending one).
    effect: Effect,
    /// Span of the introducing application.
    span: Span,
}

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
    /// Each function node's elaborated effect row and each `handle` block's
    /// computed row, recorded raw and resolved in
    /// [`Checker::finish_with_interface`]. Shares the body check's variable
    /// identities, so a row variable in a parameter type and the function's row
    /// resolve to one variable.
    pub(crate) effect_rows: Vec<(NodeKey, EffectRow)>,
    /// Each `handle` arm's handled effect, keyed by the arm node, recorded raw
    /// and resolved in [`Checker::finish_with_interface`] for the IR.
    pub(crate) handled_effects: Vec<(NodeKey, Effect)>,
    /// Each tool declaration's derived invocation record, keyed by generated
    /// name, recorded raw and resolved in [`Checker::finish_with_interface`].
    invocation_records: Vec<(Name, Type)>,
    /// The effect row accumulated while inferring the current function or lambda
    /// body — the union of every effect its applications perform. Reset at each
    /// function body and saved/restored across lambda boundaries (a lambda's
    /// effects belong to its function type, not the enclosing row).
    pub(crate) current_row: EffectRow,
    /// Provenance for [`Checker::current_row`]: which call introduced each
    /// effect. Cleared alongside the accumulator and consulted to place the
    /// declared-vs-inferred mismatch diagnostic at the offending call.
    pub(crate) current_prov: Vec<EffectIntro>,
    /// Top-level bindings in registration order, resolved in
    /// [`Checker::finish_with_interface`].
    bindings: Vec<(String, Type)>,
    /// Source file id used for spans.
    pub(crate) source_id: u32,
    /// The module being checked; set by the whole-program driver and `None`
    /// for single-file checking. Locally declared constructors record it, and
    /// the opaque gate compares a foreign constructor's module against it.
    pub(crate) current_module: Option<ModuleName>,
    /// First-seen name-token span of each value-namespace definition, for
    /// duplicate detection (functions, externs, constructors, imported
    /// values).
    value_spans: BTreeMap<String, Span>,
    /// First-seen name-token span of each type-namespace definition.
    type_spans: BTreeMap<String, Span>,
    /// Imported module qualifiers mapped to their exported value schemes, for
    /// `Mod.member` qualified access.
    pub(crate) modules: BTreeMap<String, BTreeMap<String, Type>>,
    /// Names of this module's exported (`pub`) functions.
    exported_fns: Vec<String>,
    /// This module's exported (`pub`) types paired with their opacity.
    exported_types: Vec<(Name, bool)>,
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
            effect_rows: Vec::new(),
            handled_effects: Vec::new(),
            invocation_records: Vec::new(),
            current_row: EffectRow::empty(),
            current_prov: Vec::new(),
            bindings: Vec::new(),
            source_id,
            current_module: None,
            value_spans: BTreeMap::new(),
            type_spans: BTreeMap::new(),
            modules: BTreeMap::new(),
            exported_fns: Vec::new(),
            exported_types: Vec::new(),
        }
    }

    /// Checks `file` and assembles the result, discarding the export interface.
    pub(crate) fn run(self, file: &SourceFile) -> CheckedFile {
        self.run_with_interface(file).0
    }

    /// Checks `file`, returning its result and the export interface the
    /// whole-program driver seeds into dependent modules.
    pub(crate) fn run_with_interface(
        mut self,
        file: &SourceFile,
    ) -> (CheckedFile, ModuleInterface) {
        // Duplicate detection runs first, over source order, against the tables
        // any seeded imports have already populated (catching import-vs-local).
        self.detect_duplicates(file);

        let mut type_decls = Vec::new();
        let mut fn_decls = Vec::new();
        let mut extern_decls = Vec::new();
        let mut effect_decls = Vec::new();
        let mut tool_decls = Vec::new();
        for decl in file.declarations() {
            match decl {
                Decl::Type(d) => type_decls.push(d),
                Decl::Fn(d) => fn_decls.push(d),
                Decl::Extern(d) => extern_decls.push(d),
                Decl::Effect(d) => effect_decls.push(d),
                Decl::Tool(d) => tool_decls.push(d),
                // Modules and imports are the module system's pass; actors
                // and supervisors are later phases.
                _ => {}
            }
        }

        // Effects are registered before anything elaborates a signature, so an
        // effect annotation can reference any declared effect regardless of
        // declaration order.
        for decl in &effect_decls {
            self.register_effect(decl);
        }

        for decl in &fn_decls {
            if decl.is_pub()
                && let Some(name) = decl.name()
            {
                self.exported_fns.push(String::from(name));
            }
        }

        // Headers first so constructor fields can reference any declared
        // type, including mutually recursive ones.
        for decl in &type_decls {
            self.register_adt_header(decl);
        }
        // Tool markers are nullary types, registered with the headers so any
        // signature can name them (`Tool<ReadRepo>` in a row, a constructor
        // field, another tool's input).
        for decl in &tool_decls {
            self.register_tool_marker(decl);
        }
        for decl in &type_decls {
            // Per-declaration error isolation: a bad constructor field stops
            // this declaration only.
            let _ = self.register_constructors(decl);
        }
        for decl in &extern_decls {
            let _ = self.declare_extern(decl);
        }
        for decl in &tool_decls {
            let _ = self.declare_tool(decl);
        }
        self.check_functions(&fn_decls);
        self.finish_with_interface()
    }

    // ── module-system seeding (whole-program driver) ─────────────

    /// Records the module under check.
    pub(crate) fn set_module(&mut self, name: ModuleName) {
        self.current_module = Some(name);
    }

    /// Injects a driver-discovered diagnostic (module-name mismatch, unresolved
    /// import, cycle) so it sorts in with the per-module diagnostics.
    pub(crate) fn push_diag(&mut self, diag: CheckDiagnostic) {
        self.diags.push(diag);
    }

    /// Binds a qualifier to a module's exported values for `Mod.member` access.
    pub(crate) fn seed_module_qualifier(
        &mut self,
        qualifier: &str,
        values: BTreeMap<String, Type>,
    ) {
        self.modules.insert(String::from(qualifier), values);
    }

    /// Brings an imported function into scope unqualified.
    pub(crate) fn seed_import_function(&mut self, name: &str, scheme: Type, span: Span) {
        self.note_value_name(name, span);
        self.env.insert_root(name, scheme);
    }

    /// Brings an imported transparent constructor into scope unqualified.
    pub(crate) fn seed_import_ctor(
        &mut self,
        name: &str,
        owner: Name,
        scheme: Type,
        from: ModuleName,
        span: Span,
    ) {
        self.note_value_name(name, span);
        self.registry.declare_ctor(
            Name::new(name),
            CtorInfo {
                scheme: scheme.clone(),
                owner,
                module: Some(from),
                opaque: false,
            },
        );
        self.env.insert_root(name, scheme);
    }

    /// Brings an imported type's name (and arity) into scope. A transparent
    /// type's constructors are imported separately, by name; an opaque type's
    /// constructors are registered solely so an out-of-module destructure or
    /// construction names the type instead of reporting an unknown constructor.
    pub(crate) fn seed_import_type(
        &mut self,
        name: &Name,
        exported: &ExportedType,
        from: ModuleName,
        span: Span,
    ) {
        self.note_type_name(name.as_str(), span);
        let ctor_names = exported.ctors.iter().map(|(n, _)| n.clone()).collect();
        self.registry
            .declare_adt(name.clone(), exported.arity, ctor_names);
        if exported.opaque {
            for (ctor, scheme) in &exported.ctors {
                self.registry.declare_ctor(
                    ctor.clone(),
                    CtorInfo {
                        scheme: scheme.clone(),
                        owner: name.clone(),
                        module: Some(from.clone()),
                        opaque: true,
                    },
                );
            }
        }
    }

    /// Records a value-namespace definition at `span`, reporting a duplicate
    /// (C0017) against the first occurrence.
    fn note_value_name(&mut self, name: &str, span: Span) {
        note_duplicate(
            &mut self.diags,
            &mut self.value_spans,
            CheckCode::C0017,
            name,
            span,
            || format!("duplicate definition of `{name}`"),
        );
    }

    /// Records a type-namespace definition at `span`, reporting a duplicate
    /// (C0018) against the first occurrence.
    fn note_type_name(&mut self, name: &str, span: Span) {
        note_duplicate(
            &mut self.diags,
            &mut self.type_spans,
            CheckCode::C0018,
            name,
            span,
            || format!("duplicate type `{name}`"),
        );
    }

    /// Walks declarations in source order, recording each name in its namespace
    /// and reporting collisions. Types and values are separate namespaces, so a
    /// type and a value may share a name (`type Email = Email(String)`).
    fn detect_duplicates(&mut self, file: &SourceFile) {
        for decl in file.declarations() {
            match decl {
                Decl::Fn(d) => {
                    if let Some(name) = d.name() {
                        let span = name_token_span(d.syntax(), self.source_id);
                        self.note_value_name(name, span);
                    }
                }
                Decl::Extern(d) => {
                    if let Some(name) = d.name() {
                        let span = name_token_span(d.syntax(), self.source_id);
                        self.note_value_name(name, span);
                    }
                }
                Decl::Type(d) => {
                    if let Some(name) = d.name() {
                        let span = name_token_span(d.syntax(), self.source_id);
                        self.note_type_name(name, span);
                    }
                    for ctor in d.constructors() {
                        if let Some(name) = ctor.name() {
                            let span = name_token_span(ctor.syntax(), self.source_id);
                            self.note_value_name(name, span);
                        }
                    }
                }
                Decl::Tool(d) => {
                    // A tool occupies both namespaces: its marker type and its
                    // generated function.
                    if let Some(name) = d.name() {
                        let span = name_token_span(d.syntax(), self.source_id);
                        self.note_type_name(name, span);
                        self.note_value_name(&tool_fn_name(name), span);
                    }
                }
                _ => {}
            }
        }
    }

    // ── effect declarations ─────────────────────────────────────

    /// Registers an effect declaration's name and type-parameter count, so
    /// effect annotations can resolve and arity-check it.
    fn register_effect(&mut self, decl: &EffectDecl) {
        let Some(name) = decl.name() else { return };
        let arity = decl.type_params().count();
        self.registry.declare_effect(Name::new(name), arity);
    }

    // ── type declarations ───────────────────────────────────────

    /// Registers a type declaration's name, arity, and constructor list.
    fn register_adt_header(&mut self, decl: &TypeDecl) {
        let Some(name) = decl.name() else { return };
        if decl.is_pub() {
            self.exported_types
                .push((Name::new(name), decl.is_opaque()));
        }
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
        let owner = Name::new(type_name);
        let opaque = decl.is_opaque();
        let mut scope = Scope::new();
        self.subst.enter_level();
        let mut param_args = Vec::new();
        for param in distinct_params(decl) {
            let ty = self.subst.fresh_type();
            scope.insert_type(param, ty.clone());
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
                    owner: owner.clone(),
                    module: self.current_module.clone(),
                    opaque,
                },
            );
            self.env.insert_root(&ctor_name, scheme.clone());
            self.types.push((NodeKey::of_node(&node), scheme.clone()));
            self.bindings.push((ctor_name, scheme));
        }
        if failed { Err(Aborted) } else { Ok(()) }
    }

    // ── tool declarations ───────────────────────────────────────

    /// Registers a tool declaration's marker type: a nullary nominal type with
    /// no constructors, so `Tool<Name>` resolves through ordinary
    /// effect-argument elaboration. One shared `Tool` effect parameterised by
    /// the marker — not an effect per tool.
    fn register_tool_marker(&mut self, decl: &ToolDecl) {
        let Some(name) = decl.name() else { return };
        self.registry.declare_adt(Name::new(name), 0, Vec::new());
    }

    /// Desugars a tool declaration into its function binding and derived
    /// invocation record.
    ///
    /// The function's type is `(input) → output ! ({Tool<Name>} ∪
    /// declared_row)`, elaborated in a closed scope over the tool's type
    /// parameters and generalised like an ADT constructor. The record
    /// `{ tool, args, result, timestamp, caller }` projects `args`/`result`
    /// from the signature (`tool`, `timestamp`, and `caller` are fixed,
    /// runtime-injected fields) and files under the generated name
    /// `NameInvocation`.
    fn declare_tool(&mut self, decl: &ToolDecl) -> Checked<()> {
        let Some(name) = decl.name() else {
            return Ok(());
        };
        let (Some(input), Some(output)) = (decl.input(), decl.output()) else {
            return Ok(());
        };
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

        let mut scope = Scope::new();
        self.subst.enter_level();
        let mut seen: Vec<&str> = Vec::new();
        for param in &params {
            // First occurrence wins, consistent with ADT headers.
            if seen.contains(param) {
                continue;
            }
            seen.push(param);
            scope.insert_type(String::from(*param), self.subst.fresh_type());
        }
        // Collect without `?` so `exit_level` runs on the error paths too.
        let input_ty = self.elaborate_closed(&input, &mut scope);
        let output_ty = match &input_ty {
            Ok(_) => self.elaborate_closed(&output, &mut scope),
            Err(_) => Err(Aborted),
        };
        let row = match (&output_ty, decl.effect_ann()) {
            (Ok(_), Some(ann)) => self.elaborate_row_closed(&ann, &mut scope),
            (Ok(_), None) => Ok(EffectRow::empty()),
            (Err(_), _) => Err(Aborted),
        };
        self.subst.exit_level();
        let input_ty = input_ty?;
        let output_ty = output_ty?;
        let mut row = row?;

        // Tool args and results cross the audit-log wire boundary, so both
        // sides must be wire-representable.
        for (side, ty) in [("args", &input_ty), ("result", &output_ty)] {
            if let Some(violation) = wire_violation(&self.registry, ty, &mut BTreeSet::new()) {
                let span = name_token_span(decl.syntax(), self.source_id);
                let detail = match violation {
                    WireViolation::Function(ty) => {
                        format!("function type `{ty}` cannot be serialised")
                    }
                    WireViolation::Capability(name) => {
                        format!("`{name}` is an opaque capability")
                    }
                };
                return Err(self.error(
                    CheckCode::C0032,
                    span,
                    format!("tool `{name}` {side} are not wire-representable: {detail}"),
                ));
            }
        }

        row.insert(Effect::parametric(
            "Tool",
            Vec::from([Type::con(name, Vec::new())]),
        ));
        let fn_ty = Type::func_eff(Vec::from([input_ty.clone()]), output_ty.clone(), row);
        let scheme = self.subst.generalize(&fn_ty);
        let fn_name = tool_fn_name(name);
        self.env.insert_root(&fn_name, scheme.clone());
        self.types
            .push((NodeKey::of_node(decl.syntax()), scheme.clone()));
        self.bindings.push((fn_name, scheme));

        let record = Type::record([
            (Label::new("tool"), Type::string()),
            (Label::new("args"), input_ty),
            (Label::new("result"), output_ty),
            (Label::new("timestamp"), Type::con("Timestamp", Vec::new())),
            (Label::new("caller"), Type::con("CallerId", Vec::new())),
        ]);
        self.invocation_records
            .push((Name::new(format!("{name}Invocation")), record));
        Ok(())
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
        // Externs have no effect annotation in the grammar; their row is empty.
        let scheme = self.signature_scheme(&params, &ret, None)?;
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

    /// Elaborates a full signature — every parameter type, the return type, and
    /// the optional effect annotation — into a generalised scheme. Surface type
    /// and row variables are implicitly quantified; they share one scope, so a
    /// row variable named in both a parameter type and the effect row is the
    /// same variable. Each parameter's name is recorded as a capability so the
    /// effect row may reference it (`EtsRead<t>`); callers pass fully annotated
    /// parameters, so every `ty()` is present.
    fn signature_scheme(
        &mut self,
        params: &[Param],
        ret: &hird_ast::TypeExpr,
        effect_ann: Option<&EffectAnn>,
    ) -> Checked<Type> {
        let mut scope = Scope::new();
        self.subst.enter_level();
        let mut tys = Vec::new();
        let mut result = Ok(());
        for param in params {
            let Some(ty_expr) = param.ty() else {
                result = Err(Aborted);
                break;
            };
            match self.elaborate_fresh(&ty_expr, &mut scope) {
                Ok(ty) => {
                    if let Some(name) = param.name() {
                        scope.insert_cap(name, ty.clone());
                    }
                    tys.push(ty);
                }
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
        // The row is elaborated in the same scope and level as the types, so its
        // row variables generalise alongside them.
        let row = match (&ret_ty, effect_ann) {
            (Ok(_), Some(ann)) => self.elaborate_row_fresh(ann, &mut scope),
            _ => Ok(EffectRow::empty()),
        };
        self.subst.exit_level();
        let ret_ty = ret_ty?;
        let row = row?;
        Ok(self.subst.generalize(&Type::func_eff(tys, ret_ty, row)))
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
            let params: Vec<_> = decl.params().collect();
            let ret = decl.return_type().expect("fully annotated");
            match self.signature_scheme(&params, &ret, decl.effect_ann().as_ref()) {
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
        let mut scope = Scope::new();
        let params: Vec<_> = decl.params().collect();
        let mut param_tys = Vec::new();
        for param in &params {
            let ty_expr = param.ty().expect("fully annotated");
            let ty = self.elaborate_skolem(&ty_expr, &mut scope)?;
            if let Some(name) = param.name() {
                scope.insert_cap(name, ty.clone());
            }
            param_tys.push(ty);
        }
        let ret = decl.return_type().expect("fully annotated");
        let ret_ty = self.elaborate_skolem(&ret, &mut scope)?;
        // The declared row shares the parameters' scope, so a row variable named
        // in both, or a capability the row links to, keeps one identity. The
        // annotation is already known valid (its scheme elaborated cleanly), so
        // this cannot add a diagnostic.
        let declared = self.declared_row(decl, &mut scope);

        self.env.push_scope();
        for (param, ty) in params.iter().zip(&param_tys) {
            self.bind_param(param, ty.clone());
        }
        self.begin_effect_scope();
        let body_ty = self.infer_expr(&body);
        let inferred = self.take_effect_row();
        self.env.pop_scope();
        let body_ty = body_ty?;
        let span = expr_span(&body, self.source_id);
        self.unify_at(&ret_ty, &body_ty, span)?;
        // The body's inferred effects must equal the declared row.
        if let Ok(declared) = declared {
            self.check_effect_row(&declared, &inferred, span);
        }
        Ok(())
    }

    /// Elaborates a function's declared effect row — the `! {…}` annotation, or
    /// the empty row when `!` is absent — recording an annotated row for the IR
    /// in the same `scope` as the parameters, so a shared row variable or a
    /// linked capability keeps one identity, and returning it for the
    /// declared-vs-inferred check. An elaboration error is already reported, so
    /// the row is omitted from the IR and the equality check is skipped.
    fn declared_row(&mut self, decl: &FnDecl, scope: &mut Scope) -> Checked<EffectRow> {
        let Some(ann) = decl.effect_ann() else {
            return Ok(EffectRow::empty());
        };
        let row = self.elaborate_row_fresh(&ann, scope)?;
        self.effect_rows
            .push((NodeKey::of_node(decl.syntax()), row.clone()));
        Ok(row)
    }

    /// Infers a function body, threading partial annotations as hints, and
    /// unifies the result with the component placeholder. The inferred effect
    /// row is carried on the scheme and checked against the declared row.
    fn check_inferred_fn(&mut self, decl: &FnDecl, placeholder: &Type) -> Checked<()> {
        let Some(body) = decl.body() else {
            return Ok(());
        };
        let mut scope = Scope::new();
        let params: Vec<_> = decl.params().collect();
        let mut param_tys = Vec::new();
        for param in &params {
            let ty = match param.ty() {
                Some(ty_expr) => self.elaborate_fresh(&ty_expr, &mut scope)?,
                None => self.subst.fresh_type(),
            };
            if let Some(name) = param.name() {
                scope.insert_cap(name, ty.clone());
            }
            param_tys.push(ty);
        }
        let declared = self.declared_row(decl, &mut scope);
        self.env.push_scope();
        for (param, ty) in params.iter().zip(&param_tys) {
            self.bind_param(param, ty.clone());
        }
        self.begin_effect_scope();
        let body_ty = self.infer_expr(&body);
        let inferred = self.take_effect_row();
        self.env.pop_scope();
        let mut body_ty = body_ty?;
        if let Some(ret) = decl.return_type() {
            let ret_ty = self.elaborate_fresh(&ret, &mut scope)?;
            let span = expr_span(&body, self.source_id);
            self.unify_at(&ret_ty, &body_ty, span)?;
            body_ty = ret_ty;
        }
        let span = expr_span(&body, self.source_id);
        // Empty when `!` is absent, so an effectful top-level function that
        // omits its effects fails the equality check.
        if let Ok(declared) = declared {
            self.check_effect_row(&declared, &inferred, span);
        }
        // The scheme carries the inferred row, generalised alongside the type.
        let fn_ty = Type::func_eff(param_tys, body_ty, inferred);
        let span = node_span(decl.syntax(), self.source_id);
        self.unify_at(placeholder, &fn_ty, span)
    }

    /// Binds a parameter, warning on shadowing and recording its type.
    fn bind_param(&mut self, param: &Param, ty: Type) {
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

    // ── effect inference ────────────────────────────────────────

    /// Starts a fresh effect accumulator for a function or lambda body.
    fn begin_effect_scope(&mut self) {
        self.current_row = EffectRow::empty();
        self.current_prov.clear();
    }

    /// Takes the accumulated body row, leaving an empty accumulator. Provenance
    /// is left in place for the mismatch check that immediately follows.
    fn take_effect_row(&mut self) -> EffectRow {
        mem::take(&mut self.current_row)
    }

    /// Unions `row`'s effects into the current accumulator, recording each as
    /// introduced by the call at `span`. Row tails merge so a row-polymorphic
    /// callee's tail flows into the enclosing row; both inputs are resolved
    /// first, so the merged tails are unbound representatives, never solved rows.
    pub(crate) fn add_effects(&mut self, row: &EffectRow, span: Span) {
        let added = self.subst.resolve_row(row);
        if added.is_empty() {
            return;
        }
        let mut acc = self.subst.resolve_row(&self.current_row);
        for effect in added.effects() {
            self.current_prov.push(EffectIntro {
                effect: effect.clone(),
                span,
            });
            acc.insert(effect.clone());
        }
        let tail = match (acc.tail(), added.tail()) {
            (None, other) => other,
            (Some(a), None) => Some(a),
            (Some(a), Some(b)) => {
                if a != b {
                    // Two row-polymorphic callees: collapse their tails. Bare
                    // row variables always unify, so this cannot fail.
                    let _ = unify_row(
                        &mut self.subst,
                        &EffectRow::of_var(a),
                        &EffectRow::of_var(b),
                        span,
                    );
                }
                Some(a)
            }
        };
        self.current_row = acc.with_tail(tail);
    }

    /// Checks the body's inferred effect row against the declared row by row
    /// unification (equality). A surplus or missing effect is reported against
    /// the call that introduced the offending effect, falling back to `span`; an
    /// argument-type clash between same-headed effects keeps the generic
    /// rendering.
    fn check_effect_row(&mut self, declared: &EffectRow, inferred: &EffectRow, span: Span) {
        let Err(err) = unify_row(&mut self.subst, declared, inferred, span) else {
            return;
        };
        match err {
            TypeError::EffectMismatch { offending, .. } => {
                let at = offending
                    .as_ref()
                    .and_then(|effect| self.intro_span(effect))
                    .unwrap_or(span);
                let declared = self.subst.resolve_row(declared);
                let inferred = self.subst.resolve_row(inferred);
                self.diags.push(CheckDiagnostic::error(
                    CheckCode::C0030,
                    at,
                    format!("declared {declared} but body performs {inferred}"),
                ));
            }
            other => self.diags.push(CheckDiagnostic::from_type_error(&other)),
        }
    }

    /// The span of the application that introduced an effect equal (by resolved
    /// form) to `effect`, from the current body's provenance.
    fn intro_span(&self, effect: &Effect) -> Option<Span> {
        let target = self.resolve_effect(effect);
        self.current_prov
            .iter()
            .find(|intro| self.resolve_effect(&intro.effect) == target)
            .map(|intro| intro.span)
    }

    /// Resolves an effect's type arguments, so two effects compare equal when
    /// their arguments resolve equal despite differing unsolved variables.
    fn resolve_effect(&self, effect: &Effect) -> Effect {
        effect.map_args(|arg| self.subst.resolve(arg))
    }

    /// Assembles the result and the export interface: resolves recorded types,
    /// sorts diagnostics into source order, snapshots the ADT table, and
    /// gathers the `pub` surface from the accumulated export markers.
    fn finish_with_interface(mut self) -> (CheckedFile, ModuleInterface) {
        let types = self
            .types
            .iter()
            .map(|(key, ty)| (*key, self.subst.resolve(ty)))
            .collect();
        let effect_rows = self
            .effect_rows
            .iter()
            .map(|(key, row)| (*key, self.subst.resolve_row(row)))
            .collect();
        let handled_effects = self
            .handled_effects
            .iter()
            .map(|(key, effect)| (*key, effect.map_args(|arg| self.subst.resolve(arg))))
            .collect();
        let invocation_records = self
            .invocation_records
            .iter()
            .map(|(name, ty)| (name.clone(), self.subst.resolve(ty)))
            .collect();
        let bindings: BTreeMap<String, Type> = self
            .bindings
            .iter()
            .map(|(name, ty)| (name.clone(), self.subst.resolve(ty)))
            .collect();
        let adts = self
            .registry
            .adt_summaries()
            .map(|(name, ctors)| (name.clone(), ctors.clone()))
            .collect();

        let functions = self
            .exported_fns
            .iter()
            .filter_map(|name| bindings.get(name).map(|ty| (name.clone(), ty.clone())))
            .collect();
        let mut exported = BTreeMap::new();
        for (type_name, opaque) in &self.exported_types {
            let arity = self.registry.type_arity(type_name.as_str()).unwrap_or(0);
            let ctors = self
                .registry
                .adt_constructors(type_name.as_str())
                .map(|cs| {
                    cs.iter()
                        .filter_map(|c| {
                            self.registry
                                .ctor(c.as_str())
                                .map(|info| (c.clone(), info.scheme.clone()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            exported.insert(
                type_name.clone(),
                ExportedType {
                    arity,
                    opaque: *opaque,
                    ctors,
                },
            );
        }
        let interface = ModuleInterface {
            functions,
            types: exported,
        };

        self.diags
            .sort_by_key(|d| (d.span.start, d.span.end, d.severity, d.code));
        let checked = CheckedFile {
            types,
            bindings,
            adts,
            effect_rows,
            handled_effects,
            invocation_records,
            diagnostics: self.diags,
        };
        (checked, interface)
    }
}

/// Records `name` in `spans`, or — when already present — pushes a duplicate
/// diagnostic (`code`, with `message` rendered lazily) related to the first
/// occurrence.
fn note_duplicate(
    diags: &mut Vec<CheckDiagnostic>,
    spans: &mut BTreeMap<String, Span>,
    code: CheckCode,
    name: &str,
    span: Span,
    message: impl FnOnce() -> String,
) {
    if let Some(first) = spans.get(name).copied() {
        diags.push(
            CheckDiagnostic::error(code, span, message())
                .with_related(first, String::from("first defined here")),
        );
    } else {
        spans.insert(String::from(name), span);
    }
}

/// Whether every parameter and the return type carry annotations.
fn is_fully_annotated(decl: &FnDecl) -> bool {
    decl.return_type().is_some() && decl.params().all(|p| p.ty().is_some())
}

/// The generated function name of a tool: the `PascalCase` tool name in
/// `snake_case`, with acronym runs kept whole (`ReadRepo` → `read_repo`,
/// `LLMCall` → `llm_call`).
fn tool_fn_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = String::with_capacity(bytes.len() + 4);
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_uppercase() {
            let after_lower = i > 0 && !bytes[i - 1].is_ascii_uppercase();
            let acronym_end = i > 0
                && bytes[i - 1].is_ascii_uppercase()
                && bytes.get(i + 1).is_some_and(u8::is_ascii_lowercase);
            if after_lower || acronym_end {
                out.push('_');
            }
        }
        out.push(b.to_ascii_lowercase() as char);
    }
    out
}

/// A type that cannot cross the tool wire boundary.
enum WireViolation {
    /// A function type: not serialisable.
    Function(Type),
    /// An opaque capability type: minting one from a log would forge it.
    Capability(Name),
}

/// The first wire-representability violation in `ty`, if any.
///
/// Walks the type structurally and through the constructor fields of every
/// declared ADT it applies, so a nested declaration cannot smuggle a
/// function or capability past the check. `visited` breaks recursive types
/// (by ADT name, an approximation that is sound because arguments are walked
/// at every application site). Type variables pass: a generic tool's
/// instantiations are validated at the wire layer, value by value.
fn wire_violation(
    registry: &Registry,
    ty: &Type,
    visited: &mut BTreeSet<Name>,
) -> Option<WireViolation> {
    match ty {
        Type::TyVar(_) => None,
        Type::TyFn(..) => Some(WireViolation::Function(ty.clone())),
        Type::TyTuple(elems) => elems
            .iter()
            .find_map(|e| wire_violation(registry, e, visited)),
        Type::TyRecord(fields) => fields
            .values()
            .find_map(|f| wire_violation(registry, f, visited)),
        Type::TyForall(_, _, body) => wire_violation(registry, body, visited),
        Type::TyCon(name, args) => {
            if registry.adt_is_opaque(name.as_str()) {
                return Some(WireViolation::Capability(name.clone()));
            }
            if let Some(v) = args
                .iter()
                .find_map(|a| wire_violation(registry, a, visited))
            {
                return Some(v);
            }
            if !visited.insert(name.clone()) {
                return None;
            }
            let ctors = registry.adt_constructors(name.as_str()).unwrap_or(&[]);
            for ctor in ctors {
                let Some(info) = registry.ctor(ctor.as_str()) else {
                    continue;
                };
                for field in ctor_fields(&info.scheme, args) {
                    if let Some(v) = wire_violation(registry, &field, visited) {
                        return Some(v);
                    }
                }
            }
            None
        }
    }
}

/// A constructor's field types, instantiated at the owning ADT's type
/// arguments `args`.
///
/// The scheme is `∀params. fields → Adt<params>` (or the bare instance type
/// when nullary); the return type's variables are matched positionally
/// against `args` to build the instantiation.
fn ctor_fields(scheme: &Type, args: &[Type]) -> Vec<Type> {
    let body = match scheme {
        Type::TyForall(_, _, body) => body,
        other => other,
    };
    let Type::TyFn(fields, ret, _) = body else {
        return Vec::new();
    };
    let mut map = BTreeMap::new();
    if let Type::TyCon(_, ret_args) = ret.as_ref() {
        for (ret_arg, actual) in ret_args.iter().zip(args) {
            if let Type::TyVar(v) = ret_arg {
                map.insert(*v, actual.clone());
            }
        }
    }
    let rows = BTreeMap::new();
    fields.iter().map(|f| f.substitute(&map, &rows)).collect()
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
/// condensation), which is exactly the order generalisation needs — and, reused
/// by the whole-program driver, the order modules must be checked in.
pub(crate) fn tarjan(graph: &[Vec<usize>]) -> Vec<Vec<usize>> {
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

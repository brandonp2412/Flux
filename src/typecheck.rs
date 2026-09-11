use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, ConstantDef, Expr, ExprKind, Function, ListMatchPattern, MatchPattern, NamedArg,
    Program, Stmt, StmtKind, StructPatternField, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};
use crate::ir::ControlFlowGraph;

#[derive(Debug, Clone)]
pub struct Signature {
    pub public: bool,
    pub params: Vec<Type>,
    pub param_details: Vec<ParamSignature>,
    pub returns: Vec<Type>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ParamSignature {
    pub name: String,
    pub ty: Type,
    pub named_only: bool,
    pub default: Option<ConstantValue>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct StructFieldSignature {
    pub name: String,
    pub ty: Type,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct StructSignature {
    pub public: bool,
    pub fields: Vec<StructFieldSignature>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct EnumVariantSignature {
    pub name: String,
    pub payloads: Vec<Type>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct EnumSignature {
    pub public: bool,
    pub variants: Vec<EnumVariantSignature>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceSignature {
    pub public: bool,
    pub functions: HashMap<String, Signature>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceImplementationSignature {
    pub interface_name: String,
    pub target_name: String,
    pub functions: HashMap<String, String>,
    pub span: SourceSpan,
}

impl EnumSignature {
    pub fn variant(&self, name: &str) -> Option<&EnumVariantSignature> {
        self.variants.iter().find(|variant| variant.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstantValue {
    I64(i64),
    Bool(bool),
    Str(String),
}

impl ConstantValue {
    pub fn ty(&self) -> Type {
        match self {
            Self::I64(_) => Type::I64,
            Self::Bool(_) => Type::Bool,
            Self::Str(_) => Type::Str,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstantSignature {
    pub public: bool,
    pub ty: Type,
    pub value: ConstantValue,
    pub span: SourceSpan,
}

impl StructSignature {
    pub fn field(&self, name: &str) -> Option<&StructFieldSignature> {
        self.fields.iter().find(|field| field.name == name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Signatures {
    functions: HashMap<String, Signature>,
    interfaces: HashMap<String, InterfaceSignature>,
    implementations: HashMap<(String, String), InterfaceImplementationSignature>,
    structs: HashMap<String, StructSignature>,
    enums: HashMap<String, EnumSignature>,
    aliases: HashMap<String, Type>,
    alias_visibility: HashMap<String, (SourceSpan, bool)>,
    constants: HashMap<String, ConstantSignature>,
    module_imports: HashMap<SourceId, HashSet<SourceId>>,
}

impl Signatures {
    pub fn get(&self, name: &str) -> Option<&Signature> {
        self.functions.get(name)
    }

    pub fn interface(&self, name: &str) -> Option<&InterfaceSignature> {
        self.interfaces.get(name)
    }

    pub fn interfaces(&self) -> &HashMap<String, InterfaceSignature> {
        &self.interfaces
    }

    pub fn implementation(
        &self,
        interface_name: &str,
        target_name: &str,
    ) -> Option<&InterfaceImplementationSignature> {
        self.implementations
            .get(&(interface_name.to_string(), target_name.to_string()))
    }

    pub fn implementations(&self) -> &HashMap<(String, String), InterfaceImplementationSignature> {
        &self.implementations
    }

    pub fn struct_type(&self, name: &str) -> Option<&StructSignature> {
        self.structs.get(name)
    }

    pub fn structs(&self) -> &HashMap<String, StructSignature> {
        &self.structs
    }

    pub fn enum_type(&self, name: &str) -> Option<&EnumSignature> {
        self.enums.get(name)
    }

    pub fn enums(&self) -> &HashMap<String, EnumSignature> {
        &self.enums
    }

    pub fn type_alias(&self, name: &str) -> Option<&Type> {
        self.aliases.get(name)
    }

    fn alias_declaration(&self, name: &str) -> Option<(SourceSpan, bool)> {
        self.alias_visibility.get(name).copied()
    }

    pub fn constant(&self, name: &str) -> Option<&ConstantSignature> {
        self.constants.get(name)
    }

    pub fn canonical_type(&self, ty: &Type) -> Type {
        match ty {
            Type::Named(name) => self
                .aliases
                .get(name)
                .map(|target| self.canonical_type(target))
                .unwrap_or_else(|| ty.clone()),
            Type::List(element) => Type::List(Box::new(self.canonical_type(element))),
            Type::Optional(inner) => Type::Optional(Box::new(self.canonical_type(inner))),
            Type::Function { params, returns } => Type::Function {
                params: params.iter().map(|ty| self.canonical_type(ty)).collect(),
                returns: returns.iter().map(|ty| self.canonical_type(ty)).collect(),
            },
            _ => ty.clone(),
        }
    }

    pub fn is_copy_type(&self, ty: &Type) -> bool {
        self.is_copy_type_inner(ty, &mut HashSet::new())
    }

    fn is_copy_type_inner(&self, ty: &Type, visiting: &mut HashSet<String>) -> bool {
        match self.canonical_type(ty) {
            Type::I64 | Type::Bool | Type::Str | Type::Error => true,
            Type::Void | Type::List(_) => false,
            Type::Optional(inner) => self.is_copy_type_inner(&inner, visiting),
            Type::Function { .. } => true,
            Type::Named(name) => {
                if self.interface(&name).is_some() {
                    return true;
                }
                if !visiting.insert(name.clone()) {
                    // Recursive by-value aggregates are rejected by the dedicated cycle check.
                    // Treat the in-progress edge as provisionally copyable here so generic
                    // ownership gating does not replace that more precise diagnostic.
                    return true;
                }
                let copyable = if let Some(definition) = self.struct_type(&name) {
                    definition
                        .fields
                        .iter()
                        .all(|field| self.is_copy_type_inner(&field.ty, visiting))
                } else if let Some(definition) = self.enum_type(&name) {
                    definition.variants.iter().all(|variant| {
                        variant
                            .payloads
                            .iter()
                            .all(|payload| self.is_copy_type_inner(payload, visiting))
                    })
                } else {
                    false
                };
                visiting.remove(&name);
                copyable
            }
        }
    }

    fn contains_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    fn insert_function(&mut self, name: String, signature: Signature) {
        self.functions.insert(name, signature);
    }
}

pub fn check(program: &Program) -> Result<Signatures, Diagnostic> {
    check_all(program).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("check_all always returns at least one diagnostic on failure")
    })
}

pub fn check_all(program: &Program) -> Result<Signatures, Vec<Diagnostic>> {
    let mut signatures = Signatures {
        module_imports: module_import_closure(program),
        ..Signatures::default()
    };
    let mut diagnostics = Vec::new();

    let mut alias_targets = HashMap::new();
    for alias in &program.aliases {
        if matches!(
            alias.name.as_str(),
            "i64" | "bool" | "str" | "error" | "void"
        ) {
            diagnostics.push(diag(
                alias.name_span,
                &format!("type alias '{}' conflicts with a built-in type", alias.name),
            ));
            continue;
        }
        if alias_targets
            .insert(alias.name.clone(), alias.target.clone())
            .is_some()
        {
            diagnostics.push(diag(
                alias.name_span,
                &format!("duplicate type alias '{}'", alias.name),
            ));
        }
    }

    for alias in &program.aliases {
        if !alias_targets.contains_key(&alias.name) {
            continue;
        }
        match resolve_alias_target(&alias.target, &alias_targets, &mut vec![alias.name.clone()]) {
            Ok(target) => {
                signatures.aliases.insert(alias.name.clone(), target);
                signatures
                    .alias_visibility
                    .insert(alias.name.clone(), (alias.name_span, alias.public));
            }
            Err(chain) => diagnostics.push(
                diag(
                    alias.target_span,
                    &format!("type alias '{}' is recursive", alias.name),
                )
                .with_note(format!("alias cycle: {}", chain.join(" -> "))),
            ),
        }
    }

    for definition in &program.interfaces {
        if matches!(
            definition.name.as_str(),
            "i64" | "bool" | "str" | "error" | "void"
        ) {
            diagnostics.push(diag(
                definition.name_span,
                &format!(
                    "interface '{}' conflicts with a built-in type",
                    definition.name
                ),
            ));
            continue;
        }
        if signatures.aliases.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!(
                    "interface '{}' conflicts with a type alias",
                    definition.name
                ),
            ));
            continue;
        }
        if signatures.interfaces.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!("duplicate interface '{}'", definition.name),
            ));
            continue;
        }
        signatures.interfaces.insert(
            definition.name.clone(),
            InterfaceSignature {
                public: definition.public,
                functions: HashMap::new(),
                span: definition.name_span,
            },
        );
    }

    for definition in &program.structs {
        if signatures.aliases.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!("struct '{}' conflicts with a type alias", definition.name),
            ));
            continue;
        }
        if signatures.interfaces.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!(
                    "struct '{}' conflicts with an interface name",
                    definition.name
                ),
            ));
            continue;
        }
        if signatures.structs.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!("duplicate struct '{}'", definition.name),
            ));
            continue;
        }
        signatures.structs.insert(
            definition.name.clone(),
            StructSignature {
                public: definition.public,
                fields: Vec::new(),
                span: definition.name_span,
            },
        );
    }

    for definition in &program.enums {
        if signatures.aliases.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!("enum '{}' conflicts with a type alias", definition.name),
            ));
            continue;
        }
        if signatures.interfaces.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!(
                    "enum '{}' conflicts with an interface name",
                    definition.name
                ),
            ));
            continue;
        }
        if signatures.structs.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!("enum '{}' conflicts with a struct name", definition.name),
            ));
            continue;
        }
        if signatures.enums.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!("duplicate enum '{}'", definition.name),
            ));
            continue;
        }
        signatures.enums.insert(
            definition.name.clone(),
            EnumSignature {
                public: definition.public,
                variants: Vec::new(),
                span: definition.name_span,
            },
        );
    }

    for definition in &program.structs {
        let fields = definition
            .fields
            .iter()
            .map(|field| StructFieldSignature {
                name: field.name.clone(),
                ty: signatures.canonical_type(&field.ty),
                span: field.name_span,
            })
            .collect::<Vec<_>>();
        if let Some(signature) = signatures.structs.get_mut(&definition.name) {
            signature.fields = fields;
        }
        for field in &definition.fields {
            if let Err(diagnostic) =
                require_storable_value_type(field.type_span, &field.ty, &signatures)
            {
                diagnostics.push(diagnostic);
            }
            if definition.public
                && let Err(diagnostic) =
                    require_publicly_nameable_type(field.type_span, &field.ty, &signatures)
            {
                diagnostics.push(diagnostic);
            }
        }
    }

    for definition in &program.enums {
        let variants = definition
            .variants
            .iter()
            .map(|variant| EnumVariantSignature {
                name: variant.name.clone(),
                payloads: variant
                    .payloads
                    .iter()
                    .map(|payload| signatures.canonical_type(&payload.ty))
                    .collect(),
                span: variant.name_span,
            })
            .collect::<Vec<_>>();
        if let Some(signature) = signatures.enums.get_mut(&definition.name) {
            signature.variants = variants;
        }
        for variant in &definition.variants {
            for payload in &variant.payloads {
                if let Err(diagnostic) =
                    require_storable_value_type(payload.type_span, &payload.ty, &signatures)
                {
                    diagnostics.push(diagnostic);
                }
                if definition.public
                    && let Err(diagnostic) =
                        require_publicly_nameable_type(payload.type_span, &payload.ty, &signatures)
                {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    for alias in &program.aliases {
        if let Err(diagnostic) = require_known_type(alias.target_span, &alias.target, &signatures) {
            diagnostics.push(diagnostic);
        }
        if alias.public
            && let Err(diagnostic) =
                require_publicly_nameable_type(alias.target_span, &alias.target, &signatures)
        {
            diagnostics.push(diagnostic);
        }
    }

    for definition in &program.interfaces {
        if !signatures.interfaces.contains_key(&definition.name) {
            continue;
        }
        let mut members = HashMap::new();
        for function in &definition.functions {
            let mut param_details = Vec::with_capacity(function.params.len());
            for param in &function.params {
                if let Err(diagnostic) = require_known_type(param.type_span, &param.ty, &signatures)
                {
                    diagnostics.push(diagnostic);
                }
                if definition.public
                    && let Err(diagnostic) =
                        require_publicly_nameable_type(param.type_span, &param.ty, &signatures)
                {
                    diagnostics.push(diagnostic);
                }
                param_details.push(ParamSignature {
                    name: param.name.clone(),
                    ty: signatures.canonical_type(&param.ty),
                    named_only: param.named_only,
                    default: None,
                    span: param.name_span,
                });
            }
            for (index, ty) in function.returns.iter().enumerate() {
                let span = function
                    .return_type_spans
                    .get(index)
                    .copied()
                    .unwrap_or(function.return_span);
                if let Err(diagnostic) = require_known_type(span, ty, &signatures) {
                    diagnostics.push(diagnostic);
                }
                if matches!(signatures.canonical_type(ty), Type::List(_)) {
                    diagnostics.push(diag(
                        span,
                        "list values cannot be returned from interface capabilities until collection ownership is implemented",
                    ));
                }
                if definition.public
                    && let Err(diagnostic) = require_publicly_nameable_type(span, ty, &signatures)
                {
                    diagnostics.push(diagnostic);
                }
            }
            members.insert(
                function.name.clone(),
                Signature {
                    public: true,
                    params: param_details.iter().map(|param| param.ty.clone()).collect(),
                    param_details,
                    returns: function
                        .returns
                        .iter()
                        .map(|ty| signatures.canonical_type(ty))
                        .collect(),
                    span: function.name_span,
                },
            );
        }
        if let Some(interface) = signatures.interfaces.get_mut(&definition.name) {
            interface.functions = members;
        }
    }

    let interface_defs = program
        .interfaces
        .iter()
        .map(|definition| (definition.name.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let mut composed_cache = HashMap::new();
    for definition in &program.interfaces {
        if !signatures.interfaces.contains_key(&definition.name) {
            continue;
        }
        let mut visiting = Vec::new();
        match resolve_interface_composition(
            &definition.name,
            &interface_defs,
            &signatures,
            &mut composed_cache,
            &mut visiting,
        ) {
            Ok(functions) => {
                if let Some(interface) = signatures.interfaces.get_mut(&definition.name) {
                    interface.functions = functions;
                }
            }
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    let mut constant_defs = HashMap::new();
    for constant in &program.constants {
        if signatures.aliases.contains_key(&constant.name)
            || signatures.interfaces.contains_key(&constant.name)
            || signatures.structs.contains_key(&constant.name)
            || signatures.enums.contains_key(&constant.name)
        {
            diagnostics.push(diag(
                constant.name_span,
                &format!("constant '{}' conflicts with a type name", constant.name),
            ));
            continue;
        }
        if constant_defs
            .insert(constant.name.as_str(), constant)
            .is_some()
        {
            diagnostics.push(diag(
                constant.name_span,
                &format!("duplicate constant '{}'", constant.name),
            ));
        }
        if let Err(diagnostic) = require_known_type(constant.type_span, &constant.ty, &signatures) {
            diagnostics.push(diagnostic);
        }
        if constant.public
            && let Err(diagnostic) =
                require_publicly_nameable_type(constant.type_span, &constant.ty, &signatures)
        {
            diagnostics.push(diagnostic);
        }
        let declared = signatures.canonical_type(&constant.ty);
        if !matches!(declared, Type::I64 | Type::Bool | Type::Str) {
            diagnostics.push(diag(
                constant.type_span,
                "compile-time constants currently support i64, bool, and str",
            ));
        }
    }

    let mut constant_cache = HashMap::new();
    for constant in &program.constants {
        if !constant_defs.contains_key(constant.name.as_str()) {
            continue;
        }
        let mut stack = Vec::new();
        match evaluate_constant(
            constant.name.as_str(),
            &constant_defs,
            &signatures,
            &mut constant_cache,
            &mut stack,
        ) {
            Ok(_) => {}
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    signatures.constants = constant_cache;

    for function in &program.functions {
        if matches!(
            function.name.as_str(),
            "print"
                | "error"
                | "take"
                | "skip"
                | "any"
                | "every"
                | "fold"
                | "reduce"
                | "map"
                | "filter"
                | "where"
                | "concat"
                | "distinct"
                | "flatten"
                | "sorted"
                | "chunked"
        ) {
            diagnostics.push(diag(
                function.name_span,
                &format!("'{}' is a built-in function name", function.name),
            ));
            continue;
        }
        if signatures.interfaces.contains_key(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!(
                    "function '{}' conflicts with an interface name",
                    function.name
                ),
            ));
            continue;
        }
        if signatures.structs.contains_key(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!("function '{}' conflicts with a struct name", function.name),
            ));
            continue;
        }
        if signatures.enums.contains_key(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!("function '{}' conflicts with an enum name", function.name),
            ));
            continue;
        }
        if signatures.aliases.contains_key(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!("function '{}' conflicts with a type alias", function.name),
            ));
            continue;
        }
        if signatures.constants.contains_key(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!("function '{}' conflicts with a constant", function.name),
            ));
            continue;
        }
        if signatures.contains_function(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!("duplicate function '{}'", function.name),
            ));
            continue;
        }
        let mut param_details = Vec::with_capacity(function.params.len());
        for param in &function.params {
            if let Err(diagnostic) = require_known_type(param.type_span, &param.ty, &signatures) {
                diagnostics.push(diagnostic);
            }
            if function.public
                && let Err(diagnostic) =
                    require_publicly_nameable_type(param.type_span, &param.ty, &signatures)
            {
                diagnostics.push(diagnostic);
            }
            let ty = signatures.canonical_type(&param.ty);
            let default = if let Some(default) = &param.default {
                match evaluate_default_expr(default, &signatures) {
                    Ok(value) => {
                        if let Err(diagnostic) = require_type(
                            default.span,
                            &ty,
                            &value.ty(),
                            &format!("default for parameter '{}'", param.name),
                        ) {
                            diagnostics.push(diagnostic);
                            None
                        } else {
                            Some(value)
                        }
                    }
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                }
            } else {
                None
            };
            param_details.push(ParamSignature {
                name: param.name.clone(),
                ty,
                named_only: param.named_only,
                default,
                span: param.name_span,
            });
        }
        for (index, ty) in function.returns.iter().enumerate() {
            let span = function
                .return_type_spans
                .get(index)
                .copied()
                .unwrap_or(function.return_span);
            if let Err(diagnostic) = require_known_type(span, ty, &signatures) {
                diagnostics.push(diagnostic);
            }
            if matches!(signatures.canonical_type(ty), Type::List(_)) {
                diagnostics.push(diag(
                    span,
                    "list values cannot be returned from functions until collection ownership is implemented",
                ));
            }
            if function.public
                && let Err(diagnostic) = require_publicly_nameable_type(span, ty, &signatures)
            {
                diagnostics.push(diagnostic);
            }
        }
        signatures.insert_function(
            function.name.clone(),
            Signature {
                public: function.public,
                params: param_details.iter().map(|param| param.ty.clone()).collect(),
                param_details,
                returns: function
                    .returns
                    .iter()
                    .map(|ty| signatures.canonical_type(ty))
                    .collect(),
                span: function.name_span,
            },
        );
    }

    for implementation in &program.implementations {
        let Some(interface) = signatures
            .interface(&implementation.interface_name)
            .cloned()
        else {
            diagnostics.push(diag(
                implementation.interface_span,
                &format!("unknown interface '{}'", implementation.interface_name),
            ));
            continue;
        };
        if let Err(diagnostic) = require_visible_declaration(
            implementation.interface_span,
            interface.span,
            interface.public,
            "interface",
            &implementation.interface_name,
            &signatures,
        ) {
            diagnostics.push(diagnostic);
            continue;
        }
        if let Err(diagnostic) = require_visible_named_type(
            implementation.target_span,
            &implementation.target_name,
            &signatures,
        ) {
            diagnostics.push(diagnostic);
            continue;
        }
        let target_ty = signatures.canonical_type(&Type::Named(implementation.target_name.clone()));
        let Type::Named(target_name) = &target_ty else {
            diagnostics.push(diag(
                implementation.target_span,
                &format!(
                    "interface implementation target '{}' is not a concrete data type",
                    implementation.target_name
                ),
            ));
            continue;
        };
        if signatures.struct_type(target_name).is_none()
            && signatures.enum_type(target_name).is_none()
        {
            diagnostics.push(diag(
                implementation.target_span,
                &format!(
                    "interface implementation target '{}' is not a concrete struct or enum",
                    implementation.target_name
                ),
            ));
            continue;
        }
        let key = (implementation.interface_name.clone(), target_name.clone());
        if signatures.implementations.contains_key(&key) {
            diagnostics.push(diag(
                implementation.span,
                &format!(
                    "duplicate implementation of '{}' for '{}'",
                    implementation.interface_name, implementation.target_name
                ),
            ));
            continue;
        }

        let mut mapped = HashMap::new();
        for mapping in &implementation.mappings {
            let Some(member) = interface.functions.get(&mapping.member) else {
                diagnostics.push(diag(
                    mapping.member_span,
                    &format!(
                        "interface '{}' has no capability '{}'",
                        implementation.interface_name, mapping.member
                    ),
                ));
                continue;
            };
            let Some(function) = signatures.get(&mapping.function) else {
                diagnostics.push(diag(
                    mapping.function_span,
                    &format!("unknown implementation function '{}'", mapping.function),
                ));
                continue;
            };
            if let Err(diagnostic) = require_visible_declaration(
                mapping.function_span,
                function.span,
                function.public,
                "function",
                &mapping.function,
                &signatures,
            ) {
                diagnostics.push(diagnostic);
                continue;
            }
            if function.params.len() != member.params.len() + 1 {
                diagnostics.push(diag(
                    mapping.function_span,
                    &format!(
                        "implementation function '{}' for '{}.{}' must accept the concrete '{}' receiver plus {} capability argument{}, got {} parameters",
                        mapping.function,
                        implementation.interface_name,
                        mapping.member,
                        target_name,
                        member.params.len(),
                        if member.params.len() == 1 { "" } else { "s" },
                        function.params.len()
                    ),
                ));
                continue;
            }
            let receiver = &function.param_details[0];
            if receiver.named_only || receiver.ty != target_ty {
                diagnostics.push(diag(
                    receiver.span,
                    &format!(
                        "implementation function '{}' must take '{}' as its first positional parameter",
                        mapping.function, target_name
                    ),
                ));
                continue;
            }
            let mut compatible = true;
            for (index, (actual, expected)) in function
                .param_details
                .iter()
                .skip(1)
                .zip(&member.param_details)
                .enumerate()
            {
                if actual.ty != expected.ty || actual.named_only != expected.named_only {
                    diagnostics.push(diag(
                        actual.span,
                        &format!(
                            "implementation parameter {} of '{}' does not match capability '{}.{}'",
                            index + 1,
                            mapping.function,
                            implementation.interface_name,
                            mapping.member
                        ),
                    ));
                    compatible = false;
                    break;
                }
                if expected.named_only && actual.name != expected.name {
                    diagnostics.push(diag(
                        actual.span,
                        &format!(
                            "named implementation parameter '{}' must keep capability name '{}'",
                            actual.name, expected.name
                        ),
                    ));
                    compatible = false;
                    break;
                }
            }
            if !compatible {
                continue;
            }
            if function.returns != member.returns {
                diagnostics.push(diag(
                    mapping.function_span,
                    &format!(
                        "implementation function '{}' returns {}, capability '{}.{}' requires {}",
                        mapping.function,
                        return_types_name(&function.returns),
                        implementation.interface_name,
                        mapping.member,
                        return_types_name(&member.returns)
                    ),
                ));
                continue;
            }
            mapped.insert(mapping.member.clone(), mapping.function.clone());
        }
        let missing = interface
            .functions
            .keys()
            .filter(|member| !mapped.contains_key(*member))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            diagnostics.push(diag(
                implementation.span,
                &format!(
                    "implementation of '{}' for '{}' is missing capability mapping{} {}",
                    implementation.interface_name,
                    implementation.target_name,
                    if missing.len() == 1 { "" } else { "s" },
                    missing.join(", ")
                ),
            ));
            continue;
        }
        if mapped.len() != interface.functions.len() {
            continue;
        }
        signatures.implementations.insert(
            key,
            InterfaceImplementationSignature {
                interface_name: implementation.interface_name.clone(),
                target_name: target_name.clone(),
                functions: mapped,
                span: implementation.span,
            },
        );
    }

    validate_views(program, &signatures, &mut diagnostics);

    if let Some(application) = &program.application {
        match program
            .views
            .iter()
            .find(|view| view.name == application.view_name)
        {
            None => diagnostics.push(diag(
                application.view_span,
                &format!("unknown app root view '{}'", application.view_name),
            )),
            Some(view) => {
                if let Err(diagnostic) = require_visible_declaration(
                    application.view_span,
                    view.name_span,
                    view.public,
                    "view",
                    &view.name,
                    &signatures,
                ) {
                    diagnostics.push(diagnostic);
                }
                if !view.params.is_empty() {
                    diagnostics.push(diag(
                        application.view_span,
                        "bootstrap app root view must not declare parameters",
                    ));
                }
            }
        }
        for field in &application.metadata {
            let lifecycle_signature = match field.name.as_str() {
                "onStart"
                | "onResume"
                | "onPause"
                | "onStop"
                | "onExit"
                | "onConfigurationChanged"
                | "onLowMemory" => Some(Type::Function {
                    params: Vec::new(),
                    returns: Vec::new(),
                }),
                "onBackgroundJob" => Some(Type::Function {
                    params: vec![Type::I64],
                    returns: Vec::new(),
                }),
                "onSaveState" => Some(Type::Function {
                    params: Vec::new(),
                    returns: vec![Type::Str],
                }),
                "onRestoreState" | "onOpenUrl" => Some(Type::Function {
                    params: vec![Type::Str],
                    returns: Vec::new(),
                }),
                _ => None,
            };
            if let Some(expected) = lifecycle_signature {
                if !matches!(field.value.kind, ExprKind::Var(_)) {
                    diagnostics.push(diag(
                        field.value.span,
                        &format!(
                            "application {} requires a named lifecycle callback",
                            field.name
                        ),
                    ));
                    continue;
                }
                match type_of_expr(&field.value, &HashMap::new(), &signatures) {
                    Ok(actual) => {
                        if let Err(diagnostic) = require_type(
                            field.value.span,
                            &expected,
                            &actual,
                            &format!("application {} callback", field.name),
                        ) {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                continue;
            }
            match evaluate_default_expr(&field.value, &signatures) {
                Ok(value) => match field.name.as_str() {
                    "title" | "id" | "theme" | "layoutDirection" | "surfaceColor"
                    | "surfaceRaisedColor" | "textColor" | "textMutedColor" | "accentColor"
                    | "onAccentColor" | "outlineColor" | "dangerColor" | "successColor"
                    | "warningColor" | "shadowColor"
                        if value.ty() != Type::Str =>
                    {
                        diagnostics.push(diag(
                            field.value.span,
                            &format!(
                                "application {} must be str, got {}",
                                field.name,
                                value.ty().name()
                            ),
                        ))
                    }
                    "id" => {
                        if let ConstantValue::Str(value) = value
                            && !valid_application_id(&value)
                        {
                            diagnostics.push(diag(
                                field.value.span,
                                "application id must be a valid reverse-DNS-style identifier",
                            ));
                        }
                    }
                    "theme" => {
                        if let ConstantValue::Str(value) = value
                            && !matches!(value.as_str(), "system" | "light" | "dark")
                        {
                            diagnostics.push(diag(
                                field.value.span,
                                "application theme must be one of 'system', 'light', or 'dark'",
                            ));
                        }
                    }
                    "layoutDirection" => {
                        if let ConstantValue::Str(value) = value
                            && !matches!(value.as_str(), "system" | "ltr" | "rtl")
                        {
                            diagnostics.push(diag(
                                field.value.span,
                                "application layoutDirection must be one of 'system', 'ltr', or 'rtl'",
                            ));
                        }
                    }
                    "surfaceColor" | "surfaceRaisedColor" | "textColor" | "textMutedColor"
                    | "accentColor" | "onAccentColor" | "outlineColor" | "dangerColor"
                    | "successColor" | "warningColor" | "shadowColor" => {
                        if let ConstantValue::Str(value) = value
                            && !valid_hex_ui_color(&value)
                        {
                            diagnostics.push(diag(
                                field.value.span,
                                &format!(
                                    "application {} must use '#RRGGBB' or '#RRGGBBAA'",
                                    field.name
                                ),
                            ));
                        }
                    }
                    "resizable" if value.ty() != Type::Bool => diagnostics.push(diag(
                        field.value.span,
                        &format!(
                            "application resizable must be bool, got {}",
                            value.ty().name()
                        ),
                    )),
                    "width" | "height" if value.ty() != Type::I64 => diagnostics.push(diag(
                        field.value.span,
                        &format!(
                            "application {} must be i64, got {}",
                            field.name,
                            value.ty().name()
                        ),
                    )),
                    "width" | "height" => {
                        if let ConstantValue::I64(value) = value
                            && value <= 0
                        {
                            diagnostics.push(diag(
                                field.value.span,
                                &format!("application {} must be greater than zero", field.name),
                            ));
                        }
                    }
                    _ => {}
                },
                Err(mut diagnostic) => {
                    diagnostic.message = format!(
                        "application metadata '{}' must be compile-time: {}",
                        field.name, diagnostic.message
                    );
                    diagnostics.push(diagnostic);
                }
            }
        }
        if signatures.get("main").is_some() {
            diagnostics.push(Diagnostic::global(
                DiagnosticStage::Type,
                "an app declaration replaces fn main() -> i64; declare one application entry model",
            ));
        }
    } else {
        match signatures.get("main") {
            None => diagnostics.push(
                Diagnostic::global(
                    DiagnosticStage::Type,
                    "program requires fn main() -> i64 { ... } or app ViewName",
                )
                .with_note(
                    "native executables enter Flux through a parameterless main or app root view",
                ),
            ),
            Some(main) if !main.params.is_empty() || main.returns != vec![Type::I64] => {
                diagnostics.push(
                    Diagnostic::global(
                        DiagnosticStage::Type,
                        "main must have signature fn main() -> i64",
                    )
                    .with_note(
                        "main currently receives no parameters and returns the process exit code",
                    ),
                );
            }
            Some(_) => {}
        }
    }

    for function in &program.functions {
        check_function_all(function, &signatures, &mut diagnostics);
    }

    if diagnostics.is_empty() {
        Ok(signatures)
    } else {
        Err(diagnostics)
    }
}

pub const VIEW_ENVIRONMENT_BINDINGS: &[(&str, Type)] = &[
    ("windowWidth", Type::I64),
    ("windowHeight", Type::I64),
    ("windowIsLandscape", Type::Bool),
    ("windowIsPortrait", Type::Bool),
    ("windowIsCompact", Type::Bool),
    ("windowIsMedium", Type::Bool),
    ("windowIsExpanded", Type::Bool),
    ("displayScale", Type::I64),
];

pub fn view_environment_type(name: &str) -> Option<Type> {
    let source_name = internal_name_to_source(name);
    VIEW_ENVIRONMENT_BINDINGS
        .iter()
        .find(|(candidate, _)| *candidate == name || *candidate == source_name)
        .map(|(_, ty)| ty.clone())
        .or_else(|| semantic_ui_i64_token(name).map(|_| Type::I64))
}

pub fn view_property_type(kind: &str, property: &str) -> Option<Type> {
    let internal_property = source_name_to_internal(property);
    let property = internal_property.as_str();
    if BUILTIN_VIEW_ELEMENT_KINDS.contains(&kind) {
        match property {
            "visible"
            | "clip"
            | "focusable"
            | "autofocus"
            | "accessibility_hidden"
            | "drag_translate"
            | "pinch_scale" => {
                return Some(Type::Bool);
            }
            "tooltip"
            | "context_menu_label"
            | "drag_text"
            | "shortcut"
            | "shortcut_scope"
            | "accessibility_label"
            | "accessibility_description"
            | "accessibility_value"
            | "accessibility_role" => {
                return Some(Type::Str);
            }
            "context_menu_items" => {
                return Some(Type::List(Box::new(Type::Str)));
            }
            "min_width"
            | "min_height"
            | "focus_scope"
            | "accessibility_order"
            | "margin"
            | "margin_top"
            | "margin_bottom"
            | "margin_start"
            | "margin_end"
            | "padding"
            | "padding_top"
            | "padding_bottom"
            | "padding_start"
            | "padding_end"
            | "border_width"
            | "border_top_width"
            | "border_bottom_width"
            | "border_start_width"
            | "border_end_width"
            | "radius"
            | "radius_top_left"
            | "radius_top_right"
            | "radius_bottom_left"
            | "radius_bottom_right"
            | "shadow_blur"
            | "shadow_offset_x"
            | "shadow_offset_y"
            | "translate_x"
            | "translate_y"
            | "rotate_degrees"
            | "scale_percent"
            | "scale_x_percent"
            | "scale_y_percent"
            | "skew_x_degrees"
            | "skew_y_degrees"
            | "transform_origin_x_percent"
            | "transform_origin_y_percent"
            | "transition_ms"
            | "transition_delay_ms" => {
                return Some(Type::I64);
            }
            "align_x"
            | "align_y"
            | "status"
            | "background_color"
            | "border_color"
            | "border_top_color"
            | "border_bottom_color"
            | "border_start_color"
            | "border_end_color"
            | "border_style"
            | "shadow_color"
            | "transition_easing" => {
                return Some(Type::Str);
            }
            "on_tap"
            | "on_double_tap"
            | "on_long_press"
            | "on_context_menu"
            | "on_context_menu_select"
            | "on_hover"
            | "on_leave"
            | "on_focus"
            | "on_blur" => {
                return Some(Type::Function {
                    params: Vec::new(),
                    returns: Vec::new(),
                });
            }
            "on_drag" | "on_swipe" => {
                return Some(Type::Function {
                    params: vec![Type::I64, Type::I64],
                    returns: Vec::new(),
                });
            }
            "on_scale" => {
                return Some(Type::Function {
                    params: vec![Type::I64],
                    returns: Vec::new(),
                });
            }
            "on_key" | "on_drop" => {
                return Some(Type::Function {
                    params: vec![Type::Str],
                    returns: Vec::new(),
                });
            }
            "on_context_menu_item_select" => {
                return Some(Type::Function {
                    params: vec![Type::I64],
                    returns: Vec::new(),
                });
            }
            _ => {}
        }
    }
    match (kind, property) {
        ("Text", "text")
        | ("Text", "rich_text")
        | ("Text", "variant")
        | ("Text", "color")
        | ("Text", "font_family")
        | ("Text", "text_align")
        | ("Text", "wrap_mode")
        | ("Text", "ellipsize") => Some(Type::Str),
        ("Text", "selectable")
        | ("Text", "bold")
        | ("Text", "italic")
        | ("Text", "underline")
        | ("Text", "strikethrough")
        | ("Text", "wrap") => Some(Type::Bool),
        ("Text", "size")
        | ("Text", "letter_spacing")
        | ("Text", "line_height_percent")
        | ("Text", "max_lines")
        | ("Text", "max_width_chars") => Some(Type::I64),
        ("Button", "text") => Some(Type::Str),
        ("Button", "enabled") => Some(Type::Bool),
        ("Button", "primary") => Some(Type::Bool),
        ("Button", "on_press") | ("Toggle", "on_change") | ("Radio", "on_select") => {
            Some(Type::Function {
                params: Vec::new(),
                returns: Vec::new(),
            })
        }
        ("TextInput", "text")
        | ("TextInput", "placeholder")
        | ("TextInput", "keyboard_type")
        | ("TextInput", "validation_state") => Some(Type::Str),
        ("TextInput", "enabled")
        | ("TextInput", "read_only")
        | ("TextInput", "password")
        | ("TextInput", "multiline")
        | ("TextInput", "submit_on_enter") => Some(Type::Bool),
        ("TextInput", "max_length") => Some(Type::I64),
        ("TextInput", "on_submit") | ("TextInput", "on_change") => Some(Type::Function {
            params: vec![Type::Str],
            returns: Vec::new(),
        }),
        ("Image", "source") | ("Image", "alt") | ("Image", "fit") => Some(Type::Str),
        ("Image", "can_shrink") => Some(Type::Bool),
        ("Toggle", "label") | ("Radio", "label") => Some(Type::Str),
        ("Toggle", "checked")
        | ("Toggle", "enabled")
        | ("Radio", "selected")
        | ("Radio", "enabled") => Some(Type::Bool),
        ("Nav", "label") | ("Chart", "label") | ("Content", "label") => Some(Type::Str),
        ("Card", "title") | ("Header", "text") => Some(Type::Str),
        _ => None,
    }
}

pub(crate) fn source_name_to_internal(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn internal_name_to_source(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut uppercase_next = false;
    for ch in name.chars() {
        if ch == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            out.push(ch.to_ascii_uppercase());
            uppercase_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn view_element_property_type(
    program: &Program,
    signatures: &Signatures,
    kind: &str,
    property: &str,
) -> Option<Type> {
    view_property_type(kind, property).or_else(|| {
        program
            .views
            .iter()
            .find(|view| view.name == kind)
            .and_then(|view| view.params.iter().find(|param| param.name == property))
            .map(|param| signatures.canonical_type(&param.ty))
    })
}

pub const UI_API_VERSION: u32 = 1;

pub const BUILTIN_VIEW_ELEMENT_KINDS: &[&str] = &[
    "Text",
    "Button",
    "TextInput",
    "Image",
    "Toggle",
    "Radio",
    "Nav",
    "Chart",
    "Card",
    "Header",
    "Content",
];

pub const ACCESSIBILITY_ROLES: &[&str] = &[
    "label", "heading", "button", "textBox", "checkbox", "radio", "image", "switch",
];

pub const TEXT_INPUT_VALIDATION_STATES: &[&str] = &["normal", "error", "success", "warning"];
pub const UI_PRESENTATION_STATES: &[&str] = &["normal", "loading", "empty", "error"];
pub const SHORTCUT_SCOPES: &[&str] = &["window", "focused"];
pub const TRANSITION_EASINGS: &[&str] = &[
    "linear",
    "ease",
    "easeIn",
    "easeOut",
    "easeInOut",
    "spring",
    "springGentle",
    "springSnappy",
];

pub fn transition_easing_css_value(value: &str) -> Option<&'static str> {
    match value {
        "linear" => Some("linear"),
        "ease" => Some("ease"),
        "easeIn" | "ease_in" => Some("ease-in"),
        "easeOut" | "ease_out" => Some("ease-out"),
        "easeInOut" | "ease_in_out" => Some("ease-in-out"),
        "spring" => Some("cubic-bezier(0.22, 1.20, 0.36, 1)"),
        "springGentle" | "spring_gentle" => Some("cubic-bezier(0.34, 1.36, 0.64, 1)"),
        "springSnappy" | "spring_snappy" => Some("cubic-bezier(0.16, 1.30, 0.30, 1)"),
        _ => None,
    }
}

pub const SEMANTIC_UI_COLOR_TOKENS: &[&str] = &[
    "surface",
    "surfaceRaised",
    "text",
    "textMuted",
    "accent",
    "onAccent",
    "outline",
    "danger",
    "success",
    "warning",
    "shadow",
    "transparent",
];

pub const SEMANTIC_UI_I64_TOKENS: &[(&str, i64)] = &[
    ("spaceXs", 4),
    ("spaceSm", 8),
    ("spaceMd", 12),
    ("spaceLg", 16),
    ("spaceXl", 24),
    ("spaceXxl", 32),
    ("radiusSm", 6),
    ("radiusMd", 10),
    ("radiusLg", 16),
    ("radiusPill", 999),
    ("elevationLow", 2),
    ("elevationMd", 8),
    ("elevationHigh", 16),
    ("motionFast", 120),
    ("motionNormal", 200),
    ("motionSlow", 320),
];

pub fn semantic_ui_i64_token(name: &str) -> Option<i64> {
    let source_name = internal_name_to_source(name);
    SEMANTIC_UI_I64_TOKENS
        .iter()
        .find(|(candidate, _)| *candidate == name || *candidate == source_name)
        .map(|(_, value)| *value)
}

pub fn valid_hex_ui_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn valid_ui_color(value: &str) -> bool {
    valid_hex_ui_color(value) || SEMANTIC_UI_COLOR_TOKENS.contains(&value)
}

fn view_element_kind_is_builtin(kind: &str) -> bool {
    BUILTIN_VIEW_ELEMENT_KINDS.contains(&kind)
}

const COMMON_VIEW_PROPERTIES: &[&str] = &[
    "visible",
    "clip",
    "focusable",
    "autofocus",
    "focus_scope",
    "status",
    "tooltip",
    "context_menu_label",
    "context_menu_items",
    "drag_text",
    "shortcut",
    "shortcut_scope",
    "accessibility_label",
    "accessibility_description",
    "accessibility_value",
    "accessibility_role",
    "accessibility_hidden",
    "accessibility_order",
    "on_tap",
    "on_double_tap",
    "on_long_press",
    "on_context_menu",
    "on_context_menu_select",
    "on_context_menu_item_select",
    "on_drop",
    "on_drag",
    "on_swipe",
    "on_scale",
    "drag_translate",
    "pinch_scale",
    "on_hover",
    "on_leave",
    "on_focus",
    "on_blur",
    "on_key",
    "align_x",
    "align_y",
    "margin",
    "margin_top",
    "margin_bottom",
    "margin_start",
    "margin_end",
    "padding",
    "padding_top",
    "padding_bottom",
    "padding_start",
    "padding_end",
    "background_color",
    "border_color",
    "border_top_color",
    "border_bottom_color",
    "border_start_color",
    "border_end_color",
    "border_width",
    "border_top_width",
    "border_bottom_width",
    "border_start_width",
    "border_end_width",
    "border_style",
    "radius",
    "radius_top_left",
    "radius_top_right",
    "radius_bottom_left",
    "radius_bottom_right",
    "shadow_color",
    "shadow_blur",
    "shadow_offset_x",
    "shadow_offset_y",
    "translate_x",
    "translate_y",
    "rotate_degrees",
    "scale_percent",
    "scale_x_percent",
    "scale_y_percent",
    "skew_x_degrees",
    "skew_y_degrees",
    "transform_origin_x_percent",
    "transform_origin_y_percent",
    "transition_ms",
    "transition_delay_ms",
    "transition_easing",
    "min_width",
    "min_height",
];

pub fn view_property_names(kind: &str) -> Vec<String> {
    let specific: &[&str] = match kind {
        "Text" => &[
            "text",
            "rich_text",
            "variant",
            "selectable",
            "size",
            "bold",
            "italic",
            "underline",
            "strikethrough",
            "font_family",
            "letter_spacing",
            "line_height_percent",
            "text_align",
            "wrap",
            "wrap_mode",
            "ellipsize",
            "max_lines",
            "max_width_chars",
            "color",
        ],
        "Button" => &["text", "enabled", "primary", "on_press"],
        "TextInput" => &[
            "text",
            "placeholder",
            "enabled",
            "read_only",
            "password",
            "multiline",
            "submit_on_enter",
            "max_length",
            "keyboard_type",
            "validation_state",
            "on_change",
            "on_submit",
        ],
        "Image" => &["source", "alt", "fit", "can_shrink"],
        "Toggle" => &["label", "checked", "enabled", "on_change"],
        "Radio" => &["label", "selected", "enabled", "on_select"],
        "Nav" | "Chart" | "Content" => &["label"],
        "Card" => &["title"],
        "Header" => &["text"],
        _ => return Vec::new(),
    };
    specific
        .iter()
        .copied()
        .chain(COMMON_VIEW_PROPERTIES.iter().copied())
        .map(internal_name_to_source)
        .collect()
}

fn valid_application_id(value: &str) -> bool {
    if value.len() > 255 || !value.contains('.') || value.starts_with('.') {
        return false;
    }
    value.split('.').all(|segment| {
        !segment.is_empty()
            && !segment.as_bytes()[0].is_ascii_digit()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    })
}

fn validate_views(program: &Program, signatures: &Signatures, diagnostics: &mut Vec<Diagnostic>) {
    let mut view_defs = HashMap::new();
    for view in &program.views {
        if view_element_kind_is_builtin(&view.name) {
            diagnostics.push(diag(
                view.name_span,
                &format!(
                    "view '{}' conflicts with a built-in view element type",
                    view.name
                ),
            ));
        }
        if view_defs.insert(view.name.as_str(), view).is_some() {
            diagnostics.push(diag(
                view.name_span,
                &format!("duplicate view '{}'", view.name),
            ));
        }

        for param in &view.params {
            if view_environment_type(&param.name).is_some() {
                diagnostics.push(diag(
                    param.name_span,
                    &format!(
                        "view parameter '{}' conflicts with a read-only view environment binding",
                        param.name
                    ),
                ));
            }
            if let Err(diagnostic) = require_known_type(param.type_span, &param.ty, signatures) {
                diagnostics.push(diagnostic);
            }
            if view.public
                && let Err(diagnostic) =
                    require_publicly_nameable_type(param.type_span, &param.ty, signatures)
            {
                diagnostics.push(diagnostic);
            }
            if let Some(default) = &param.default {
                match evaluate_default_expr(default, signatures) {
                    Ok(value) => {
                        let expected = signatures.canonical_type(&param.ty);
                        if let Err(diagnostic) = require_type(
                            default.span,
                            &expected,
                            &value.ty(),
                            &format!("default for view parameter '{}'", param.name),
                        ) {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
            }
        }

        for state in &view.states {
            if view_environment_type(&state.name).is_some() {
                diagnostics.push(diag(
                    state.name_span,
                    &format!(
                        "view state '{}' conflicts with a read-only view environment binding",
                        state.name
                    ),
                ));
            }
            if let Err(diagnostic) = require_known_type(state.type_span, &state.ty, signatures) {
                diagnostics.push(diagnostic);
                continue;
            }
            let expected = signatures.canonical_type(&state.ty);
            if expected == Type::Void || matches!(expected, Type::Function { .. }) {
                diagnostics.push(diag(
                    state.type_span,
                    "view state requires a concrete non-void value type",
                ));
                continue;
            }
            if !matches!(expected, Type::Bool | Type::I64 | Type::Str) {
                diagnostics.push(
                    diag(
                        state.type_span,
                        "bootstrap view state currently supports only copyable bool, i64, and borrowed str values",
                    )
                    .with_note(
                        "owned aggregates, optionals, resources, and other state require explicit ownership/lifetime storage rules before they can persist in a native view",
                    ),
                );
                continue;
            }
            match evaluate_default_expr(&state.initial, signatures) {
                Ok(value) => {
                    if let Err(diagnostic) = require_type(
                        state.initial.span,
                        &expected,
                        &value.ty(),
                        &format!("initial value for view state '{}'", state.name),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
                Err(mut diagnostic) => {
                    diagnostic.message = format!(
                        "view state '{}' initial value must be compile-time: {}",
                        state.name, diagnostic.message
                    );
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    validate_view_composition_cycles(&view_defs, diagnostics);
    validate_application_shortcut_conflicts(program, signatures, diagnostics);

    for view in &program.views {
        let row_count = view.grid.rows.len() as u64;
        let column_count = view.grid.columns.len() as u64;
        let mut property_env = VIEW_ENVIRONMENT_BINDINGS
            .iter()
            .map(|(name, ty)| ((*name).to_string(), ty.clone()))
            .collect::<HashMap<_, _>>();
        for (name, ty) in VIEW_ENVIRONMENT_BINDINGS {
            property_env.insert(source_name_to_internal(name), ty.clone());
        }
        for (name, _) in SEMANTIC_UI_I64_TOKENS {
            property_env.insert((*name).to_string(), Type::I64);
            property_env.insert(source_name_to_internal(name), Type::I64);
        }
        property_env.extend(
            view.params
                .iter()
                .map(|param| (param.name.clone(), signatures.canonical_type(&param.ty))),
        );
        for state in &view.states {
            property_env.insert(state.name.clone(), signatures.canonical_type(&state.ty));
        }
        for derived in &view.derived {
            if view_environment_type(&derived.name).is_some() {
                diagnostics.push(diag(
                    derived.name_span,
                    &format!(
                        "derived view value '{}' conflicts with a read-only view environment binding",
                        derived.name
                    ),
                ));
            }
            let expected = match require_known_type(derived.type_span, &derived.ty, signatures) {
                Ok(()) => signatures.canonical_type(&derived.ty),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    continue;
                }
            };
            if !matches!(expected, Type::I64 | Type::Bool | Type::Str) {
                diagnostics.push(diag(
                    derived.type_span,
                    "derived view values currently support i64, bool, and str",
                ));
                property_env.insert(derived.name.clone(), expected);
                continue;
            }
            match type_of_expr(&derived.value, &property_env, signatures) {
                Ok(actual) => {
                    if let Err(diagnostic) = require_type(
                        derived.value.span,
                        &expected,
                        &actual,
                        &format!("derived view value '{}'", derived.name),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                }
                Err(mut diagnostic) => {
                    diagnostic.message = format!(
                        "derived view value '{}': {}",
                        derived.name, diagnostic.message
                    );
                    diagnostics.push(diagnostic);
                }
            }
            property_env.insert(derived.name.clone(), expected);
        }

        for (index, element) in view.elements.iter().enumerate() {
            let custom_view = view_defs.get(element.kind.as_str()).copied();
            if custom_view.is_none() && !view_element_kind_is_builtin(&element.kind) {
                diagnostics.push(
                    diag(
                        element.kind_span,
                        &format!("unknown view element type '{}'", element.kind),
                    )
                    .with_note("element types are built-ins or declared Flux views"),
                );
            }
            if let Some(target) = custom_view
                && let Err(diagnostic) = require_visible_declaration(
                    element.kind_span,
                    target.name_span,
                    target.public,
                    "view",
                    &target.name,
                    signatures,
                )
            {
                diagnostics.push(diagnostic);
            }

            let long_press = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "on_long_press");
            let context_menu = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "on_context_menu");
            if let (Some(long_press), Some(context_menu)) = (long_press, context_menu) {
                diagnostics.push(
                    diag(
                        context_menu.name_span,
                        "onContextMenu cannot be combined with onLongPress on the same element",
                    )
                    .with_label(long_press.name_span, "onLongPress is declared here")
                    .with_note(
                        "Android uses the native long-click gesture for context-menu requests; choose one semantic action for that gesture",
                    ),
                );
            }
            let context_menu_label = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "context_menu_label");
            let context_menu_select = element.properties.iter().find(|property| {
                source_name_to_internal(&property.name) == "on_context_menu_select"
            });
            if context_menu.is_none()
                && let (Some(long_press), Some(label)) = (long_press, context_menu_label)
            {
                diagnostics.push(
                    diag(
                        label.name_span,
                        "contextMenuLabel cannot be combined with onLongPress on the same element",
                    )
                    .with_label(long_press.name_span, "onLongPress is declared here")
                    .with_note(
                        "Android uses the native long-click gesture to present the context menu; choose one semantic action for that gesture",
                    ),
                );
            }
            match (context_menu_label, context_menu_select) {
                (Some(label), None) => diagnostics.push(
                    diag(
                        label.name_span,
                        "contextMenuLabel requires onContextMenuSelect on the same element",
                    )
                    .with_note(
                        "the label describes the native menu action selected by that callback",
                    ),
                ),
                (None, Some(select)) => diagnostics.push(
                    diag(
                        select.name_span,
                        "onContextMenuSelect requires contextMenuLabel on the same element",
                    )
                    .with_note("declare the visible native menu action label explicitly"),
                ),
                (Some(label), Some(_)) => match evaluate_default_expr(&label.value, signatures) {
                    Ok(ConstantValue::Str(value)) if !value.is_empty() => {}
                    Ok(ConstantValue::Str(_)) => diagnostics
                        .push(diag(label.value.span, "contextMenuLabel must not be empty")),
                    Ok(_) => {}
                    Err(_) => diagnostics.push(diag(
                        label.value.span,
                        "contextMenuLabel must be a compile-time string value",
                    )),
                },
                (None, None) => {}
            }

            let context_menu_items = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "context_menu_items");
            let context_menu_item_select = element.properties.iter().find(|property| {
                source_name_to_internal(&property.name) == "on_context_menu_item_select"
            });
            match (context_menu_items, context_menu_item_select) {
                (Some(items), None) => diagnostics.push(
                    diag(
                        items.name_span,
                        "contextMenuItems requires onContextMenuItemSelect on the same element",
                    )
                    .with_note("the callback receives the zero-based index of the selected native menu item"),
                ),
                (None, Some(select)) => diagnostics.push(
                    diag(
                        select.name_span,
                        "onContextMenuItemSelect requires contextMenuItems on the same element",
                    )
                    .with_note("declare the native menu labels as a compile-time string list"),
                ),
                (Some(items), Some(_)) => match &items.value.kind {
                    ExprKind::List(values) if values.is_empty() => diagnostics.push(diag(
                        items.value.span,
                        "contextMenuItems must contain at least one menu label",
                    )),
                    ExprKind::List(values) => {
                        for value in values {
                            match evaluate_default_expr(value, signatures) {
                                Ok(ConstantValue::Str(label)) if !label.is_empty() => {}
                                Ok(ConstantValue::Str(_)) => diagnostics.push(diag(
                                    value.span,
                                    "contextMenuItems labels must not be empty",
                                )),
                                Ok(_) | Err(_) => diagnostics.push(diag(
                                    value.span,
                                    "contextMenuItems must be a compile-time list of string values",
                                )),
                            }
                        }
                    }
                    _ => diagnostics.push(diag(
                        items.value.span,
                        "contextMenuItems must be a compile-time list literal of string values",
                    )),
                },
                (None, None) => {}
            }
            if context_menu_items.is_some() && context_menu_label.is_some() {
                diagnostics.push(
                    diag(
                        context_menu_items.unwrap().name_span,
                        "contextMenuItems cannot be combined with contextMenuLabel on the same element",
                    )
                    .with_label(
                        context_menu_label.unwrap().name_span,
                        "single-action context menu is declared here",
                    ),
                );
            }
            if let (Some(long_press), Some(items)) = (long_press, context_menu_items) {
                diagnostics.push(
                    diag(
                        items.name_span,
                        "contextMenuItems cannot be combined with onLongPress on the same element",
                    )
                    .with_label(long_press.name_span, "onLongPress is declared here")
                    .with_note(
                        "Android uses the native long-click gesture to present the context menu; choose one semantic action for that gesture",
                    ),
                );
            }

            let drag_text = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "drag_text");
            if let Some(drag_text) = drag_text {
                match evaluate_default_expr(&drag_text.value, signatures) {
                    Ok(ConstantValue::Str(value)) if !value.is_empty() => {}
                    Ok(ConstantValue::Str(_)) => {
                        diagnostics.push(diag(drag_text.value.span, "dragText must not be empty"))
                    }
                    Ok(_) => {}
                    Err(_) => diagnostics.push(diag(
                        drag_text.value.span,
                        "dragText must be a compile-time string value",
                    )),
                }
                if let Some(conflict) = long_press
                    .or(context_menu)
                    .or(context_menu_label)
                    .or(context_menu_items)
                {
                    diagnostics.push(
                        diag(
                            drag_text.name_span,
                            "dragText cannot be combined with long-press/context-menu activation on the same element",
                        )
                        .with_label(conflict.name_span, "conflicting long-press gesture is declared here")
                        .with_note(
                            "Android starts native drag-and-drop from the long-click gesture, so one element cannot claim that gesture for both actions",
                        ),
                    );
                }
            }

            for property in &element.properties {
                let internal_property = source_name_to_internal(&property.name);
                if internal_property == "accessibility_role" {
                    match evaluate_default_expr(&property.value, signatures) {
                        Ok(ConstantValue::Str(role))
                            if ACCESSIBILITY_ROLES.contains(&role.as_str()) => {}
                        Ok(ConstantValue::Str(role)) => diagnostics.push(
                            diag(
                                property.value.span,
                                &format!("unsupported accessibilityRole '{role}'"),
                            )
                            .with_note(format!(
                                "supported semantic roles: {}",
                                ACCESSIBILITY_ROLES.join(", ")
                            )),
                        ),
                        Ok(_) => {}
                        Err(_) => diagnostics.push(diag(
                            property.value.span,
                            "accessibilityRole must be a compile-time string value",
                        )),
                    }
                }
                if internal_property == "shortcut_scope" {
                    match evaluate_default_expr(&property.value, signatures) {
                        Ok(ConstantValue::Str(scope))
                            if SHORTCUT_SCOPES.contains(&scope.as_str()) => {}
                        Ok(ConstantValue::Str(scope)) => diagnostics.push(
                            diag(
                                property.value.span,
                                &format!("unsupported shortcutScope '{scope}'"),
                            )
                            .with_note(format!(
                                "supported shortcut scopes: {}",
                                SHORTCUT_SCOPES.join(", ")
                            )),
                        ),
                        Ok(_) => {}
                        Err(_) => diagnostics.push(diag(
                            property.value.span,
                            "shortcutScope must be a compile-time string value",
                        )),
                    }
                }
                if internal_property == "status" {
                    match evaluate_default_expr(&property.value, signatures) {
                        Ok(ConstantValue::Str(state))
                            if UI_PRESENTATION_STATES.contains(&state.as_str()) => {}
                        Ok(ConstantValue::Str(state)) => diagnostics.push(
                            diag(
                                property.value.span,
                                &format!("unsupported UI status '{state}'"),
                            )
                            .with_note(format!(
                                "supported UI statuses: {}",
                                UI_PRESENTATION_STATES.join(", ")
                            )),
                        ),
                        Ok(_) => {}
                        Err(_) => diagnostics.push(diag(
                            property.value.span,
                            "status must be a compile-time string value",
                        )),
                    }
                }
                if element.kind == "TextInput" && internal_property == "validation_state" {
                    match evaluate_default_expr(&property.value, signatures) {
                        Ok(ConstantValue::Str(state))
                            if TEXT_INPUT_VALIDATION_STATES.contains(&state.as_str()) => {}
                        Ok(ConstantValue::Str(state)) => diagnostics.push(
                            diag(
                                property.value.span,
                                &format!("unsupported TextInput.validationState '{state}'"),
                            )
                            .with_note(format!(
                                "supported validation states: {}",
                                TEXT_INPUT_VALIDATION_STATES.join(", ")
                            )),
                        ),
                        Ok(_) => {}
                        Err(_) => diagnostics.push(diag(
                            property.value.span,
                            "TextInput.validationState must be a compile-time string value",
                        )),
                    }
                }
                if internal_property == "transition_easing" {
                    match evaluate_default_expr(&property.value, signatures) {
                        Ok(ConstantValue::Str(easing))
                            if transition_easing_css_value(&easing).is_some() => {}
                        Ok(ConstantValue::Str(easing)) => diagnostics.push(
                            diag(
                                property.value.span,
                                &format!("unsupported transitionEasing '{easing}'"),
                            )
                            .with_note(format!(
                                "supported transition easings: {}",
                                TRANSITION_EASINGS.join(", ")
                            )),
                        ),
                        Ok(_) => {}
                        Err(_) => diagnostics.push(diag(
                            property.value.span,
                            "transitionEasing must be a compile-time string value",
                        )),
                    }
                }
                if let Some(transition) = &property.transition {
                    let internal_property = source_name_to_internal(&property.name);
                    let transition_property = matches!(
                        (element.kind.as_str(), internal_property.as_str()),
                        ("Button", "on_press")
                            | ("Toggle", "on_change")
                            | ("Radio", "on_select")
                            | (_, "on_tap")
                            | (_, "on_double_tap")
                            | (_, "on_long_press")
                            | (_, "on_context_menu")
                            | (_, "on_context_menu_select")
                            | (_, "on_hover")
                            | (_, "on_leave")
                            | (_, "on_focus")
                            | (_, "on_blur")
                    );
                    if !transition_property {
                        diagnostics.push(diag(
                            property.span,
                            "view state transitions are valid only for event properties such as Button.onPress, Toggle.onChange, Radio.onSelect, onTap/onDoubleTap/onLongPress/onContextMenu/onContextMenuSelect, onHover/onLeave, or onFocus/onBlur",
                        ));
                        continue;
                    }
                    let Some(state) = view
                        .states
                        .iter()
                        .find(|state| state.name == transition.state)
                    else {
                        diagnostics.push(diag(
                            transition.state_span,
                            &format!("unknown view state '{}'", transition.state),
                        ));
                        continue;
                    };
                    match type_of_expr(&property.value, &property_env, signatures) {
                        Ok(actual) => {
                            let expected = signatures.canonical_type(&state.ty);
                            if let Err(diagnostic) = require_type(
                                property.value.span,
                                &expected,
                                &actual,
                                &format!("next value for view state '{}'", state.name),
                            ) {
                                diagnostics.push(diagnostic);
                            }
                        }
                        Err(diagnostic) => diagnostics.push(diagnostic),
                    }
                    continue;
                }

                let Some(expected) =
                    view_element_property_type(program, signatures, &element.kind, &property.name)
                else {
                    if view_element_kind_is_builtin(&element.kind) {
                        diagnostics.push(
                            diag(
                                property.name_span,
                                &format!(
                                    "view element type '{}' has no property '{}'",
                                    element.kind, property.name
                                ),
                            )
                            .with_note(format!(
                                "supported properties for '{}': {}",
                                element.kind,
                                view_property_names(&element.kind).join(", ")
                            )),
                        );
                    } else if let Some(target) = custom_view {
                        diagnostics.push(
                            diag(
                                property.name_span,
                                &format!(
                                    "view '{}' has no parameter '{}'",
                                    target.name, property.name
                                ),
                            )
                            .with_label(target.name_span, "view is declared here"),
                        );
                    }
                    continue;
                };
                match type_of_expr(&property.value, &property_env, signatures) {
                    Ok(actual) => {
                        if let Err(diagnostic) = require_type(
                            property.value.span,
                            &expected,
                            &actual,
                            &format!("property '{}.{}'", element.kind, property.name),
                        ) {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
            }

            let has_shortcut = element
                .properties
                .iter()
                .any(|property| source_name_to_internal(&property.name) == "shortcut");
            let has_shortcut_scope = element
                .properties
                .iter()
                .any(|property| source_name_to_internal(&property.name) == "shortcut_scope");
            if has_shortcut_scope && !has_shortcut {
                diagnostics.push(diag(
                    element.span,
                    &format!("{}.shortcutScope requires shortcut", element.kind),
                ));
            }
            if has_shortcut {
                let has_press = element.kind == "Button"
                    && element
                        .properties
                        .iter()
                        .any(|property| source_name_to_internal(&property.name) == "on_press");
                let has_tap = element
                    .properties
                    .iter()
                    .any(|property| source_name_to_internal(&property.name) == "on_tap");
                if !has_press && !has_tap {
                    diagnostics.push(diag(
                        element.span,
                        &format!(
                            "{}.shortcut requires {} so the shortcut has a typed Flux action",
                            element.kind,
                            if element.kind == "Button" {
                                "Button.onPress or onTap"
                            } else {
                                "onTap"
                            }
                        ),
                    ));
                }
            }

            if let Some(target) = custom_view {
                for param in target.params.iter().filter(|param| param.default.is_none()) {
                    if !element
                        .properties
                        .iter()
                        .any(|property| property.name == param.name)
                    {
                        diagnostics.push(
                            diag(
                                element.span,
                                &format!(
                                    "view element '{}' is missing required parameter '{}' for '{}'",
                                    element.name, param.name, target.name
                                ),
                            )
                            .with_label(param.name_span, "required view parameter declared here"),
                        );
                    }
                }
            }

            let row_start = u64::from(element.row);
            let column_start = u64::from(element.column);
            let row_end = row_start + u64::from(element.row_span) - 1;
            let column_end = column_start + u64::from(element.column_span) - 1;

            if row_end > row_count {
                diagnostics.push(
                    diag(
                        element.span,
                        &format!(
                            "view element '{}' occupies grid row {row_end}, but view '{}' declares only {row_count} row{}",
                            element.name,
                            view.name,
                            if row_count == 1 { "" } else { "s" }
                        ),
                    )
                    .with_label(view.name_span, "grid is declared by this view"),
                );
            }
            if column_end > column_count {
                diagnostics.push(
                    diag(
                        element.span,
                        &format!(
                            "view element '{}' occupies grid column {column_end}, but view '{}' declares only {column_count} column{}",
                            element.name,
                            view.name,
                            if column_count == 1 { "" } else { "s" }
                        ),
                    )
                    .with_label(view.name_span, "grid is declared by this view"),
                );
            }

            if view.grid.overlay != Some(true) {
                for previous in &view.elements[..index] {
                    if grid_elements_overlap(previous, element) {
                        diagnostics.push(
                            diag(
                                element.span,
                                &format!(
                                    "view element '{}' overlaps sibling '{}' in grid '{}'",
                                    element.name, previous.name, view.name
                                ),
                            )
                            .with_label(
                                previous.span,
                                format!("'{}' already occupies this grid area", previous.name),
                            )
                            .with_note("grid siblings may not overlap by default; declare 'grid overlay: true' when intentional overlap is part of the flat layout"),
                        );
                    }
                }
            }
        }

        let mut autofocus_target = None::<(&str, SourceSpan)>;
        for element in &view.elements {
            if !view_element_kind_is_builtin(&element.kind) {
                continue;
            }
            let Some(property) = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "autofocus")
            else {
                continue;
            };
            match evaluate_default_expr(&property.value, signatures) {
                Ok(ConstantValue::Bool(false)) => {}
                Ok(ConstantValue::Bool(true)) => {
                    if let Some(focusable) = element
                        .properties
                        .iter()
                        .find(|candidate| source_name_to_internal(&candidate.name) == "focusable")
                    {
                        match evaluate_default_expr(&focusable.value, signatures) {
                            Ok(ConstantValue::Bool(true)) => {}
                            Ok(ConstantValue::Bool(false)) => diagnostics.push(diag(
                                focusable.value.span,
                                "autofocus: true requires focusable: true when focusable is specified explicitly",
                            )),
                            Ok(_) => {}
                            Err(_) => diagnostics.push(diag(
                                focusable.value.span,
                                "focusable must be a compile-time bool when autofocus is enabled",
                            )),
                        }
                    }
                    if let Some((previous_name, previous_span)) = autofocus_target {
                        diagnostics.push(
                            diag(
                                property.value.span,
                                &format!(
                                    "view '{}' has more than one autofocus target; '{}' is already selected",
                                    view.name, previous_name
                                ),
                            )
                            .with_label(previous_span, "first autofocus target"),
                        );
                    } else {
                        autofocus_target = Some((&element.name, property.value.span));
                    }
                }
                Ok(_) => {}
                Err(_) => diagnostics.push(diag(
                    property.value.span,
                    "autofocus must be a compile-time bool value",
                )),
            }
        }

        for element in &view.elements {
            let Some(property) = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "focus_scope")
            else {
                continue;
            };
            match evaluate_default_expr(&property.value, signatures) {
                Ok(ConstantValue::I64(scope)) if (0..=i64::from(i32::MAX)).contains(&scope) => {}
                Ok(ConstantValue::I64(_)) => diagnostics.push(diag(
                    property.value.span,
                    "focusScope must be between 0 and 2147483647",
                )),
                Ok(_) => {}
                Err(_) => diagnostics.push(diag(
                    property.value.span,
                    "focusScope must be a compile-time i64 value",
                )),
            }
        }

        let mut accessibility_orders = HashMap::<i64, (&str, SourceSpan)>::new();
        for element in &view.elements {
            let Some(property) = element
                .properties
                .iter()
                .find(|property| source_name_to_internal(&property.name) == "accessibility_order")
            else {
                continue;
            };
            match evaluate_default_expr(&property.value, signatures) {
                Ok(ConstantValue::I64(order)) if order >= 0 => {
                    if let Some((previous_name, previous_span)) =
                        accessibility_orders.insert(order, (&element.name, property.value.span))
                    {
                        diagnostics.push(
                            diag(
                                property.value.span,
                                &format!(
                                    "accessibilityOrder {order} is already used by view element '{previous_name}'"
                                ),
                            )
                            .with_label(previous_span, "first use of this accessibility order"),
                        );
                    }
                }
                Ok(ConstantValue::I64(_)) => diagnostics.push(diag(
                    property.value.span,
                    "accessibilityOrder must be non-negative",
                )),
                Ok(_) => {}
                Err(_) => diagnostics.push(diag(
                    property.value.span,
                    "accessibilityOrder must be a compile-time i64 value",
                )),
            }
        }
    }
}

pub(crate) fn parse_ui_shortcut(value: &str) -> Option<(bool, bool, bool, String)> {
    let parts = value.split('+').map(str::trim).collect::<Vec<_>>();
    let (key, modifiers) = parts.split_last()?;
    if key.is_empty() || modifiers.is_empty() {
        return None;
    }
    let mut control = false;
    let mut shift = false;
    let mut alt = false;
    for modifier in modifiers {
        match *modifier {
            "Ctrl" if !control => control = true,
            "Shift" if !shift => shift = true,
            "Alt" if !alt => alt = true,
            _ => return None,
        }
    }
    let key = match *key {
        "Enter" | "Space" | "Tab" | "Escape" | "Delete" | "Up" | "Down" | "Left" | "Right" => {
            key.to_string()
        }
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => {
            key.to_ascii_uppercase()
        }
        _ => return None,
    };
    Some((control, shift, alt, key))
}

fn canonical_ui_shortcut(value: &str) -> Option<String> {
    let (control, shift, alt, key) = parse_ui_shortcut(value)?;
    let mut canonical = String::new();
    if control {
        canonical.push_str("Ctrl+");
    }
    if shift {
        canonical.push_str("Shift+");
    }
    if alt {
        canonical.push_str("Alt+");
    }
    canonical.push_str(&key);
    Some(canonical)
}

fn validate_application_shortcut_conflicts(
    program: &Program,
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(application) = &program.application else {
        return;
    };
    let view_defs = program
        .views
        .iter()
        .map(|view| (view.name.as_str(), view))
        .collect::<HashMap<_, _>>();
    let Some(root) = view_defs.get(application.view_name.as_str()).copied() else {
        return;
    };
    let mut shortcuts = HashMap::<String, (SourceSpan, String)>::new();
    let mut stack = Vec::<String>::new();
    collect_view_shortcuts(
        root,
        root.name.clone(),
        &view_defs,
        signatures,
        &mut shortcuts,
        &mut stack,
        diagnostics,
    );
}

fn collect_view_shortcuts(
    view: &crate::ast::ViewDef,
    path: String,
    view_defs: &HashMap<&str, &crate::ast::ViewDef>,
    signatures: &Signatures,
    shortcuts: &mut HashMap<String, (SourceSpan, String)>,
    stack: &mut Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if stack.iter().any(|name| name == &view.name) {
        return;
    }
    stack.push(view.name.clone());
    for element in &view.elements {
        let element_path = format!("{path}.{}", element.name);
        if let Some(property) = element
            .properties
            .iter()
            .find(|property| source_name_to_internal(&property.name) == "shortcut")
            && let Ok(ConstantValue::Str(value)) =
                evaluate_default_expr(&property.value, signatures)
            && let Some(shortcut) = canonical_ui_shortcut(&value)
        {
            if let Some((previous_span, previous_path)) = shortcuts.get(&shortcut) {
                diagnostics.push(
                    diag(
                        property.value.span,
                        &format!(
                            "keyboard shortcut '{shortcut}' conflicts with another action in the app window"
                        ),
                    )
                    .with_label(*previous_span, format!("first used by '{previous_path}'"))
                    .with_note(format!("conflicting action: '{element_path}'")),
                );
            } else {
                shortcuts.insert(shortcut, (property.value.span, element_path.clone()));
            }
        }
        if let Some(child) = view_defs.get(element.kind.as_str()).copied() {
            collect_view_shortcuts(
                child,
                element_path,
                view_defs,
                signatures,
                shortcuts,
                stack,
                diagnostics,
            );
        }
    }
    stack.pop();
}

fn validate_view_composition_cycles(
    view_defs: &HashMap<&str, &crate::ast::ViewDef>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut finished = HashSet::new();
    for name in view_defs.keys().copied() {
        let mut stack = Vec::new();
        visit_view_composition(name, view_defs, &mut finished, &mut stack, diagnostics);
    }
}

fn visit_view_composition(
    name: &str,
    view_defs: &HashMap<&str, &crate::ast::ViewDef>,
    finished: &mut HashSet<String>,
    stack: &mut Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if finished.contains(name) {
        return;
    }
    let Some(view) = view_defs.get(name).copied() else {
        return;
    };
    stack.push(name.to_string());
    for element in &view.elements {
        if !view_defs.contains_key(element.kind.as_str()) {
            continue;
        }
        if let Some(start) = stack.iter().position(|entry| entry == &element.kind) {
            let mut cycle = stack[start..].to_vec();
            cycle.push(element.kind.clone());
            diagnostics.push(
                diag(element.kind_span, "cyclic view composition detected")
                    .with_note(format!("view cycle: {}", cycle.join(" -> "))),
            );
            continue;
        }
        visit_view_composition(&element.kind, view_defs, finished, stack, diagnostics);
    }
    stack.pop();
    finished.insert(name.to_string());
}

fn grid_elements_overlap(left: &crate::ast::ViewElement, right: &crate::ast::ViewElement) -> bool {
    let left_row_end = u64::from(left.row) + u64::from(left.row_span) - 1;
    let right_row_end = u64::from(right.row) + u64::from(right.row_span) - 1;
    let left_column_end = u64::from(left.column) + u64::from(left.column_span) - 1;
    let right_column_end = u64::from(right.column) + u64::from(right.column_span) - 1;

    u64::from(left.row) <= right_row_end
        && u64::from(right.row) <= left_row_end
        && u64::from(left.column) <= right_column_end
        && u64::from(right.column) <= left_column_end
}

fn check_function_all(
    function: &Function,
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut env = HashMap::new();
    let mut mutable = HashSet::new();
    for param in &function.params {
        env.insert(param.name.clone(), signatures.canonical_type(&param.ty));
    }
    let return_types = function
        .returns
        .iter()
        .map(|ty| signatures.canonical_type(ty))
        .collect::<Vec<_>>();

    let diagnostics_before_body = diagnostics.len();
    check_block_all(
        &function.body,
        &mut env,
        &mut mutable,
        &return_types,
        signatures,
        diagnostics,
        0,
    );
    if diagnostics.len() == diagnostics_before_body {
        check_cfg_moved_reads(function, signatures, diagnostics);
    }
    if diagnostics.len() == diagnostics_before_body {
        check_unused_function_bindings(function, diagnostics);
    }

    if !function.returns.is_empty() && !block_guarantees_return(&function.body) {
        diagnostics.push(diag(
            function.span,
            &format!(
                "function '{}' can reach the end without returning {}",
                function.name,
                return_types_name(&function.returns)
            ),
        ));
    }
}

fn check_unused_function_bindings(function: &Function, diagnostics: &mut Vec<Diagnostic>) {
    let mut declarations = function
        .params
        .iter()
        .map(|param| (param.name.clone(), param.name_span, "parameter"))
        .collect::<Vec<_>>();
    collect_binding_declarations(&function.body, &mut declarations);
    let mut reads = HashSet::new();
    collect_block_reads(&function.body, &mut reads);
    for (name, span, kind) in declarations {
        if name.starts_with('_') || reads.contains(&name) {
            continue;
        }
        diagnostics.push(
            diag(span, &format!("unused {kind} '{name}'"))
                .with_note("Flux has no lint-warning tier: unused bindings are compile errors; prefix an intentionally ignored binding with '_'"),
        );
    }
}

fn collect_binding_declarations(
    body: &[Stmt],
    declarations: &mut Vec<(String, SourceSpan, &'static str)>,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let {
                name,
                name_span,
                expr,
                ..
            } => {
                declarations.push((name.clone(), *name_span, "binding"));
                collect_expr_pattern_declarations(expr, declarations);
            }
            StmtKind::Var {
                name,
                name_span,
                expr,
                ..
            } => {
                declarations.push((name.clone(), *name_span, "mutable binding"));
                collect_expr_pattern_declarations(expr, declarations);
            }
            StmtKind::LetDestructure { bindings, .. } => {
                for binding in bindings {
                    declarations.push((binding.name.clone(), binding.name_span, "binding"));
                }
            }
            StmtKind::LetMultiDestructure { bindings, .. } => {
                for binding in bindings {
                    if binding.name != "_" {
                        declarations.push((
                            binding.name.clone(),
                            binding.span,
                            "destructured binding",
                        ));
                    }
                }
            }
            StmtKind::LetListDestructure { bindings, rest, .. } => {
                for binding in bindings {
                    if binding.name != "_" {
                        declarations.push((
                            binding.name.clone(),
                            binding.span,
                            "destructured binding",
                        ));
                    }
                }
                if let Some(rest) = rest
                    && rest.binding.name != "_"
                {
                    declarations.push((
                        rest.binding.name.clone(),
                        rest.binding.span,
                        "destructured binding",
                    ));
                }
            }
            StmtKind::LetStructDestructure { fields, .. } => {
                collect_struct_pattern_declarations(fields, declarations);
            }
            StmtKind::ForRange {
                name,
                name_span,
                body,
                ..
            } => {
                declarations.push((name.clone(), *name_span, "loop variable"));
                collect_binding_declarations(body, declarations);
            }
            StmtKind::ForEach {
                index_name,
                index_span,
                name,
                name_span,
                body,
                ..
            } => {
                if let (Some(index_name), Some(index_span)) = (index_name, index_span) {
                    declarations.push((index_name.clone(), *index_span, "loop index"));
                }
                declarations.push((name.clone(), *name_span, "loop variable"));
                collect_binding_declarations(body, declarations);
            }
            StmtKind::If {
                binding,
                body,
                else_body,
                ..
            } => {
                if let Some(binding) = binding
                    && binding.name != "_"
                {
                    declarations.push((
                        binding.name.clone(),
                        binding.span,
                        "optional pattern binding",
                    ));
                }
                collect_binding_declarations(body, declarations);
                collect_binding_declarations(else_body, declarations);
            }
            StmtKind::While { body, .. } => collect_binding_declarations(body, declarations),
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    for pattern in &arm.patterns {
                        match pattern {
                            MatchPattern::Binding(binding) if binding.name != "_" => declarations
                                .push((binding.name.clone(), binding.span, "match binding")),
                            MatchPattern::Struct(pattern) => {
                                collect_struct_pattern_declarations(&pattern.fields, declarations);
                            }
                            MatchPattern::Binding(_)
                            | MatchPattern::Relational(_)
                            | MatchPattern::Logical { .. } => {}
                        }
                    }
                    collect_binding_declarations(&arm.body, declarations);
                }
            }
            StmtKind::ListMatch { arms, .. } => {
                for arm in arms {
                    collect_list_match_pattern_declarations(&arm.pattern, declarations);
                    collect_binding_declarations(&arm.body, declarations);
                }
            }
            StmtKind::Return(values) => {
                for value in values {
                    collect_expr_pattern_declarations(value, declarations);
                }
            }
            StmtKind::Assign { .. }
            | StmtKind::AssignMultiDestructure { .. }
            | StmtKind::AssignListDestructure { .. }
            | StmtKind::AssignStructDestructure { .. }
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Expr(_)
            | StmtKind::Shell { .. } => {}
        }
    }
}

fn collect_expr_pattern_declarations(
    expr: &Expr,
    declarations: &mut Vec<(String, SourceSpan, &'static str)>,
) {
    match &expr.kind {
        ExprKind::Match { arms, .. } => {
            for arm in arms {
                for pattern in &arm.patterns {
                    match pattern {
                        MatchPattern::Binding(binding) if binding.name != "_" => {
                            declarations.push((binding.name.clone(), binding.span, "match binding"))
                        }
                        MatchPattern::Struct(pattern) => {
                            collect_struct_pattern_declarations(&pattern.fields, declarations);
                        }
                        MatchPattern::Binding(_)
                        | MatchPattern::Relational(_)
                        | MatchPattern::Logical { .. } => {}
                    }
                }
                collect_expr_pattern_declarations(&arm.value, declarations);
            }
        }
        ExprKind::ListMatch { arms, .. } => {
            for arm in arms {
                collect_list_match_pattern_declarations(&arm.pattern, declarations);
                collect_expr_pattern_declarations(&arm.value, declarations);
            }
        }
        _ => {}
    }
}

fn collect_list_match_pattern_declarations(
    pattern: &ListMatchPattern,
    declarations: &mut Vec<(String, SourceSpan, &'static str)>,
) {
    let ListMatchPattern::List { bindings, rest, .. } = pattern else {
        return;
    };
    for binding in bindings {
        if binding.name != "_" {
            declarations.push((binding.name.clone(), binding.span, "match binding"));
        }
    }
    if let Some(rest) = rest
        && rest.binding.name != "_"
    {
        declarations.push((
            rest.binding.name.clone(),
            rest.binding.span,
            "match binding",
        ));
    }
}

fn collect_struct_pattern_declarations(
    fields: &[crate::ast::StructPatternField],
    declarations: &mut Vec<(String, SourceSpan, &'static str)>,
) {
    for field in fields {
        if field.binding.name != "_" {
            declarations.push((
                field.binding.name.clone(),
                field.binding.span,
                "destructured binding",
            ));
        }
        if let Some(nested) = &field.nested {
            collect_struct_pattern_declarations(&nested.fields, declarations);
        }
    }
}

fn collect_block_reads(body: &[Stmt], reads: &mut HashSet<String>) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let { expr, .. }
            | StmtKind::Var { expr, .. }
            | StmtKind::Assign { expr, .. }
            | StmtKind::AssignMultiDestructure { expr, .. }
            | StmtKind::AssignListDestructure { expr, .. }
            | StmtKind::AssignStructDestructure { expr, .. }
            | StmtKind::LetListDestructure { expr, .. }
            | StmtKind::LetStructDestructure { expr, .. } => collect_expr_reads(expr, reads),
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
                ..
            } => {
                collect_expr_reads(expr, reads);
                if *else_return && let Some(binding) = bindings.last() {
                    reads.insert(binding.name.clone());
                }
            }
            StmtKind::LetMultiDestructure {
                bindings,
                expr,
                else_return,
                ..
            } => {
                collect_expr_reads(expr, reads);
                if *else_return
                    && let Some(binding) = bindings.last()
                    && binding.name != "_"
                {
                    reads.insert(binding.name.clone());
                }
            }
            StmtKind::Return(values) => {
                for value in values {
                    collect_expr_reads(value, reads);
                }
            }
            StmtKind::Expr(expr) => collect_expr_reads(expr, reads),
            StmtKind::Shell { expr, redirect, .. } => {
                collect_expr_reads(expr, reads);
                if let Some(redirect) = redirect {
                    collect_expr_reads(&redirect.path, reads);
                }
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
                collect_expr_reads(cond, reads);
                collect_block_reads(body, reads);
                collect_block_reads(else_body, reads);
            }
            StmtKind::ForRange {
                start, end, body, ..
            } => {
                collect_expr_reads(start, reads);
                collect_expr_reads(end, reads);
                collect_block_reads(body, reads);
            }
            StmtKind::ForEach { iterable, body, .. } => {
                collect_expr_reads(iterable, reads);
                collect_block_reads(body, reads);
            }
            StmtKind::While { cond, body } => {
                collect_expr_reads(cond, reads);
                collect_block_reads(body, reads);
            }
            StmtKind::Match { value, arms } => {
                collect_expr_reads(value, reads);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        collect_expr_reads(guard, reads);
                    }
                    collect_block_reads(&arm.body, reads);
                }
            }
            StmtKind::ListMatch { value, arms } => {
                collect_expr_reads(value, reads);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        collect_expr_reads(guard, reads);
                    }
                    collect_block_reads(&arm.body, reads);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
    }
}

fn collect_inline_sequence_callback_reads(expr: &Expr, reads: &mut HashSet<String>) {
    if let ExprKind::AnonymousFunction { params, body, .. } = &expr.kind {
        let mut nested = HashSet::new();
        collect_expr_reads(body, &mut nested);
        for param in params {
            nested.remove(&param.name);
        }
        reads.extend(nested);
    } else {
        collect_expr_reads(expr, reads);
    }
}

pub(crate) fn collect_expr_reads(expr: &Expr, reads: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Var(name) => {
            reads.insert(name.clone());
        }
        ExprKind::AnonymousFunction { .. } => {}
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
            reads.insert(name.clone());
            for (index, arg) in args.iter().enumerate() {
                if named_args.is_empty()
                    && matches!(name.as_str(), "map" | "filter" | "where")
                    && args.len() == 2
                    && index == 1
                {
                    collect_inline_sequence_callback_reads(arg, reads);
                } else {
                    collect_expr_reads(arg, reads);
                }
            }
            for arg in named_args {
                collect_expr_reads(&arg.value, reads);
            }
        }
        ExprKind::ShellCall { name, args, .. } => {
            reads.insert(name.clone());
            for arg in args {
                collect_expr_reads(arg, reads);
            }
        }
        ExprKind::Pipe {
            input, name, args, ..
        } => {
            collect_expr_reads(input, reads);
            reads.insert(name.clone());
            for (index, arg) in args.iter().enumerate() {
                if matches!(name.as_str(), "map" | "filter" | "where")
                    && args.len() == 1
                    && index == 0
                {
                    collect_inline_sequence_callback_reads(arg, reads);
                } else {
                    collect_expr_reads(arg, reads);
                }
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_expr_reads(item, reads);
            }
        }
        ExprKind::ListSpread { value, .. } | ExprKind::ListOptional { value, .. } => {
            collect_expr_reads(value, reads)
        }
        ExprKind::ListIf {
            condition,
            binding,
            value,
            else_value,
            ..
        } => {
            collect_expr_reads(condition, reads);
            let mut nested_reads = HashSet::new();
            collect_expr_reads(value, &mut nested_reads);
            if let Some(binding) = binding {
                nested_reads.remove(&binding.name);
            }
            reads.extend(nested_reads);
            if let Some(else_value) = else_value {
                collect_expr_reads(else_value, reads);
            }
        }
        ExprKind::Index { base, index } => {
            collect_expr_reads(base, reads);
            collect_expr_reads(index, reads);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_expr_reads(base, reads);
            if let Some(start) = start {
                collect_expr_reads(start, reads);
            }
            if let Some(end) = end {
                collect_expr_reads(end, reads);
            }
            if let Some(step) = step {
                collect_expr_reads(step, reads);
            }
        }
        ExprKind::ListComprehension {
            value,
            binding,
            iterable,
            condition,
            ..
        } => {
            collect_expr_reads(iterable, reads);
            let mut nested_reads = HashSet::new();
            collect_expr_reads(value, &mut nested_reads);
            if let Some(condition) = condition {
                collect_expr_reads(condition, &mut nested_reads);
            }
            nested_reads.remove(binding);
            reads.extend(nested_reads);
        }
        ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                collect_expr_reads(arg, reads);
            }
            for arg in named_args {
                collect_expr_reads(&arg.value, reads);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_expr_reads(base, reads);
            }
            for field in fields {
                collect_expr_reads(&field.value, reads);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_expr_reads(base, reads);
        }
        ExprKind::Match { value, arms } => {
            collect_expr_reads(value, reads);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_expr_reads(guard, reads);
                }
                collect_expr_reads(&arm.value, reads);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_expr_reads(value, reads);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_expr_reads(guard, reads);
                }
                collect_expr_reads(&arm.value, reads);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_expr_reads(then_expr, reads);
            collect_expr_reads(cond, reads);
            collect_expr_reads(else_expr, reads);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_expr_reads(left, reads);
            collect_expr_reads(right, reads);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::None => {}
    }
}

fn optional_presence_promotion(
    cond: &Expr,
    env: &HashMap<String, Type>,
    mutable: &HashSet<String>,
    signatures: &Signatures,
) -> Option<(String, Type, bool)> {
    let ExprKind::Binary { left, op, right } = &cond.kind else {
        return None;
    };
    if !matches!(op, BinOp::Eq | BinOp::Ne) {
        return None;
    }
    let name = match (&left.kind, &right.kind) {
        (ExprKind::Var(name), ExprKind::None) | (ExprKind::None, ExprKind::Var(name)) => name,
        _ => return None,
    };
    if mutable.contains(name) {
        return None;
    }
    let Type::Optional(inner) = signatures.canonical_type(env.get(name)?) else {
        return None;
    };
    if *inner == Type::Void {
        return None;
    }
    Some((
        name.clone(),
        signatures.canonical_type(&inner),
        matches!(op, BinOp::Ne),
    ))
}

fn check_block_all(
    body: &[Stmt],
    env: &mut HashMap<String, Type>,
    mutable: &mut HashSet<String>,
    return_types: &[Type],
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
    loop_depth: usize,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let {
                name,
                ty,
                type_span,
                expr,
                ..
            }
            | StmtKind::Var {
                name,
                ty,
                type_span,
                expr,
                ..
            } => {
                if let Err(diagnostic) = require_known_type(*type_span, ty, signatures) {
                    diagnostics.push(diagnostic);
                }
                if matches!(stmt.kind, StmtKind::Var { .. }) && !signatures.is_copy_type(ty) {
                    let message = if matches!(signatures.canonical_type(ty), Type::List(_)) {
                        "list bindings are currently immutable local values; use 'let' until collection ownership is implemented".to_string()
                    } else {
                        format!(
                            "non-copy value type '{}' cannot be mutable until move/borrow semantics are implemented",
                            signatures.canonical_type(ty).name()
                        )
                    };
                    diagnostics.push(diag(*type_span, &message));
                }
                let duplicate = env.contains_key(name);
                if duplicate {
                    diagnostics.push(diag(
                        stmt.span,
                        &format!("'{name}' is already defined in this scope"),
                    ));
                }
                match type_of_expr(expr, env, signatures) {
                    Ok(actual) => {
                        let declared = signatures.canonical_type(ty);
                        if let Err(diagnostic) =
                            require_type(expr.span, &declared, &actual, "binding")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                if !duplicate {
                    env.insert(name.clone(), signatures.canonical_type(ty));
                    if matches!(stmt.kind, StmtKind::Var { .. }) {
                        mutable.insert(name.clone());
                    }
                }
            }
            StmtKind::Assign {
                name,
                name_span,
                expr,
                ..
            } => {
                let Some(expected) = env.get(name).cloned() else {
                    diagnostics.push(diag(*name_span, &format!("unknown binding '{name}'")));
                    continue;
                };
                if !mutable.contains(name) {
                    diagnostics.push(diag(
                        *name_span,
                        &format!(
                            "cannot assign to immutable binding '{name}'; declare it with 'var'"
                        ),
                    ));
                    continue;
                }
                match type_of_expr(expr, env, signatures) {
                    Ok(actual) => {
                        if let Err(diagnostic) =
                            require_type(expr.span, &expected, &actual, "assignment")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
            }
            StmtKind::AssignMultiDestructure { bindings, expr } => {
                let actuals = match value_types_of_expr(expr, env, signatures) {
                    Ok(actuals) => Some(actuals),
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };
                if let Some(actuals) = actuals {
                    if actuals.len() != bindings.len() {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "multi-value assignment pattern expects {} values, expression returns {}",
                                bindings.len(),
                                actuals.len()
                            ),
                        ));
                    }
                    for (binding, actual) in bindings.iter().zip(actuals.iter()) {
                        if binding.name == "_" {
                            continue;
                        }
                        validate_assignment_target(
                            &binding.name,
                            binding.span,
                            actual,
                            env,
                            mutable,
                            signatures,
                            diagnostics,
                        );
                    }
                }
            }
            StmtKind::AssignListDestructure {
                bindings,
                rest,
                expr,
            } => {
                let element_ty = match type_of_expr(expr, env, signatures) {
                    Ok(actual) => match signatures.canonical_type(&actual) {
                        Type::List(element) => Some(*element),
                        other => {
                            diagnostics.push(diag(
                                expr.span,
                                &format!(
                                    "list assignment pattern requires a list value, got {}",
                                    other.name()
                                ),
                            ));
                            None
                        }
                    },
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };
                if let Some(element_ty) = element_ty {
                    for binding in bindings {
                        if binding.name != "_" {
                            validate_assignment_target(
                                &binding.name,
                                binding.span,
                                &element_ty,
                                env,
                                mutable,
                                signatures,
                                diagnostics,
                            );
                        }
                    }
                    if let Some(rest) = rest
                        && rest.binding.name != "_"
                    {
                        validate_assignment_target(
                            &rest.binding.name,
                            rest.binding.span,
                            &Type::List(Box::new(element_ty)),
                            env,
                            mutable,
                            signatures,
                            diagnostics,
                        );
                    }
                }
            }
            StmtKind::AssignStructDestructure {
                struct_name,
                struct_span,
                fields,
                expr,
            } => {
                let pattern_ty = signatures.canonical_type(&Type::Named(struct_name.clone()));
                let Type::Named(concrete_name) = &pattern_ty else {
                    diagnostics.push(diag(
                        *struct_span,
                        &format!("struct pattern '{struct_name}' does not name a struct type"),
                    ));
                    continue;
                };
                if signatures.struct_type(concrete_name).is_none() {
                    diagnostics.push(diag(
                        *struct_span,
                        &format!("struct pattern '{struct_name}' does not name a struct type"),
                    ));
                    continue;
                }
                match type_of_expr(expr, env, signatures) {
                    Ok(actual) => {
                        if let Err(diagnostic) = require_type(
                            expr.span,
                            &pattern_ty,
                            &actual,
                            "struct assignment pattern",
                        ) {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                validate_struct_assignment_fields(
                    fields,
                    concrete_name,
                    env,
                    mutable,
                    signatures,
                    diagnostics,
                );
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
                mutable: pattern_mutable,
            } => {
                let actuals = match value_types_of_expr(expr, env, signatures) {
                    Ok(actuals) => Some(actuals),
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };

                for binding in bindings {
                    if let Err(diagnostic) =
                        require_known_type(binding.type_span, &binding.ty, signatures)
                    {
                        diagnostics.push(diagnostic);
                    }
                }

                if let Some(actuals) = actuals.as_ref() {
                    if actuals.len() != bindings.len() {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "destructuring expects {} values, expression returns {}",
                                bindings.len(),
                                actuals.len()
                            ),
                        ));
                    }
                    for (binding, actual) in bindings.iter().zip(actuals) {
                        let declared = signatures.canonical_type(&binding.ty);
                        if let Err(diagnostic) = require_type(
                            stmt.span,
                            &declared,
                            actual,
                            &format!("destructured binding '{}'", binding.name),
                        ) {
                            diagnostics.push(diagnostic);
                        }
                    }
                }

                for binding in bindings {
                    if env.contains_key(&binding.name) {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!("'{}' is already defined in this scope", binding.name),
                        ));
                    } else {
                        let binding_ty = signatures.canonical_type(&binding.ty);
                        env.insert(binding.name.clone(), binding_ty.clone());
                        if *pattern_mutable {
                            if signatures.is_copy_type(&binding_ty) {
                                mutable.insert(binding.name.clone());
                            } else {
                                diagnostics.push(diag(
                                    binding.type_span,
                                    &format!(
                                        "non-copy destructured binding '{}' cannot be mutable until move/borrow semantics are implemented",
                                        binding.name
                                    ),
                                ));
                            }
                        }
                    }
                }

                if *else_return {
                    if bindings
                        .last()
                        .map(|binding| signatures.canonical_type(&binding.ty))
                        != Some(Type::Error)
                    {
                        diagnostics.push(diag(
                            stmt.span,
                            "'else return' requires the final destructured value to have type error",
                        ));
                    }
                    if let Some(actuals) = actuals.as_ref()
                        && actuals.as_slice() != return_types
                    {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "'else return' can only forward an exact return shape: function returns {}, call returns {}",
                                return_types_name(return_types),
                                return_types_name(actuals)
                            ),
                        ));
                    }
                }
            }
            StmtKind::LetMultiDestructure {
                bindings,
                expr,
                else_return,
                mutable: pattern_mutable,
            } => {
                let actuals = match value_types_of_expr(expr, env, signatures) {
                    Ok(actuals) => Some(actuals),
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };
                if let Some(actuals) = actuals.as_ref() {
                    if actuals.len() != bindings.len() {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "multi-value pattern expects {} values, expression returns {}",
                                bindings.len(),
                                actuals.len()
                            ),
                        ));
                    }
                    for (binding, actual) in bindings.iter().zip(actuals) {
                        if binding.name == "_" {
                            continue;
                        }
                        if env.contains_key(&binding.name) {
                            diagnostics.push(diag(
                                binding.span,
                                &format!("'{}' is already defined in this scope", binding.name),
                            ));
                        } else {
                            let binding_ty = signatures.canonical_type(actual);
                            env.insert(binding.name.clone(), binding_ty.clone());
                            if *pattern_mutable {
                                if signatures.is_copy_type(&binding_ty) {
                                    mutable.insert(binding.name.clone());
                                } else {
                                    diagnostics.push(diag(
                                        binding.span,
                                        &format!(
                                            "non-copy destructured binding '{}' cannot be mutable until move/borrow semantics are implemented",
                                            binding.name
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    if *else_return {
                        if actuals.last().map(|ty| signatures.canonical_type(ty))
                            != Some(Type::Error)
                        {
                            diagnostics.push(diag(
                                stmt.span,
                                "'else return' requires the final multi-value pattern position to have type error",
                            ));
                        }
                        let canonical_actuals = actuals
                            .iter()
                            .map(|ty| signatures.canonical_type(ty))
                            .collect::<Vec<_>>();
                        let canonical_returns = return_types
                            .iter()
                            .map(|ty| signatures.canonical_type(ty))
                            .collect::<Vec<_>>();
                        if canonical_actuals != canonical_returns {
                            diagnostics.push(diag(
                                stmt.span,
                                &format!(
                                    "'else return' can only forward an exact return shape: function returns {}, expression returns {}",
                                    return_types_name(return_types),
                                    return_types_name(actuals)
                                ),
                            ));
                        }
                    }
                }
            }
            StmtKind::LetListDestructure {
                bindings,
                rest,
                expr,
                mutable: pattern_mutable,
            } => {
                let element_ty = match type_of_expr(expr, env, signatures) {
                    Ok(actual) => match signatures.canonical_type(&actual) {
                        Type::List(element) => Some(*element),
                        other => {
                            diagnostics.push(diag(
                                expr.span,
                                &format!(
                                    "list destructuring requires a list value, got {}",
                                    other.name()
                                ),
                            ));
                            None
                        }
                    },
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };
                if let Some(element_ty) = element_ty {
                    for binding in bindings {
                        if binding.name == "_" {
                            continue;
                        }
                        if env.contains_key(&binding.name) {
                            diagnostics.push(diag(
                                binding.span,
                                &format!("'{}' is already defined in this scope", binding.name),
                            ));
                        } else {
                            env.insert(binding.name.clone(), element_ty.clone());
                            if *pattern_mutable {
                                if signatures.is_copy_type(&element_ty) {
                                    mutable.insert(binding.name.clone());
                                } else {
                                    diagnostics.push(diag(
                                        binding.span,
                                        &format!(
                                            "non-copy destructured binding '{}' cannot be mutable until move/borrow semantics are implemented",
                                            binding.name
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    if let Some(rest) = rest
                        && rest.binding.name != "_"
                    {
                        if env.contains_key(&rest.binding.name) {
                            diagnostics.push(diag(
                                rest.binding.span,
                                &format!(
                                    "'{}' is already defined in this scope",
                                    rest.binding.name
                                ),
                            ));
                        } else {
                            let rest_ty = Type::List(Box::new(element_ty));
                            env.insert(rest.binding.name.clone(), rest_ty.clone());
                            if *pattern_mutable {
                                diagnostics.push(diag(
                                    rest.binding.span,
                                    &format!(
                                        "non-copy destructured binding '{}' cannot be mutable until move/borrow semantics are implemented",
                                        rest.binding.name
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
            StmtKind::LetStructDestructure {
                struct_name,
                struct_span,
                fields,
                expr,
                mutable: pattern_mutable,
            } => {
                let pattern_ty = signatures.canonical_type(&Type::Named(struct_name.clone()));
                let Type::Named(concrete_name) = &pattern_ty else {
                    diagnostics.push(diag(
                        *struct_span,
                        &format!("struct pattern '{struct_name}' does not name a struct type"),
                    ));
                    continue;
                };
                let Some(_) = signatures.struct_type(concrete_name) else {
                    diagnostics.push(diag(
                        *struct_span,
                        &format!("struct pattern '{struct_name}' does not name a struct type"),
                    ));
                    continue;
                };
                match type_of_expr(expr, env, signatures) {
                    Ok(actual) => {
                        if let Err(diagnostic) =
                            require_type(expr.span, &pattern_ty, &actual, "struct destructuring")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                bind_struct_pattern_fields(fields, concrete_name, env, signatures, diagnostics);
                if *pattern_mutable {
                    let mut declarations = Vec::new();
                    collect_struct_pattern_declarations(fields, &mut declarations);
                    for (name, span, _) in declarations {
                        let Some(binding_ty) = env.get(&name).cloned() else {
                            continue;
                        };
                        if signatures.is_copy_type(&binding_ty) {
                            mutable.insert(name);
                        } else {
                            diagnostics.push(diag(
                                span,
                                &format!(
                                    "non-copy destructured binding '{name}' cannot be mutable until move/borrow semantics are implemented"
                                ),
                            ));
                        }
                    }
                }
            }
            StmtKind::Return(expressions) => {
                let actuals = if expressions.len() == 1 {
                    match value_types_of_expr(&expressions[0], env, signatures) {
                        Ok(actuals) => Some(actuals),
                        Err(diagnostic) => {
                            diagnostics.push(diagnostic);
                            None
                        }
                    }
                } else {
                    let mut actuals = Vec::with_capacity(expressions.len());
                    let mut complete = true;
                    for expr in expressions {
                        match type_of_expr(expr, env, signatures) {
                            Ok(actual) => actuals.push(actual),
                            Err(diagnostic) => {
                                diagnostics.push(diagnostic);
                                complete = false;
                            }
                        }
                    }
                    complete.then_some(actuals)
                };

                if let Some(actuals) = actuals {
                    if actuals.len() != return_types.len() {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "return expects {} values, got {}",
                                return_types.len(),
                                actuals.len()
                            ),
                        ));
                    } else {
                        for (index, (actual, expected)) in
                            actuals.iter().zip(return_types).enumerate()
                        {
                            if let Err(diagnostic) = require_type(
                                stmt.span,
                                expected,
                                actual,
                                &format!("return value {}", index + 1),
                            ) {
                                diagnostics.push(diagnostic);
                            }
                        }
                    }
                }
            }
            StmtKind::Break => {
                if loop_depth == 0 {
                    diagnostics.push(diag(
                        stmt.keyword_span,
                        "'break' is only valid inside a loop",
                    ));
                }
            }
            StmtKind::Continue => {
                if loop_depth == 0 {
                    diagnostics.push(diag(
                        stmt.keyword_span,
                        "'continue' is only valid inside a loop",
                    ));
                }
            }
            StmtKind::Expr(expr) => {
                if let Err(diagnostic) = type_of_expr(expr, env, signatures) {
                    diagnostics.push(diagnostic);
                }
            }
            StmtKind::Shell { expr, redirect, .. } => match type_of_expr(expr, env, signatures) {
                Ok(result_ty) => {
                    if let Some(redirect) = redirect {
                        match type_of_expr(&redirect.path, env, signatures) {
                            Ok(path_ty) => {
                                if let Err(diagnostic) = require_type(
                                    redirect.path.span,
                                    &Type::Str,
                                    &path_ty,
                                    "redirection path",
                                ) {
                                    diagnostics.push(diagnostic);
                                }
                            }
                            Err(diagnostic) => diagnostics.push(diagnostic),
                        }
                        if !matches!(result_ty, Type::I64 | Type::Bool | Type::Str | Type::Error) {
                            diagnostics.push(diag(
                                expr.span,
                                &format!(
                                    "redirection requires a scalar result, got {}",
                                    result_ty.name()
                                ),
                            ));
                        }
                    }
                }
                Err(diagnostic) => diagnostics.push(diagnostic),
            },
            StmtKind::If {
                cond,
                binding,
                body,
                else_body,
                ..
            } => {
                let promotion = binding
                    .is_none()
                    .then(|| optional_presence_promotion(cond, env, mutable, signatures))
                    .flatten();
                let mut then_env = env.clone();
                if let Some(binding) = binding {
                    match type_of_expr(cond, env, signatures) {
                        Ok(cond_type) => {
                            let cond_type = signatures.canonical_type(&cond_type);
                            match cond_type {
                                Type::Optional(inner) if *inner != Type::Void => {
                                    if binding.name != "_" {
                                        if env.contains_key(&binding.name) {
                                            diagnostics.push(diag(
                                                binding.span,
                                                &format!(
                                                    "optional pattern binding '{}' shadows an existing binding",
                                                    binding.name
                                                ),
                                            ));
                                        } else {
                                            then_env.insert(
                                                binding.name.clone(),
                                                signatures.canonical_type(&inner),
                                            );
                                        }
                                    }
                                }
                                Type::Optional(inner) if *inner == Type::Void => {
                                    diagnostics.push(diag(
                                        cond.span,
                                        "optional pattern binding on 'none' needs a concrete optional value type",
                                    ));
                                }
                                other => diagnostics.push(diag(
                                    cond.span,
                                    &format!(
                                        "optional pattern binding requires an optional value, got {}",
                                        other.name()
                                    ),
                                )),
                            }
                        }
                        Err(diagnostic) => diagnostics.push(diagnostic),
                    }
                } else {
                    match type_of_expr(cond, env, signatures) {
                        Ok(cond_type) => {
                            if let Err(diagnostic) =
                                require_type(cond.span, &Type::Bool, &cond_type, "if condition")
                            {
                                diagnostics.push(diagnostic);
                            }
                        }
                        Err(diagnostic) => diagnostics.push(diagnostic),
                    }
                }
                if let Some((name, inner, true)) = &promotion {
                    then_env.insert(name.clone(), inner.clone());
                }
                let mut then_mutable = mutable.clone();
                check_block_all(
                    body,
                    &mut then_env,
                    &mut then_mutable,
                    return_types,
                    signatures,
                    diagnostics,
                    loop_depth,
                );
                let mut else_env = env.clone();
                if let Some((name, inner, false)) = &promotion {
                    else_env.insert(name.clone(), inner.clone());
                }
                let mut else_mutable = mutable.clone();
                check_block_all(
                    else_body,
                    &mut else_env,
                    &mut else_mutable,
                    return_types,
                    signatures,
                    diagnostics,
                    loop_depth,
                );
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                body,
                ..
            } => {
                let shadows = env.contains_key(name);
                if shadows {
                    diagnostics.push(diag(
                        stmt.span,
                        &format!("loop variable '{name}' shadows an existing binding"),
                    ));
                }
                match type_of_expr(start, env, signatures) {
                    Ok(start_type) => {
                        if let Err(diagnostic) =
                            require_type(start.span, &Type::I64, &start_type, "range start")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                match type_of_expr(end, env, signatures) {
                    Ok(end_type) => {
                        if let Err(diagnostic) =
                            require_type(end.span, &Type::I64, &end_type, "range end")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                let mut nested = env.clone();
                let mut nested_mutable = mutable.clone();
                if !shadows {
                    nested.insert(name.clone(), Type::I64);
                }
                check_block_all(
                    body,
                    &mut nested,
                    &mut nested_mutable,
                    return_types,
                    signatures,
                    diagnostics,
                    loop_depth + 1,
                );
            }
            StmtKind::ForEach {
                index_name,
                name,
                iterable,
                body,
                ..
            } => {
                let element_type = match type_of_expr(iterable, env, signatures) {
                    Ok(iterable_type) => match signatures.canonical_type(&iterable_type) {
                        Type::List(element) => Some(*element),
                        actual => {
                            diagnostics.push(diag(
                                iterable.span,
                                &format!("for-loop source must be a list, got {}", actual.name()),
                            ));
                            None
                        }
                    },
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };
                let mut nested = env.clone();
                let mut nested_mutable = mutable.clone();
                if let Some(index_name) = index_name {
                    if env.contains_key(index_name) {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!("loop index '{index_name}' shadows an existing binding"),
                        ));
                    } else {
                        nested.insert(index_name.clone(), Type::I64);
                    }
                }
                if env.contains_key(name) {
                    diagnostics.push(diag(
                        stmt.span,
                        &format!("loop variable '{name}' shadows an existing binding"),
                    ));
                } else if let Some(element_type) = element_type {
                    nested.insert(name.clone(), element_type);
                }
                check_block_all(
                    body,
                    &mut nested,
                    &mut nested_mutable,
                    return_types,
                    signatures,
                    diagnostics,
                    loop_depth + 1,
                );
            }
            StmtKind::While { cond, body } => {
                match type_of_expr(cond, env, signatures) {
                    Ok(cond_type) => {
                        if let Err(diagnostic) =
                            require_type(cond.span, &Type::Bool, &cond_type, "while condition")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                let mut nested = env.clone();
                let mut nested_mutable = mutable.clone();
                check_block_all(
                    body,
                    &mut nested,
                    &mut nested_mutable,
                    return_types,
                    signatures,
                    diagnostics,
                    loop_depth + 1,
                );
            }
            StmtKind::Match { value, arms } => {
                let value_ty = match type_of_expr(value, env, signatures) {
                    Ok(value_ty) => value_ty,
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        continue;
                    }
                };
                let Type::Named(enum_name) = value_ty else {
                    diagnostics.push(diag(
                        value.span,
                        &format!("match requires an enum value, got {}", value_ty.name()),
                    ));
                    continue;
                };
                let Some(definition) = signatures.enum_type(&enum_name) else {
                    diagnostics.push(diag(
                        value.span,
                        &format!("match requires an enum value, got {enum_name}"),
                    ));
                    continue;
                };
                let mut covered = HashSet::new();
                for arm in arms {
                    if arm.enum_name != enum_name {
                        diagnostics.push(diag(
                            arm.enum_span,
                            &format!(
                                "match arm uses enum '{}', expected '{enum_name}'",
                                arm.enum_name
                            ),
                        ));
                        continue;
                    }
                    let Some(variant) = definition.variant(&arm.variant) else {
                        diagnostics.push(
                            diag(
                                arm.variant_span,
                                &format!("enum '{enum_name}' has no variant '{}'", arm.variant),
                            )
                            .with_label(definition.span, format!("'{enum_name}' is declared here")),
                        );
                        continue;
                    };
                    if covered.contains(arm.variant.as_str()) {
                        diagnostics.push(diag(
                            arm.variant_span,
                            &format!(
                                "duplicate match arm for '{enum_name}.{}' is unreachable; an earlier unguarded arm already covers this variant",
                                arm.variant
                            ),
                        ));
                        continue;
                    }
                    if arm.patterns.len() != variant.payloads.len() {
                        diagnostics.push(diag(
                            arm.span,
                            &format!(
                                "match arm '{}.{}' expects {} payload pattern{}, got {}",
                                enum_name,
                                arm.variant,
                                variant.payloads.len(),
                                if variant.payloads.len() == 1 { "" } else { "s" },
                                arm.patterns.len()
                            ),
                        ));
                        continue;
                    }
                    let mut nested = env.clone();
                    for (pattern, payload_ty) in arm.patterns.iter().zip(&variant.payloads) {
                        match pattern {
                            MatchPattern::Binding(binding) => {
                                if binding.name == "_" {
                                    continue;
                                }
                                if nested.contains_key(&binding.name) {
                                    diagnostics.push(diag(
                                        binding.span,
                                        &format!(
                                            "match binding '{}' shadows an existing binding",
                                            binding.name
                                        ),
                                    ));
                                } else {
                                    nested.insert(binding.name.clone(), payload_ty.clone());
                                }
                            }
                            MatchPattern::Struct(pattern) => {
                                let expected = signatures
                                    .canonical_type(&Type::Named(pattern.struct_name.clone()));
                                let actual = signatures.canonical_type(payload_ty);
                                if let Err(diagnostic) = require_type(
                                    pattern.struct_span,
                                    &expected,
                                    &actual,
                                    "match struct pattern",
                                ) {
                                    diagnostics.push(diagnostic);
                                    continue;
                                }
                                let Type::Named(struct_name) = expected else {
                                    diagnostics.push(diag(
                                        pattern.struct_span,
                                        &format!(
                                            "struct pattern '{}' does not name a struct type",
                                            pattern.struct_name
                                        ),
                                    ));
                                    continue;
                                };
                                if signatures.struct_type(&struct_name).is_none() {
                                    diagnostics.push(diag(
                                        pattern.struct_span,
                                        &format!(
                                            "struct pattern '{}' does not name a struct type",
                                            pattern.struct_name
                                        ),
                                    ));
                                    continue;
                                }
                                bind_struct_pattern_fields(
                                    &pattern.fields,
                                    &struct_name,
                                    &mut nested,
                                    signatures,
                                    diagnostics,
                                );
                            }
                            MatchPattern::Relational(_) | MatchPattern::Logical { .. } => {
                                if let Err(diagnostic) = validate_relational_match_pattern(
                                    pattern, payload_ty, signatures,
                                ) {
                                    diagnostics.push(diagnostic);
                                }
                            }
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        match type_of_expr(guard, &nested, signatures) {
                            Ok(actual) => {
                                if let Err(diagnostic) =
                                    require_type(guard.span, &Type::Bool, &actual, "match guard")
                                {
                                    diagnostics.push(diagnostic);
                                }
                            }
                            Err(diagnostic) => diagnostics.push(diagnostic),
                        }
                    } else if arm.patterns.iter().all(match_pattern_is_irrefutable) {
                        covered.insert(arm.variant.as_str());
                    }
                    let mut nested_mutable = mutable.clone();
                    check_block_all(
                        &arm.body,
                        &mut nested,
                        &mut nested_mutable,
                        return_types,
                        signatures,
                        diagnostics,
                        loop_depth,
                    );
                }
                let missing = definition
                    .variants
                    .iter()
                    .filter(|variant| !covered.contains(variant.name.as_str()))
                    .map(|variant| variant.name.as_str())
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    diagnostics.push(diag(
                        stmt.span,
                        &format!(
                            "non-exhaustive match on '{enum_name}'; missing {}",
                            missing.join(", ")
                        ),
                    ));
                }
            }
            StmtKind::ListMatch { value, arms } => {
                let element_ty = match type_of_expr(value, env, signatures) {
                    Ok(value_ty) => match signatures.canonical_type(&value_ty) {
                        Type::List(element) => Some(*element),
                        actual => {
                            diagnostics.push(diag(
                                value.span,
                                &format!("list match requires a list value, got {}", actual.name()),
                            ));
                            None
                        }
                    },
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };
                let patterns = arms
                    .iter()
                    .map(|arm| (&arm.pattern, arm.guard.is_some()))
                    .collect::<Vec<_>>();
                if let Err(diagnostic) = validate_list_match_coverage(&patterns, stmt.span) {
                    diagnostics.push(diagnostic);
                }
                if let Some(element_ty) = element_ty {
                    for arm in arms {
                        let mut nested = env.clone();
                        if let Err(diagnostic) =
                            bind_list_match_pattern(&arm.pattern, &element_ty, &mut nested)
                        {
                            diagnostics.push(diagnostic);
                        }
                        if let Some(guard) = &arm.guard {
                            match type_of_expr(guard, &nested, signatures) {
                                Ok(actual) => {
                                    if let Err(diagnostic) = require_type(
                                        guard.span,
                                        &Type::Bool,
                                        &actual,
                                        "match guard",
                                    ) {
                                        diagnostics.push(diagnostic);
                                    }
                                }
                                Err(diagnostic) => diagnostics.push(diagnostic),
                            }
                        }
                        let mut nested_mutable = mutable.clone();
                        check_block_all(
                            &arm.body,
                            &mut nested,
                            &mut nested_mutable,
                            return_types,
                            signatures,
                            diagnostics,
                            loop_depth,
                        );
                    }
                }
            }
        }
    }
}

fn check_cfg_moved_reads(
    function: &Function,
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let graph = ControlFlowGraph::from_function(function, signatures);
    for node in graph.nodes() {
        let Some(state) = graph.move_state_before(node.id) else {
            continue;
        };
        if !state.reachable() {
            continue;
        }
        let mut moved_reads = node
            .ownership
            .borrows
            .iter()
            .map(|borrow| &borrow.source)
            .filter(|name| state.is_moved(name))
            .cloned()
            .collect::<Vec<_>>();
        moved_reads.sort();
        moved_reads.dedup();
        if moved_reads.is_empty() {
            continue;
        }
        let mut diagnostic = diag(
            node.span,
            &format!(
                "use of moved non-copy binding{} {}",
                if moved_reads.len() == 1 { "" } else { "s" },
                moved_reads
                    .iter()
                    .map(|name| format!("'{name}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        for name in moved_reads {
            if let Some(move_span) = state.origin(&name) {
                diagnostic = diagnostic.with_label(move_span, format!("'{name}' moved here"));
            }
        }
        diagnostics.push(diagnostic);
    }
    check_cfg_live_borrow_moves(&graph, diagnostics);
}

fn check_cfg_live_borrow_moves(graph: &ControlFlowGraph, diagnostics: &mut Vec<Diagnostic>) {
    for node in graph.nodes() {
        if !graph.is_reachable(node.id) || node.ownership.moves.is_empty() {
            continue;
        }
        let Some(live_after) = graph.live_after(node.id) else {
            continue;
        };
        for ownership_move in &node.ownership.moves {
            let mut aliases = live_after
                .live()
                .iter()
                .filter(|name| {
                    *name != &ownership_move.source && *name != &ownership_move.destination
                })
                .filter_map(|name| {
                    graph
                        .borrowed_reaching_definition_span(node.id, name, &ownership_move.source)
                        .map(|span| (name.clone(), span))
                })
                .collect::<Vec<_>>();
            aliases.sort_by(|left, right| left.0.cmp(&right.0));
            aliases.dedup_by(|left, right| left.0 == right.0);
            for (alias, borrow_span) in aliases {
                diagnostics.push(
                    diag(
                        ownership_move.span,
                        &format!(
                            "cannot move non-copy binding '{}' while borrowed view '{}' is still live",
                            ownership_move.source, alias
                        ),
                    )
                    .with_label(borrow_span, format!("'{alias}' borrows from '{}'", ownership_move.source))
                    .with_note("use the borrowed view before moving its owner, or move the owner only after the view's last use"),
                );
            }
        }
    }
}

fn match_pattern_is_irrefutable(pattern: &MatchPattern) -> bool {
    matches!(pattern, MatchPattern::Binding(_) | MatchPattern::Struct(_))
}

fn validate_relational_match_pattern(
    pattern: &MatchPattern,
    payload_ty: &Type,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    match pattern {
        MatchPattern::Relational(pattern) => {
            let Some(value) = constant_primitive_value(&pattern.value, signatures) else {
                return Err(diag(
                    pattern.value.span,
                    "relational match pattern values must be compile-time primitive constants",
                ));
            };
            let payload_ty = signatures.canonical_type(payload_ty);
            let value_ty = value.ty();
            require_type(
                pattern.value.span,
                &payload_ty,
                &value_ty,
                "relational match pattern",
            )?;
            if matches!(pattern.op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
                && payload_ty != Type::I64
            {
                return Err(diag(
                    pattern.span,
                    "ordered relational match patterns currently require i64 payloads",
                ));
            }
            if !matches!(payload_ty, Type::I64 | Type::Bool | Type::Str) {
                return Err(diag(
                    pattern.span,
                    &format!(
                        "relational match patterns are not supported for {} payloads",
                        payload_ty.name()
                    ),
                ));
            }
            Ok(())
        }
        MatchPattern::Logical { left, right, .. } => {
            validate_relational_match_pattern(left, payload_ty, signatures)?;
            validate_relational_match_pattern(right, payload_ty, signatures)
        }
        _ => Ok(()),
    }
}

fn bind_list_match_pattern(
    pattern: &ListMatchPattern,
    element_ty: &Type,
    env: &mut HashMap<String, Type>,
) -> Result<(), Diagnostic> {
    let ListMatchPattern::List { bindings, rest, .. } = pattern else {
        return Ok(());
    };
    for binding in bindings {
        if binding.name == "_" {
            continue;
        }
        if env.contains_key(&binding.name) {
            return Err(diag(
                binding.span,
                &format!(
                    "match binding '{}' shadows an existing binding",
                    binding.name
                ),
            ));
        }
        env.insert(binding.name.clone(), element_ty.clone());
    }
    if let Some(rest) = rest
        && rest.binding.name != "_"
    {
        if env.contains_key(&rest.binding.name) {
            return Err(diag(
                rest.binding.span,
                &format!(
                    "match binding '{}' shadows an existing binding",
                    rest.binding.name
                ),
            ));
        }
        env.insert(
            rest.binding.name.clone(),
            Type::List(Box::new(element_ty.clone())),
        );
    }
    Ok(())
}

fn list_match_pattern_span(pattern: &ListMatchPattern) -> SourceSpan {
    match pattern {
        ListMatchPattern::List { span, .. } | ListMatchPattern::Wildcard { span } => *span,
    }
}

fn validate_list_match_coverage(
    patterns: &[(&ListMatchPattern, bool)],
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    if patterns.is_empty() {
        return Err(diag(span, "list match requires at least one arm"));
    }
    let mut exact_lengths = HashSet::new();
    let mut minimum_rest: Option<usize> = None;
    let mut wildcard_seen = false;
    for (pattern, guarded) in patterns {
        let already_complete = minimum_rest
            .is_some_and(|minimum| (0..minimum).all(|length| exact_lengths.contains(&length)));
        if wildcard_seen || already_complete {
            return Err(diag(
                list_match_pattern_span(pattern),
                "unreachable list match arm; earlier patterns are already exhaustive",
            ));
        }
        match pattern {
            ListMatchPattern::Wildcard { .. } => {
                if !*guarded {
                    wildcard_seen = true;
                }
            }
            ListMatchPattern::List { bindings, rest, .. } => {
                let fixed = bindings.len();
                if rest.is_some() {
                    if minimum_rest.is_some_and(|minimum| minimum <= fixed) {
                        return Err(diag(
                            list_match_pattern_span(pattern),
                            "unreachable list match arm; an earlier rest pattern already covers this length range",
                        ));
                    }
                    if !*guarded {
                        minimum_rest =
                            Some(minimum_rest.map_or(fixed, |minimum| minimum.min(fixed)));
                    }
                } else {
                    if exact_lengths.contains(&fixed)
                        || minimum_rest.is_some_and(|minimum| minimum <= fixed)
                    {
                        return Err(diag(
                            list_match_pattern_span(pattern),
                            &format!(
                                "unreachable list match arm for exactly {fixed} element{}",
                                if fixed == 1 { "" } else { "s" }
                            ),
                        ));
                    }
                    if !*guarded {
                        exact_lengths.insert(fixed);
                    }
                }
            }
        }
    }
    if wildcard_seen {
        return Ok(());
    }
    let Some(minimum_rest) = minimum_rest else {
        return Err(diag(
            span,
            "non-exhaustive list match; add a rest pattern or '_:' fallback",
        ));
    };
    let missing = (0..minimum_rest)
        .filter(|length| !exact_lengths.contains(length))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(diag(
            span,
            &format!(
                "non-exhaustive list match; missing exact length{} {} before the rest range",
                if missing.len() == 1 { "" } else { "s" },
                missing
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ))
    }
}

fn pipe_input_expr(input: &Expr, env: &HashMap<String, Type>, signatures: &Signatures) -> Expr {
    let ExprKind::Var(name) = &input.kind else {
        return input.clone();
    };
    let zero_arg_declared = signatures
        .get(name)
        .is_some_and(|signature| signature.params.is_empty());
    let zero_arg_value = matches!(
        env.get(name),
        Some(Type::Function { params, .. }) if params.is_empty()
    );
    if zero_arg_declared || zero_arg_value {
        Expr {
            line: input.line,
            span: input.span,
            kind: ExprKind::Call {
                name: name.clone(),
                args: Vec::new(),
                named_args: Vec::new(),
            },
        }
    } else {
        input.clone()
    }
}

fn type_of_optional_pipe(
    expr: &Expr,
    input: &Expr,
    name: &str,
    args: &[Expr],
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Type, Diagnostic> {
    let input_expr = pipe_input_expr(input, env, signatures);
    let input_ty = signatures.canonical_type(&type_of_expr(&input_expr, env, signatures)?);
    let Type::Optional(inner) = input_ty else {
        return Err(diag(
            input.span,
            &format!(
                "optional cascade '?..' requires an optional target, got {}",
                input_ty.name()
            ),
        ));
    };
    if *inner == Type::Void {
        return Err(diag(
            input.span,
            "optional cascade '?..' cannot infer a value type from bare none",
        ));
    }

    let synthetic_name = format!(
        "__flux_optional_cascade_{}_{}",
        expr.span.line, expr.span.column
    );
    let mut nested_env = env.clone();
    nested_env.insert(synthetic_name.clone(), (*inner).clone());
    let synthetic_input = Expr {
        line: input.line,
        span: input.span,
        kind: ExprKind::Var(synthetic_name),
    };
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(synthetic_input);
    call_args.extend(args.iter().cloned());
    let call = Expr {
        line: expr.line,
        span: expr.span,
        kind: ExprKind::Call {
            name: name.to_string(),
            args: call_args,
            named_args: Vec::new(),
        },
    };
    let output = signatures.canonical_type(&type_of_expr(&call, &nested_env, signatures)?);
    if output == Type::Void {
        return Err(diag(
            expr.span,
            "optional cascade stages must return a value",
        ));
    }
    let result = if matches!(output, Type::Optional(_)) {
        output
    } else {
        Type::Optional(Box::new(output))
    };
    require_known_type(expr.span, &result, signatures)?;
    Ok(result)
}

fn type_of_anonymous_function(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    allow_copy_captures: bool,
) -> Result<Type, Diagnostic> {
    let ExprKind::AnonymousFunction {
        params,
        return_type,
        body,
    } = &expr.kind
    else {
        return Err(diag(expr.span, "expected anonymous function"));
    };

    let mut reads = HashSet::new();
    collect_expr_reads(body, &mut reads);
    let param_names = params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<HashSet<_>>();
    let mut captures = reads
        .iter()
        .filter(|name| env.contains_key(*name) && !param_names.contains(name.as_str()))
        .map(|name| name.as_str())
        .collect::<Vec<_>>();
    captures.sort_unstable();
    if !captures.is_empty() && !allow_copy_captures {
        let captured = captures[0];
        return Err(diag(
            body.span,
            &format!(
                "anonymous function captures outer binding '{captured}'; general closure capture semantics are not implemented yet"
            ),
        ));
    }
    if allow_copy_captures {
        if let Some(captured) = captures.iter().find(|name| {
            env.get(**name)
                .is_some_and(|ty| !signatures.is_copy_type(ty))
        }) {
            let ty = env
                .get(*captured)
                .expect("captured binding was resolved from the environment");
            return Err(diag(
                body.span,
                &format!(
                    "inline anonymous callback capture '{captured}' must be Copy, got {}",
                    signatures.canonical_type(ty).name()
                ),
            )
            .with_note("non-copy captures require first-class borrow/lifetime semantics before they can be safe"));
        }
    }

    let mut lambda_env = if allow_copy_captures {
        env.clone()
    } else {
        HashMap::new()
    };
    for param in params {
        require_known_type(param.type_span, &param.ty, signatures)?;
        let ty = signatures.canonical_type(&param.ty);
        if ty == Type::Void {
            return Err(diag(
                param.type_span,
                "anonymous function parameters cannot have type void",
            ));
        }
        lambda_env.insert(param.name.clone(), ty);
    }
    for param in params {
        if !param.name.starts_with('_') && !reads.contains(&param.name) {
            return Err(diag(
                param.name_span,
                &format!("unused anonymous function parameter '{}'", param.name),
            )
            .with_note("Flux has no lint-warning tier: unused bindings are compile errors; prefix an intentionally ignored binding with '_'"));
        }
    }

    let actual = signatures.canonical_type(&type_of_expr(body, &lambda_env, signatures)?);
    let returns = if let Some(declared) = return_type {
        require_known_type(expr.span, declared, signatures)?;
        let declared = signatures.canonical_type(declared);
        if declared == Type::Void {
            require_type(body.span, &Type::Void, &actual, "anonymous function body")?;
            Vec::new()
        } else {
            require_type(body.span, &declared, &actual, "anonymous function body")?;
            vec![declared]
        }
    } else if actual == Type::Void {
        Vec::new()
    } else {
        vec![actual]
    };
    if returns
        .iter()
        .any(|ty| matches!(signatures.canonical_type(ty), Type::List(_)))
    {
        return Err(diag(
            body.span,
            "list values cannot be returned from anonymous functions until collection ownership is implemented",
        ));
    }
    Ok(Type::Function {
        params: params
            .iter()
            .map(|param| signatures.canonical_type(&param.ty))
            .collect(),
        returns,
    })
}

fn type_of_sequence_callback(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Type, Diagnostic> {
    if matches!(expr.kind, ExprKind::AnonymousFunction { .. }) {
        type_of_anonymous_function(expr, env, signatures, true)
    } else {
        type_of_expr(expr, env, signatures)
    }
}

pub fn type_of_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Type, Diagnostic> {
    match &expr.kind {
        ExprKind::Int(_) => Ok(Type::I64),
        ExprKind::Bool(_) => Ok(Type::Bool),
        ExprKind::Str(_) => Ok(Type::Str),
        ExprKind::Nil => Ok(Type::Error),
        ExprKind::None => Ok(Type::Optional(Box::new(Type::Void))),
        ExprKind::Var(name) => {
            if let Some(ty) = env.get(name) {
                return Ok(ty.clone());
            }
            if let Some(constant) = signatures.constant(name) {
                require_visible_declaration(
                    expr.span,
                    constant.span,
                    constant.public,
                    "constant",
                    name,
                    signatures,
                )?;
                return Ok(constant.ty.clone());
            }
            if let Some(signature) = signatures.get(name) {
                require_visible_declaration(
                    expr.span,
                    signature.span,
                    signature.public,
                    "function",
                    name,
                    signatures,
                )?;
                return Ok(Type::Function {
                    params: signature.params.clone(),
                    returns: signature.returns.clone(),
                });
            }
            Err(diag(
                expr.span,
                &format!("unknown binding, constant, or function '{name}'"),
            ))
        }
        ExprKind::AnonymousFunction { .. } => {
            type_of_anonymous_function(expr, env, signatures, false)
        }
        ExprKind::ShellCall { name, args, .. } => {
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: args.clone(),
                    named_args: Vec::new(),
                },
            };
            type_of_expr(&call, env, signatures)
        }
        ExprKind::Pipe {
            input,
            name,
            args,
            optional,
            ..
        } => {
            if *optional {
                return type_of_optional_pipe(expr, input, name, args, env, signatures);
            }
            let mut call_args = Vec::with_capacity(args.len() + 1);
            call_args.push(pipe_input_expr(input, env, signatures));
            call_args.extend(args.iter().cloned());
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: call_args,
                    named_args: Vec::new(),
                },
            };
            type_of_expr(&call, env, signatures)
        }
        ExprKind::List(items) => {
            let Some(first) = items.first() else {
                return Err(diag(
                    expr.span,
                    "empty list literals cannot infer an element type yet",
                ));
            };
            let item_element_type = |item: &Expr| -> Result<Type, Diagnostic> {
                match &item.kind {
                    ExprKind::ListSpread { value, .. } => {
                        let spread_ty =
                            signatures.canonical_type(&type_of_expr(value, env, signatures)?);
                        let Type::List(element) = spread_ty else {
                            return Err(diag(value.span, "list spread expression must be a list"));
                        };
                        Ok(*element)
                    }
                    ExprKind::ListOptional { value, .. } => {
                        let optional_ty =
                            signatures.canonical_type(&type_of_expr(value, env, signatures)?);
                        let Type::Optional(inner) = optional_ty else {
                            return Err(diag(
                                value.span,
                                "null-aware list element requires an optional value",
                            ));
                        };
                        if matches!(*inner, Type::Void) {
                            return Err(diag(
                                value.span,
                                "null-aware list element cannot infer a value type from bare none",
                            ));
                        }
                        Ok(*inner)
                    }
                    ExprKind::ListIf {
                        condition,
                        binding,
                        value,
                        else_value,
                        ..
                    } => {
                        let value_ty = if let Some(binding) = binding {
                            let condition_ty = signatures
                                .canonical_type(&type_of_expr(condition, env, signatures)?);
                            let Type::Optional(inner) = condition_ty else {
                                return Err(diag(
                                    condition.span,
                                    &format!(
                                        "list optional guard requires an optional value, got {}",
                                        condition_ty.name()
                                    ),
                                ));
                            };
                            if matches!(inner.as_ref(), Type::Void) {
                                return Err(diag(
                                    condition.span,
                                    "list optional guard on 'none' needs a concrete optional value type",
                                ));
                            }
                            let mut guarded_env = env.clone();
                            if binding.name != "_" {
                                if env.contains_key(&binding.name) {
                                    return Err(diag(
                                        binding.span,
                                        &format!(
                                            "list optional guard binding '{}' shadows an existing binding",
                                            binding.name
                                        ),
                                    ));
                                }
                                guarded_env.insert(
                                    binding.name.clone(),
                                    signatures.canonical_type(&inner),
                                );
                            }
                            type_of_expr(value, &guarded_env, signatures)?
                        } else {
                            let condition_ty = type_of_expr(condition, env, signatures)?;
                            require_type(
                                condition.span,
                                &Type::Bool,
                                &condition_ty,
                                "list if condition",
                            )?;
                            type_of_expr(value, env, signatures)?
                        };
                        if let Some(else_value) = else_value {
                            let else_ty = type_of_expr(else_value, env, signatures)?;
                            require_type(
                                else_value.span,
                                &value_ty,
                                &else_ty,
                                "list else element",
                            )?;
                        }
                        Ok(value_ty)
                    }
                    _ => type_of_expr(item, env, signatures),
                }
            };
            let element_ty = item_element_type(first)?;
            if matches!(element_ty, Type::Void | Type::Function { .. }) {
                return Err(diag(
                    first.span,
                    &format!("list elements cannot have type {}", element_ty.name()),
                ));
            }
            for item in items.iter().skip(1) {
                let actual = item_element_type(item)?;
                require_type(item.span, &element_ty, &actual, "list element")?;
            }
            Ok(Type::List(Box::new(element_ty)))
        }
        ExprKind::ListSpread { .. } => Err(diag(
            expr.span,
            "list spread syntax is only valid inside a list literal",
        )),
        ExprKind::ListOptional { .. } => Err(diag(
            expr.span,
            "null-aware list element syntax is only valid inside a list literal",
        )),
        ExprKind::ListIf { .. } => Err(diag(
            expr.span,
            "list if/else syntax is only valid inside a list literal",
        )),
        ExprKind::Index { base, index } => {
            let base_ty = signatures.canonical_type(&type_of_expr(base, env, signatures)?);
            let Type::List(element) = base_ty else {
                return Err(diag(base.span, "indexing currently requires a list value"));
            };
            let index_ty = type_of_expr(index, env, signatures)?;
            require_type(index.span, &Type::I64, &index_ty, "list index")?;
            Ok(*element)
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            let base_ty = signatures.canonical_type(&type_of_expr(base, env, signatures)?);
            let Type::List(element) = base_ty else {
                return Err(diag(base.span, "slicing currently requires a list value"));
            };
            for (bound, label) in [
                (start.as_deref(), "slice start"),
                (end.as_deref(), "slice end"),
                (step.as_deref(), "slice step"),
            ] {
                if let Some(bound) = bound {
                    let bound_ty = type_of_expr(bound, env, signatures)?;
                    require_type(bound.span, &Type::I64, &bound_ty, label)?;
                }
            }
            if matches!(
                step.as_deref().map(|step| &step.kind),
                Some(ExprKind::Int(0))
            ) {
                return Err(diag(
                    step.as_deref().expect("zero step exists").span,
                    "list slice step cannot be zero",
                ));
            }
            Ok(Type::List(element))
        }
        ExprKind::ListComprehension {
            value,
            binding,
            binding_span,
            iterable,
            condition,
        } => {
            let iterable_ty = signatures.canonical_type(&type_of_expr(iterable, env, signatures)?);
            let Type::List(element) = iterable_ty else {
                return Err(diag(
                    iterable.span,
                    "list comprehension 'in' expression must be a list",
                ));
            };
            if env.contains_key(binding) {
                return Err(diag(
                    *binding_span,
                    &format!("list comprehension binding '{binding}' shadows an existing binding"),
                ));
            }
            let mut comprehension_reads = HashSet::new();
            collect_expr_reads(value, &mut comprehension_reads);
            if let Some(condition) = condition {
                collect_expr_reads(condition, &mut comprehension_reads);
            }
            if !binding.starts_with('_') && !comprehension_reads.contains(binding) {
                return Err(diag(
                    *binding_span,
                    &format!("unused list comprehension binding '{binding}'"),
                )
                .with_note(
                    "Flux has no lint-warning tier: unused bindings are compile errors; prefix an intentionally ignored binding with '_'",
                ));
            }
            let mut nested = env.clone();
            nested.insert(binding.clone(), (*element).clone());
            if let Some(condition) = condition {
                let condition_ty = type_of_expr(condition, &nested, signatures)?;
                require_type(
                    condition.span,
                    &Type::Bool,
                    &condition_ty,
                    "list comprehension filter",
                )?;
            }
            let value_ty = type_of_expr(value, &nested, signatures)?;
            if matches!(value_ty, Type::Void | Type::Function { .. }) {
                return Err(diag(
                    value.span,
                    &format!("list comprehension cannot produce {}", value_ty.name()),
                ));
            }
            Ok(Type::List(Box::new(value_ty)))
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "chunked" => {
            if !named_args.is_empty() {
                return Err(diag(expr.span, "chunked does not accept named arguments"));
            }
            if args.len() != 2 {
                return Err(diag(
                    expr.span,
                    "chunked expects exactly two arguments: a list and an i64 size",
                ));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            if !matches!(list_ty, Type::List(_)) {
                return Err(diag(
                    args[0].span,
                    "chunked expects a list as its first argument",
                ));
            }
            let size_ty = signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
            require_type(args[1].span, &Type::I64, &size_ty, "chunked size")?;
            if matches!(args[1].kind, ExprKind::Int(0)) {
                return Err(diag(args[1].span, "chunked size must be greater than zero"));
            }
            Ok(Type::List(Box::new(list_ty)))
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "sorted" => {
            if !named_args.is_empty() {
                return Err(diag(expr.span, "sorted does not accept named arguments"));
            }
            if args.len() != 1 {
                return Err(diag(expr.span, "sorted expects exactly one list argument"));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            let Type::List(element) = &list_ty else {
                return Err(diag(args[0].span, "sorted expects a list argument"));
            };
            let element_ty = signatures.canonical_type(element);
            if !matches!(element_ty, Type::I64 | Type::Bool | Type::Str) {
                return Err(diag(
                    args[0].span,
                    &format!(
                        "sorted requires ordered scalar list elements, got {}",
                        element_ty.name()
                    ),
                ));
            }
            Ok(list_ty)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "flatten" => {
            if !named_args.is_empty() {
                return Err(diag(expr.span, "flatten does not accept named arguments"));
            }
            if args.len() != 1 {
                return Err(diag(
                    expr.span,
                    "flatten expects exactly one nested-list argument",
                ));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            let Type::List(outer_element) = list_ty else {
                return Err(diag(args[0].span, "flatten expects a nested list argument"));
            };
            let outer_element = signatures.canonical_type(&outer_element);
            let Type::List(inner_element) = outer_element else {
                return Err(diag(
                    args[0].span,
                    "flatten expects a list whose elements are lists",
                ));
            };
            Ok(Type::List(inner_element))
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "distinct" => {
            if !named_args.is_empty() {
                return Err(diag(expr.span, "distinct does not accept named arguments"));
            }
            if args.len() != 1 {
                return Err(diag(
                    expr.span,
                    "distinct expects exactly one list argument",
                ));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            let Type::List(element) = &list_ty else {
                return Err(diag(args[0].span, "distinct expects a list argument"));
            };
            let element_ty = signatures.canonical_type(element);
            if !matches!(element_ty, Type::I64 | Type::Bool | Type::Str | Type::Error) {
                return Err(diag(
                    args[0].span,
                    &format!(
                        "distinct requires list elements with scalar equality, got {}",
                        element_ty.name()
                    ),
                ));
            }
            Ok(list_ty)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "concat" => {
            if !named_args.is_empty() {
                return Err(diag(expr.span, "concat does not accept named arguments"));
            }
            if args.len() != 2 {
                return Err(diag(expr.span, "concat expects exactly two list arguments"));
            }
            let left_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            let Type::List(_) = &left_ty else {
                return Err(diag(
                    args[0].span,
                    "concat expects a list as its first argument",
                ));
            };
            let right_ty = signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
            require_type(args[1].span, &left_ty, &right_ty, "concat right list")?;
            Ok(left_ty)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "map" || name == "filter" || name == "where" => {
            if !named_args.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!("{name} does not accept named arguments"),
                ));
            }
            if args.len() != 2 {
                return Err(diag(
                    expr.span,
                    &format!("{name} expects exactly two arguments: a list and a callback"),
                ));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            let Type::List(element) = list_ty else {
                return Err(diag(
                    args[0].span,
                    &format!("{name} expects a list as its first argument"),
                ));
            };
            let callback_ty =
                signatures.canonical_type(&type_of_sequence_callback(&args[1], env, signatures)?);
            if name == "map" {
                let Type::Function { params, returns } = callback_ty else {
                    return Err(diag(args[1].span, "map callback must be a function"));
                };
                if params != vec![(*element).clone()] || returns.len() != 1 {
                    return Err(diag(
                        args[1].span,
                        &format!("map callback must have type fn({}) -> U", element.name()),
                    ));
                }
                let output = signatures.canonical_type(&returns[0]);
                if matches!(output, Type::Void | Type::Function { .. }) {
                    return Err(diag(
                        args[1].span,
                        &format!("map cannot produce {} list elements", output.name()),
                    ));
                }
                Ok(Type::List(Box::new(output)))
            } else {
                let expected = Type::Function {
                    params: vec![(*element).clone()],
                    returns: vec![Type::Bool],
                };
                require_type(
                    args[1].span,
                    &expected,
                    &callback_ty,
                    &format!("{name} callback"),
                )?;
                Ok(Type::List(element))
            }
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "fold" || name == "reduce" => {
            if !named_args.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!("{name} does not accept named arguments"),
                ));
            }
            let expected_len = if name == "fold" { 3 } else { 2 };
            if args.len() != expected_len {
                return Err(diag(
                    expr.span,
                    &format!("{name} expects exactly {expected_len} arguments"),
                ));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            let Type::List(element) = list_ty else {
                return Err(diag(
                    args[0].span,
                    &format!("{name} expects a list as its first argument"),
                ));
            };
            let (result_ty, reducer_index, expected_params) = if name == "fold" {
                let initial_ty =
                    signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
                (
                    initial_ty.clone(),
                    2usize,
                    vec![initial_ty, (*element).clone()],
                )
            } else {
                (
                    (*element).clone(),
                    1usize,
                    vec![(*element).clone(), (*element).clone()],
                )
            };
            let reducer_ty =
                signatures.canonical_type(&type_of_expr(&args[reducer_index], env, signatures)?);
            let expected_reducer = Type::Function {
                params: expected_params,
                returns: vec![result_ty.clone()],
            };
            require_type(
                args[reducer_index].span,
                &expected_reducer,
                &reducer_ty,
                &format!("{name} reducer"),
            )?;
            Ok(result_ty)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "any" || name == "every" => {
            if !named_args.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!("{name} does not accept named arguments"),
                ));
            }
            if args.len() != 1 {
                return Err(diag(
                    expr.span,
                    &format!("{name} expects exactly one bool[] argument"),
                ));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            require_type(
                args[0].span,
                &Type::List(Box::new(Type::Bool)),
                &list_ty,
                &format!("{name} input"),
            )?;
            Ok(Type::Bool)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "take" || name == "skip" => {
            if !named_args.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!("{name} does not accept named arguments"),
                ));
            }
            if args.len() != 2 {
                return Err(diag(
                    expr.span,
                    &format!("{name} expects exactly two arguments: a list and an i64 count"),
                ));
            }
            let list_ty = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            if !matches!(list_ty, Type::List(_)) {
                return Err(diag(
                    args[0].span,
                    &format!(
                        "{name} expects a list as its first argument, got {}",
                        list_ty.name()
                    ),
                ));
            }
            let count_ty = type_of_expr(&args[1], env, signatures)?;
            require_type(
                args[1].span,
                &Type::I64,
                &count_ty,
                &format!("{name} count"),
            )?;
            Ok(list_ty)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "print" => {
            if !named_args.is_empty() {
                return Err(diag(expr.span, "print does not accept named arguments"));
            }
            if args.len() != 1 {
                return Err(diag(expr.span, "print expects exactly one argument"));
            }
            let ty = type_of_expr(&args[0], env, signatures)?;
            if !matches!(ty, Type::I64 | Type::Bool | Type::Str | Type::Error) {
                return Err(diag(
                    expr.span,
                    &format!("print does not support {}", ty.name()),
                ));
            }
            Ok(Type::Void)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "error" => {
            if !named_args.is_empty() {
                return Err(diag(expr.span, "error does not accept named arguments"));
            }
            if args.len() != 1 {
                return Err(diag(expr.span, "error expects exactly one string argument"));
            }
            let ty = type_of_expr(&args[0], env, signatures)?;
            require_type(expr.span, &Type::Str, &ty, "error message")?;
            Ok(Type::Error)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if signatures.interface(name).is_some() => {
            let interface = signatures
                .interface(name)
                .expect("interface was checked above");
            require_visible_declaration(
                expr.span,
                interface.span,
                interface.public,
                "interface",
                name,
                signatures,
            )?;
            if !named_args.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!(
                        "interface value conversion '{name}(...)' does not accept named arguments"
                    ),
                ));
            }
            if args.len() != 1 {
                return Err(diag(
                    expr.span,
                    &format!(
                        "interface value conversion '{name}(...)' expects exactly one concrete value"
                    ),
                ));
            }
            let concrete = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
            let Type::Named(target_name) = concrete else {
                return Err(diag(
                    args[0].span,
                    &format!("interface '{name}' can only pack a concrete struct or enum value"),
                ));
            };
            if target_name == *name && signatures.interface(&target_name).is_some() {
                return Err(diag(
                    args[0].span,
                    &format!("value already has interface type '{name}'"),
                ));
            }
            if signatures.implementation(name, &target_name).is_none() {
                return Err(diag(
                    args[0].span,
                    &format!("type '{target_name}' does not implement interface '{name}'"),
                ));
            }
            Ok(Type::Named(name.clone()))
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
            let returns = check_call(expr.span, name, args, named_args, env, signatures)?;
            match returns.as_slice() {
                [] => Ok(Type::Void),
                [ty] => Ok(ty.clone()),
                _ => Err(diag(
                    expr.span,
                    &format!(
                        "function '{name}' returns {} values; use a destructuring binding",
                        returns.len()
                    ),
                )),
            }
        }
        ExprKind::QualifiedCall {
            namespace, name, ..
        } => {
            let returns = check_qualified_call(expr, env, signatures)?;
            match returns.as_slice() {
                [] => Ok(Type::Void),
                [ty] => Ok(ty.clone()),
                _ => Err(diag(
                    expr.span,
                    &format!(
                        "qualified call '{namespace}.{name}' returns {} values; use a destructuring binding",
                        returns.len()
                    ),
                )),
            }
        }
        ExprKind::StructLiteral {
            name,
            name_span,
            base,
            fields,
        } => {
            let Some(definition) = signatures.struct_type(name) else {
                return Err(diag(*name_span, &format!("unknown struct '{name}'")));
            };
            require_visible_declaration(
                *name_span,
                definition.span,
                definition.public,
                "struct",
                name,
                signatures,
            )?;
            if let Some(base) = base {
                let base_ty = type_of_expr(base, env, signatures)?;
                require_type(
                    base.span,
                    &Type::Named(name.clone()),
                    &base_ty,
                    &format!("{name} update base"),
                )?;
            }
            let mut seen = HashSet::new();
            for field in fields {
                if !seen.insert(field.name.as_str()) {
                    return Err(diag(
                        field.name_span,
                        &format!("duplicate field '{}' in {name} literal", field.name),
                    ));
                }
                let Some(expected) = definition.field(&field.name) else {
                    return Err(diag(
                        field.name_span,
                        &format!("struct '{name}' has no field '{}'", field.name),
                    )
                    .with_label(definition.span, format!("'{name}' is declared here")));
                };
                let actual = type_of_expr(&field.value, env, signatures)?;
                require_type(
                    field.value.span,
                    &expected.ty,
                    &actual,
                    &format!("field '{}.{}'", name, field.name),
                )?;
            }
            let missing = definition
                .fields
                .iter()
                .filter(|field| !seen.contains(field.name.as_str()))
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>();
            if base.is_none() && !missing.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!(
                        "{name} literal is missing field{} {}",
                        if missing.len() == 1 { "" } else { "s" },
                        missing.join(", ")
                    ),
                ));
            }
            Ok(Type::Named(name.clone()))
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            let cond_ty = type_of_expr(cond, env, signatures)?;
            require_type(
                cond.span,
                &Type::Bool,
                &cond_ty,
                "conditional expression condition",
            )?;
            let then_ty = type_of_expr(then_expr, env, signatures)?;
            let else_ty = type_of_expr(else_expr, env, signatures)?;
            if then_ty == Type::Void || else_ty == Type::Void {
                return Err(diag(
                    expr.span,
                    "conditional expression branches cannot produce void",
                ));
            }
            require_type(
                else_expr.span,
                &then_ty,
                &else_ty,
                "conditional expression branch",
            )?;
            Ok(then_ty)
        }
        ExprKind::Match { value, arms } => {
            let value_ty = type_of_expr(value, env, signatures)?;
            let Type::Named(enum_name) = value_ty else {
                return Err(diag(
                    value.span,
                    &format!("match requires an enum value, got {}", value_ty.name()),
                ));
            };
            let Some(definition) = signatures.enum_type(&enum_name) else {
                return Err(diag(
                    value.span,
                    &format!("match requires an enum value, got {enum_name}"),
                ));
            };
            let mut covered = HashSet::new();
            let mut result_ty: Option<Type> = None;
            for arm in arms {
                if arm.enum_name != enum_name {
                    return Err(diag(
                        arm.enum_span,
                        &format!(
                            "match arm uses enum '{}', expected '{enum_name}'",
                            arm.enum_name
                        ),
                    ));
                }
                let Some(variant) = definition.variant(&arm.variant) else {
                    return Err(diag(
                        arm.variant_span,
                        &format!("enum '{enum_name}' has no variant '{}'", arm.variant),
                    ));
                };
                if covered.contains(arm.variant.as_str()) {
                    return Err(diag(
                        arm.variant_span,
                        &format!(
                            "duplicate match arm for '{enum_name}.{}' is unreachable; an earlier unguarded arm already covers this variant",
                            arm.variant
                        ),
                    ));
                }
                if arm.patterns.len() != variant.payloads.len() {
                    return Err(diag(
                        arm.span,
                        &format!(
                            "match arm '{}.{}' expects {} payload pattern{}, got {}",
                            enum_name,
                            arm.variant,
                            variant.payloads.len(),
                            if variant.payloads.len() == 1 { "" } else { "s" },
                            arm.patterns.len()
                        ),
                    ));
                }
                let mut nested = env.clone();
                for (pattern, payload_ty) in arm.patterns.iter().zip(&variant.payloads) {
                    match pattern {
                        MatchPattern::Binding(binding) => {
                            if binding.name == "_" {
                                continue;
                            }
                            if nested.contains_key(&binding.name) {
                                return Err(diag(
                                    binding.span,
                                    &format!(
                                        "match binding '{}' shadows an existing binding",
                                        binding.name
                                    ),
                                ));
                            }
                            nested.insert(binding.name.clone(), payload_ty.clone());
                        }
                        MatchPattern::Struct(pattern) => {
                            let expected = signatures
                                .canonical_type(&Type::Named(pattern.struct_name.clone()));
                            let actual = signatures.canonical_type(payload_ty);
                            require_type(
                                pattern.struct_span,
                                &expected,
                                &actual,
                                "match struct pattern",
                            )?;
                            let Type::Named(struct_name) = expected else {
                                return Err(diag(
                                    pattern.struct_span,
                                    &format!(
                                        "struct pattern '{}' does not name a struct type",
                                        pattern.struct_name
                                    ),
                                ));
                            };
                            if signatures.struct_type(&struct_name).is_none() {
                                return Err(diag(
                                    pattern.struct_span,
                                    &format!(
                                        "struct pattern '{}' does not name a struct type",
                                        pattern.struct_name
                                    ),
                                ));
                            }
                            let mut diagnostics = Vec::new();
                            bind_struct_pattern_fields(
                                &pattern.fields,
                                &struct_name,
                                &mut nested,
                                signatures,
                                &mut diagnostics,
                            );
                            if let Some(diagnostic) = diagnostics.into_iter().next() {
                                return Err(diagnostic);
                            }
                        }
                        MatchPattern::Relational(_) | MatchPattern::Logical { .. } => {
                            validate_relational_match_pattern(pattern, payload_ty, signatures)?;
                        }
                    }
                }
                if let Some(guard) = &arm.guard {
                    let guard_ty = type_of_expr(guard, &nested, signatures)?;
                    require_type(guard.span, &Type::Bool, &guard_ty, "match guard")?;
                } else if arm.patterns.iter().all(match_pattern_is_irrefutable) {
                    covered.insert(arm.variant.as_str());
                }
                let arm_ty = type_of_expr(&arm.value, &nested, signatures)?;
                if arm_ty == Type::Void {
                    return Err(diag(
                        arm.value.span,
                        "match expression arms cannot produce void",
                    ));
                }
                if let Some(expected) = &result_ty {
                    require_type(arm.value.span, expected, &arm_ty, "match expression arm")?;
                } else {
                    result_ty = Some(arm_ty);
                }
            }
            let missing = definition
                .variants
                .iter()
                .filter(|variant| !covered.contains(variant.name.as_str()))
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!(
                        "non-exhaustive match on '{enum_name}'; missing {}",
                        missing.join(", ")
                    ),
                ));
            }
            result_ty.ok_or_else(|| diag(expr.span, "match expression requires at least one arm"))
        }
        ExprKind::ListMatch { value, arms } => {
            let value_ty = signatures.canonical_type(&type_of_expr(value, env, signatures)?);
            let Type::List(element_ty) = value_ty else {
                return Err(diag(
                    value.span,
                    &format!("list match requires a list value, got {}", value_ty.name()),
                ));
            };
            let patterns = arms
                .iter()
                .map(|arm| (&arm.pattern, arm.guard.is_some()))
                .collect::<Vec<_>>();
            validate_list_match_coverage(&patterns, expr.span)?;
            let mut result_ty: Option<Type> = None;
            for arm in arms {
                let mut nested = env.clone();
                bind_list_match_pattern(&arm.pattern, &element_ty, &mut nested)?;
                if let Some(guard) = &arm.guard {
                    let guard_ty = type_of_expr(guard, &nested, signatures)?;
                    require_type(guard.span, &Type::Bool, &guard_ty, "match guard")?;
                }
                let arm_ty = type_of_expr(&arm.value, &nested, signatures)?;
                if arm_ty == Type::Void {
                    return Err(diag(
                        arm.value.span,
                        "match expression arms cannot produce void",
                    ));
                }
                if let Some(expected) = &result_ty {
                    require_type(arm.value.span, expected, &arm_ty, "match expression arm")?;
                } else {
                    result_ty = Some(arm_ty);
                }
            }
            result_ty.ok_or_else(|| diag(expr.span, "match expression requires at least one arm"))
        }
        ExprKind::Field {
            base,
            name,
            name_span,
            optional,
        } => {
            let base_ty = signatures.canonical_type(&type_of_expr(base, env, signatures)?);
            let (base_ty, optional_result) = if *optional {
                let Type::Optional(inner) = base_ty else {
                    return Err(diag(
                        base.span,
                        &format!(
                            "optional-aware '?.' access requires an optional receiver, got {}",
                            base_ty.name()
                        ),
                    ));
                };
                if *inner == Type::Void {
                    return Err(diag(
                        base.span,
                        "optional-aware '?.' access on 'none' needs a concrete optional receiver type",
                    ));
                }
                (*inner, true)
            } else {
                (base_ty, false)
            };
            let field_ty = if let Type::List(element) = &base_ty {
                match name.as_str() {
                    "length" => Type::I64,
                    "isEmpty" | "isNotEmpty" => Type::Bool,
                    "first" | "last" | "single" => (**element).clone(),
                    _ => {
                        return Err(diag(
                            *name_span,
                            &format!("list type '{}' has no property '{name}'", base_ty.name()),
                        ));
                    }
                }
            } else {
                let Type::Named(struct_name) = base_ty else {
                    return Err(diag(
                        *name_span,
                        &format!(
                            "field access requires a struct or list value, got {}",
                            base_ty.name()
                        ),
                    ));
                };
                let Some(definition) = signatures.struct_type(&struct_name) else {
                    return Err(diag(base.span, &format!("unknown struct '{struct_name}'")));
                };
                definition
                    .field(name)
                    .map(|field| field.ty.clone())
                    .ok_or_else(|| {
                        diag(
                            *name_span,
                            &format!("struct '{struct_name}' has no field '{name}'"),
                        )
                        .with_label(definition.span, format!("'{struct_name}' is declared here"))
                    })?
            };
            if optional_result {
                let field_ty = signatures.canonical_type(&field_ty);
                if matches!(field_ty, Type::Optional(_)) {
                    Ok(field_ty)
                } else {
                    Ok(Type::Optional(Box::new(field_ty)))
                }
            } else {
                Ok(field_ty)
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let ty = type_of_expr(inner, env, signatures)?;
            match op {
                UnaryOp::Neg => {
                    require_type(expr.span, &Type::I64, &ty, "unary '-'")?;
                    Ok(Type::I64)
                }
                UnaryOp::Not => {
                    require_type(expr.span, &Type::Bool, &ty, "unary '!'")?;
                    Ok(Type::Bool)
                }
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left_ty = type_of_expr(left, env, signatures)?;
            let right_ty = type_of_expr(right, env, signatures)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    require_type(expr.span, &Type::I64, &left_ty, "left arithmetic operand")?;
                    require_type(expr.span, &Type::I64, &right_ty, "right arithmetic operand")?;
                    Ok(Type::I64)
                }
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    require_type(expr.span, &Type::I64, &left_ty, "left comparison operand")?;
                    require_type(expr.span, &Type::I64, &right_ty, "right comparison operand")?;
                    Ok(Type::Bool)
                }
                BinOp::Eq | BinOp::Ne => {
                    if left_ty == Type::Void || right_ty == Type::Void {
                        return Err(diag(expr.span, "void values cannot be compared"));
                    }
                    let left_canonical = signatures.canonical_type(&left_ty);
                    let right_canonical = signatures.canonical_type(&right_ty);
                    if matches!(left_canonical, Type::Optional(_))
                        || matches!(right_canonical, Type::Optional(_))
                    {
                        let compares_none = matches!(
                            (&left_canonical, &right_canonical),
                            (Type::Optional(left), Type::Optional(right))
                                if **left == Type::Void || **right == Type::Void
                        );
                        if compares_none {
                            return Ok(Type::Bool);
                        }
                        return Err(diag(
                            expr.span,
                            "optional equality is only defined for presence checks against 'none'; unwrap with 'if let' or '??' before comparing values",
                        ));
                    }
                    if matches!(left_canonical, Type::Named(_))
                        || matches!(right_canonical, Type::Named(_))
                    {
                        return Err(diag(
                            expr.span,
                            "whole-struct equality is not defined yet; compare fields explicitly",
                        ));
                    }
                    require_type(expr.span, &left_ty, &right_ty, "equality operand")?;
                    Ok(Type::Bool)
                }
                BinOp::And | BinOp::Or => {
                    require_type(expr.span, &Type::Bool, &left_ty, "left boolean operand")?;
                    require_type(expr.span, &Type::Bool, &right_ty, "right boolean operand")?;
                    Ok(Type::Bool)
                }
                BinOp::Coalesce => {
                    let left_ty = signatures.canonical_type(&left_ty);
                    let right_ty = signatures.canonical_type(&right_ty);
                    let Type::Optional(inner) = left_ty else {
                        return Err(diag(
                            left.span,
                            &format!(
                                "left operand of '??' must be optional, got {}",
                                left_ty.name()
                            ),
                        ));
                    };
                    if *inner == Type::Void {
                        if matches!(right_ty, Type::Optional(ref right) if **right == Type::Void) {
                            return Err(diag(
                                expr.span,
                                "'none ?? none' has no concrete optional value type",
                            ));
                        }
                        return Ok(right_ty);
                    }
                    if right_ty == *inner {
                        Ok(*inner)
                    } else if right_ty == Type::Optional(inner.clone())
                        || matches!(right_ty, Type::Optional(ref right) if **right == Type::Void)
                    {
                        Ok(Type::Optional(inner))
                    } else {
                        Err(diag(
                            right.span,
                            &format!(
                                "fallback of '??' must be {} or {}?, got {}",
                                inner.name(),
                                inner.name(),
                                right_ty.name()
                            ),
                        ))
                    }
                }
            }
        }
    }
}

pub(crate) fn value_types_of_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<Type>, Diagnostic> {
    match &expr.kind {
        ExprKind::ShellCall { name, args, .. } => {
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: args.clone(),
                    named_args: Vec::new(),
                },
            };
            value_types_of_expr(&call, env, signatures)
        }
        ExprKind::Pipe {
            input,
            name,
            args,
            optional,
            ..
        } => {
            if *optional {
                return Ok(vec![type_of_optional_pipe(
                    expr, input, name, args, env, signatures,
                )?]);
            }
            let mut call_args = Vec::with_capacity(args.len() + 1);
            call_args.push(pipe_input_expr(input, env, signatures));
            call_args.extend(args.iter().cloned());
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: call_args,
                    named_args: Vec::new(),
                },
            };
            value_types_of_expr(&call, env, signatures)
        }
        ExprKind::Call { name, .. }
            if signatures.interface(name).is_some()
                || matches!(
                    name.as_str(),
                    "take"
                        | "skip"
                        | "any"
                        | "every"
                        | "fold"
                        | "reduce"
                        | "map"
                        | "filter"
                        | "where"
                        | "concat"
                        | "distinct"
                        | "flatten"
                        | "sorted"
                        | "chunked"
                ) =>
        {
            Ok(vec![type_of_expr(expr, env, signatures)?])
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name != "print" && name != "error" => {
            check_call(expr.span, name, args, named_args, env, signatures)
        }
        ExprKind::QualifiedCall { .. } => check_qualified_call(expr, env, signatures),
        _ => Ok(vec![type_of_expr(expr, env, signatures)?]),
    }
}

fn check_qualified_call(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<Type>, Diagnostic> {
    let ExprKind::QualifiedCall {
        namespace,
        namespace_span,
        name,
        name_span,
        args,
        named_args,
    } = &expr.kind
    else {
        return Err(diag(expr.span, "expected a qualified call"));
    };
    let span = expr.span;
    if namespace == "process" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("process.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "pid" | "parentPid" | "cpuMillis" | "peakResidentMemoryBytes" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("process.{name} expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(vec![Type::I64]);
            }
            "terminationRequested" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!(
                            "process.terminationRequested expects 0 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                return Ok(vec![Type::Bool]);
            }
            "exit" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("process.exit expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::I64, &actual, "process.exit code")?;
                if matches!(
                    constant_primitive_value(&args[0], signatures),
                    Some(ConstantValue::I64(value)) if !(0..=255).contains(&value)
                ) {
                    return Err(diag(
                        args[0].span,
                        "process.exit code must be between 0 and 255",
                    ));
                }
                return Ok(Vec::new());
            }
            "hasEnv" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("process.hasEnv expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &actual, "process.hasEnv name")?;
                return Ok(vec![Type::Bool]);
            }
            "env" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("process.env expects 2 arguments, got {}", args.len()),
                    ));
                }
                let name_type = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &name_type, "process.env name")?;
                let fallback_type = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::Str,
                    &fallback_type,
                    "process.env fallback",
                )?;
                return Ok(vec![Type::Str]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("process module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "net" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("net.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "tcpConnect" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("net.tcpConnect expects 2 arguments, got {}", args.len()),
                    ));
                }
                let host = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &host, "net.tcpConnect host")?;
                let port = type_of_expr(&args[1], env, signatures)?;
                require_type(args[1].span, &Type::I64, &port, "net.tcpConnect port")?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(1..=65535).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "net.tcpConnect port must be between 1 and 65535",
                    ));
                }
                return Ok(vec![Type::I64, Type::Error]);
            }
            "udpConnect" | "udpBind" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("net.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let host = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &host, &format!("net.{name} host"))?;
                let port = type_of_expr(&args[1], env, signatures)?;
                require_type(args[1].span, &Type::I64, &port, &format!("net.{name} port"))?;
                let minimum = if name == "udpBind" { 0 } else { 1 };
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if value < minimum || value > 65535
                ) {
                    return Err(diag(
                        args[1].span,
                        &format!("net.{name} port must be between {minimum} and 65535"),
                    ));
                }
                return Ok(vec![Type::I64, Type::Error]);
            }
            "tcpListen" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!("net.tcpListen expects 3 arguments, got {}", args.len()),
                    ));
                }
                let host = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &host, "net.tcpListen host")?;
                let port = type_of_expr(&args[1], env, signatures)?;
                require_type(args[1].span, &Type::I64, &port, "net.tcpListen port")?;
                let backlog = type_of_expr(&args[2], env, signatures)?;
                require_type(args[2].span, &Type::I64, &backlog, "net.tcpListen backlog")?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(0..=65535).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "net.tcpListen port must be between 0 and 65535",
                    ));
                }
                if matches!(
                    constant_primitive_value(&args[2], signatures),
                    Some(ConstantValue::I64(value)) if value < 1 || value > i32::MAX as i64
                ) {
                    return Err(diag(
                        args[2].span,
                        "net.tcpListen backlog must be between 1 and 2147483647",
                    ));
                }
                return Ok(vec![Type::I64, Type::Error]);
            }
            "tcpAccept" | "localPort" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("net.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &handle,
                    &format!("net.{name} socket"),
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "peerAddress" | "localAddress" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("net.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &handle,
                    &format!("net.{name} socket"),
                )?;
                let callback = signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::Str, Type::I64],
                    returns: Vec::new(),
                };
                require_type(
                    args[1].span,
                    &expected,
                    &callback,
                    &format!("net.{name} callback"),
                )?;
                return Ok(vec![Type::Error]);
            }
            "setNonblocking" | "setNoDelay" | "setKeepAlive" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("net.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &handle,
                    &format!("net.{name} socket"),
                )?;
                let enabled = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::Bool,
                    &enabled,
                    &format!("net.{name} enabled"),
                )?;
                return Ok(vec![Type::Error]);
            }
            "waitReadable" | "waitWritable" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("net.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &handle,
                    &format!("net.{name} socket"),
                )?;
                let timeout = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &timeout,
                    &format!("net.{name} timeoutMillis"),
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(-1..=i32::MAX as i64).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        &format!("net.{name} timeoutMillis must be -1 or between 0 and 2147483647"),
                    ));
                }
                return Ok(vec![Type::Bool, Type::Error]);
            }
            "waitReadableMany" | "waitWritableMany" | "waitReadyMany" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!("net.{name} expects 3 arguments, got {}", args.len()),
                    ));
                }
                let sockets = signatures.canonical_type(&type_of_expr(&args[0], env, signatures)?);
                require_type(
                    args[0].span,
                    &Type::List(Box::new(Type::I64)),
                    &sockets,
                    &format!("net.{name} sockets"),
                )?;
                let timeout = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &timeout,
                    &format!("net.{name} timeoutMillis"),
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(-1..=i32::MAX as i64).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        &format!("net.{name} timeoutMillis must be -1 or between 0 and 2147483647"),
                    ));
                }
                let callback = signatures.canonical_type(&type_of_expr(&args[2], env, signatures)?);
                let expected = Type::Function {
                    params: if name == "waitReadyMany" {
                        vec![Type::I64, Type::Bool, Type::Bool]
                    } else {
                        vec![Type::I64]
                    },
                    returns: Vec::new(),
                };
                require_type(
                    args[2].span,
                    &expected,
                    &callback,
                    &format!("net.{name} callback"),
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "sendText" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("net.sendText expects 2 arguments, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::I64, &handle, "net.sendText socket")?;
                let text = type_of_expr(&args[1], env, signatures)?;
                require_type(args[1].span, &Type::Str, &text, "net.sendText text")?;
                return Ok(vec![Type::Error]);
            }
            "sendTextTo" => {
                if args.len() != 4 {
                    return Err(diag(
                        span,
                        &format!("net.sendTextTo expects 4 arguments, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::I64, &handle, "net.sendTextTo socket")?;
                let host = type_of_expr(&args[1], env, signatures)?;
                require_type(args[1].span, &Type::Str, &host, "net.sendTextTo host")?;
                let port = type_of_expr(&args[2], env, signatures)?;
                require_type(args[2].span, &Type::I64, &port, "net.sendTextTo port")?;
                if matches!(
                    constant_primitive_value(&args[2], signatures),
                    Some(ConstantValue::I64(value)) if !(1..=65535).contains(&value)
                ) {
                    return Err(diag(
                        args[2].span,
                        "net.sendTextTo port must be between 1 and 65535",
                    ));
                }
                let text = type_of_expr(&args[3], env, signatures)?;
                require_type(args[3].span, &Type::Str, &text, "net.sendTextTo text")?;
                return Ok(vec![Type::Error]);
            }
            "receiveText" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!("net.receiveText expects 3 arguments, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::I64, &handle, "net.receiveText socket")?;
                let max_bytes = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &max_bytes,
                    "net.receiveText maxBytes",
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(1..=65536).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "net.receiveText maxBytes must be between 1 and 65536",
                    ));
                }
                let callback = signatures.canonical_type(&type_of_expr(&args[2], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::I64, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[2].span,
                    &expected,
                    &callback,
                    "net.receiveText callback",
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "receiveTextFrom" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!(
                            "net.receiveTextFrom expects 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &handle,
                    "net.receiveTextFrom socket",
                )?;
                let max_bytes = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &max_bytes,
                    "net.receiveTextFrom maxBytes",
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(1..=65536).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "net.receiveTextFrom maxBytes must be between 1 and 65536",
                    ));
                }
                let callback = signatures.canonical_type(&type_of_expr(&args[2], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str, Type::I64],
                    returns: Vec::new(),
                };
                require_type(
                    args[2].span,
                    &expected,
                    &callback,
                    "net.receiveTextFrom callback",
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "shutdownRead" | "shutdownWrite" | "close" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("net.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let handle = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &handle,
                    &format!("net.{name} socket"),
                )?;
                return Ok(vec![Type::Error]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("net module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "http" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("http.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "receiveRequestHead" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!(
                            "http.receiveRequestHead expects 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.receiveRequestHead socket",
                )?;
                let max_bytes = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &max_bytes,
                    "http.receiveRequestHead maxBytes",
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(1..=65536).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "http.receiveRequestHead maxBytes must be between 1 and 65536",
                    ));
                }
                let callback = signatures.canonical_type(&type_of_expr(&args[2], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[2].span,
                    &expected,
                    &callback,
                    "http.receiveRequestHead callback",
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "receiveRequestHeadWithHeaders" => {
                if args.len() != 4 {
                    return Err(diag(
                        span,
                        &format!(
                            "http.receiveRequestHeadWithHeaders expects 4 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.receiveRequestHeadWithHeaders socket",
                )?;
                let max_bytes = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &max_bytes,
                    "http.receiveRequestHeadWithHeaders maxBytes",
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(1..=65536).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "http.receiveRequestHeadWithHeaders maxBytes must be between 1 and 65536",
                    ));
                }
                let request_callback =
                    signatures.canonical_type(&type_of_expr(&args[2], env, signatures)?);
                let expected_request_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[2].span,
                    &expected_request_callback,
                    &request_callback,
                    "http.receiveRequestHeadWithHeaders requestCallback",
                )?;
                let header_callback =
                    signatures.canonical_type(&type_of_expr(&args[3], env, signatures)?);
                let expected_header_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[3].span,
                    &expected_header_callback,
                    &header_callback,
                    "http.receiveRequestHeadWithHeaders headerCallback",
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "receiveRequestWithTextBody" => {
                if args.len() != 6 {
                    return Err(diag(
                        span,
                        &format!(
                            "http.receiveRequestWithTextBody expects 6 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.receiveRequestWithTextBody socket",
                )?;
                for (index, label, minimum) in [
                    (1usize, "maxHeadBytes", 1i64),
                    (2usize, "maxBodyBytes", 0i64),
                ] {
                    let limit = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::I64,
                        &limit,
                        &format!("http.receiveRequestWithTextBody {label}"),
                    )?;
                    if matches!(
                        constant_primitive_value(&args[index], signatures),
                        Some(ConstantValue::I64(value)) if value < minimum || value > 65536
                    ) {
                        return Err(diag(
                            args[index].span,
                            &format!(
                                "http.receiveRequestWithTextBody {label} must be between {minimum} and 65536"
                            ),
                        ));
                    }
                }
                let request_callback =
                    signatures.canonical_type(&type_of_expr(&args[3], env, signatures)?);
                let expected_request_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[3].span,
                    &expected_request_callback,
                    &request_callback,
                    "http.receiveRequestWithTextBody requestCallback",
                )?;
                let header_callback =
                    signatures.canonical_type(&type_of_expr(&args[4], env, signatures)?);
                let expected_header_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[4].span,
                    &expected_header_callback,
                    &header_callback,
                    "http.receiveRequestWithTextBody headerCallback",
                )?;
                let body_callback =
                    signatures.canonical_type(&type_of_expr(&args[5], env, signatures)?);
                let expected_body_callback = Type::Function {
                    params: vec![Type::I64, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[5].span,
                    &expected_body_callback,
                    &body_callback,
                    "http.receiveRequestWithTextBody bodyCallback",
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "receiveResponseHeadWithHeaders" => {
                if args.len() != 4 {
                    return Err(diag(
                        span,
                        &format!(
                            "http.receiveResponseHeadWithHeaders expects 4 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.receiveResponseHeadWithHeaders socket",
                )?;
                let max_bytes = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &max_bytes,
                    "http.receiveResponseHeadWithHeaders maxBytes",
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(1..=65536).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "http.receiveResponseHeadWithHeaders maxBytes must be between 1 and 65536",
                    ));
                }
                let response_callback =
                    signatures.canonical_type(&type_of_expr(&args[2], env, signatures)?);
                let expected_response_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::I64, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[2].span,
                    &expected_response_callback,
                    &response_callback,
                    "http.receiveResponseHeadWithHeaders responseCallback",
                )?;
                let header_callback =
                    signatures.canonical_type(&type_of_expr(&args[3], env, signatures)?);
                let expected_header_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[3].span,
                    &expected_header_callback,
                    &header_callback,
                    "http.receiveResponseHeadWithHeaders headerCallback",
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "receiveResponseWithTextBody" => {
                if args.len() != 6 {
                    return Err(diag(
                        span,
                        &format!(
                            "http.receiveResponseWithTextBody expects 6 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.receiveResponseWithTextBody socket",
                )?;
                for (index, label, minimum) in [
                    (1usize, "maxHeadBytes", 1i64),
                    (2usize, "maxBodyBytes", 0i64),
                ] {
                    let limit = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::I64,
                        &limit,
                        &format!("http.receiveResponseWithTextBody {label}"),
                    )?;
                    if matches!(
                        constant_primitive_value(&args[index], signatures),
                        Some(ConstantValue::I64(value)) if value < minimum || value > 65536
                    ) {
                        return Err(diag(
                            args[index].span,
                            &format!(
                                "http.receiveResponseWithTextBody {label} must be between {minimum} and 65536"
                            ),
                        ));
                    }
                }
                let response_callback =
                    signatures.canonical_type(&type_of_expr(&args[3], env, signatures)?);
                let expected_response_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::I64, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[3].span,
                    &expected_response_callback,
                    &response_callback,
                    "http.receiveResponseWithTextBody responseCallback",
                )?;
                let header_callback =
                    signatures.canonical_type(&type_of_expr(&args[4], env, signatures)?);
                let expected_header_callback = Type::Function {
                    params: vec![Type::I64, Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[4].span,
                    &expected_header_callback,
                    &header_callback,
                    "http.receiveResponseWithTextBody headerCallback",
                )?;
                let body_callback =
                    signatures.canonical_type(&type_of_expr(&args[5], env, signatures)?);
                let expected_body_callback = Type::Function {
                    params: vec![Type::I64, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[5].span,
                    &expected_body_callback,
                    &body_callback,
                    "http.receiveResponseWithTextBody bodyCallback",
                )?;
                return Ok(vec![Type::I64, Type::Error]);
            }
            "sendTextRequest" => {
                if !(6..=7).contains(&args.len()) {
                    return Err(diag(
                        span,
                        &format!(
                            "http.sendTextRequest expects 6 or 7 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.sendTextRequest socket",
                )?;
                for (index, label) in [
                    (1usize, "method"),
                    (2usize, "target"),
                    (3usize, "host"),
                    (4usize, "contentType"),
                    (5usize, "body"),
                ] {
                    let value = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::Str,
                        &value,
                        &format!("http.sendTextRequest {label}"),
                    )?;
                }
                if args.len() == 7 {
                    let keep_alive = type_of_expr(&args[6], env, signatures)?;
                    require_type(
                        args[6].span,
                        &Type::Bool,
                        &keep_alive,
                        "http.sendTextRequest keepAlive",
                    )?;
                }
                return Ok(vec![Type::Error]);
            }
            "sendTextRequestWithHeaders" => {
                if !(7..=8).contains(&args.len()) {
                    return Err(diag(
                        span,
                        &format!(
                            "http.sendTextRequestWithHeaders expects 7 or 8 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.sendTextRequestWithHeaders socket",
                )?;
                for (index, label) in [
                    (1usize, "method"),
                    (2usize, "target"),
                    (3usize, "host"),
                    (4usize, "contentType"),
                    (5usize, "body"),
                    (6usize, "headers"),
                ] {
                    let value = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::Str,
                        &value,
                        &format!("http.sendTextRequestWithHeaders {label}"),
                    )?;
                }
                if args.len() == 8 {
                    let keep_alive = type_of_expr(&args[7], env, signatures)?;
                    require_type(
                        args[7].span,
                        &Type::Bool,
                        &keep_alive,
                        "http.sendTextRequestWithHeaders keepAlive",
                    )?;
                }
                return Ok(vec![Type::Error]);
            }
            "sendTextResponseWithHeaders" => {
                if !(5..=6).contains(&args.len()) {
                    return Err(diag(
                        span,
                        &format!(
                            "http.sendTextResponseWithHeaders expects 5 or 6 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.sendTextResponseWithHeaders socket",
                )?;
                let status = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &status,
                    "http.sendTextResponseWithHeaders status",
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(100..=599).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "http.sendTextResponseWithHeaders status must be between 100 and 599",
                    ));
                }
                for (index, label) in [
                    (2usize, "contentType"),
                    (3usize, "body"),
                    (4usize, "headers"),
                ] {
                    let value = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::Str,
                        &value,
                        &format!("http.sendTextResponseWithHeaders {label}"),
                    )?;
                }
                if args.len() == 6 {
                    let keep_alive = type_of_expr(&args[5], env, signatures)?;
                    require_type(
                        args[5].span,
                        &Type::Bool,
                        &keep_alive,
                        "http.sendTextResponseWithHeaders keepAlive",
                    )?;
                }
                return Ok(vec![Type::Error]);
            }
            "sendTextResponse" => {
                if !(4..=5).contains(&args.len()) {
                    return Err(diag(
                        span,
                        &format!(
                            "http.sendTextResponse expects 4 or 5 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let socket = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &socket,
                    "http.sendTextResponse socket",
                )?;
                let status = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &status,
                    "http.sendTextResponse status",
                )?;
                if matches!(
                    constant_primitive_value(&args[1], signatures),
                    Some(ConstantValue::I64(value)) if !(100..=599).contains(&value)
                ) {
                    return Err(diag(
                        args[1].span,
                        "http.sendTextResponse status must be between 100 and 599",
                    ));
                }
                let content_type = type_of_expr(&args[2], env, signatures)?;
                require_type(
                    args[2].span,
                    &Type::Str,
                    &content_type,
                    "http.sendTextResponse contentType",
                )?;
                let body = type_of_expr(&args[3], env, signatures)?;
                require_type(
                    args[3].span,
                    &Type::Str,
                    &body,
                    "http.sendTextResponse body",
                )?;
                if args.len() == 5 {
                    let keep_alive = type_of_expr(&args[4], env, signatures)?;
                    require_type(
                        args[4].span,
                        &Type::Bool,
                        &keep_alive,
                        "http.sendTextResponse keepAlive",
                    )?;
                }
                return Ok(vec![Type::Error]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("http module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "url" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("url.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "parseHttp" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("url.parseHttp expects 2 arguments, got {}", args.len()),
                    ));
                }
                let value = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &value, "url.parseHttp url")?;
                let callback = signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::Str, Type::Str, Type::I64, Type::Str],
                    returns: Vec::new(),
                };
                require_type(args[1].span, &expected, &callback, "url.parseHttp callback")?;
                return Ok(vec![Type::Error]);
            }
            "parseFormQuery" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("url.parseFormQuery expects 2 arguments, got {}", args.len()),
                    ));
                }
                let value = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &value, "url.parseFormQuery query")?;
                let callback = signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::Str, Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[1].span,
                    &expected,
                    &callback,
                    "url.parseFormQuery callback",
                )?;
                return Ok(vec![Type::Error]);
            }
            "decodeComponent"
            | "encodeComponent"
            | "decodeFormComponent"
            | "encodeFormComponent" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("url.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let value = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &value,
                    &format!("url.{name} value"),
                )?;
                let callback = signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[1].span,
                    &expected,
                    &callback,
                    &format!("url.{name} callback"),
                )?;
                return Ok(vec![Type::Error]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("url module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "locale" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("locale.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "language" | "region" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("locale.{name} expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(vec![Type::Str]);
            }
            "text" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("locale.text expects 2 arguments, got {}", args.len()),
                    ));
                }
                for (index, argument) in args.iter().enumerate() {
                    let actual = type_of_expr(argument, env, signatures)?;
                    require_type(
                        argument.span,
                        &Type::Str,
                        &actual,
                        if index == 0 {
                            "locale.text key"
                        } else {
                            "locale.text fallback"
                        },
                    )?;
                }
                return Ok(vec![Type::Str]);
            }
            "select" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!("locale.select expects 3 arguments, got {}", args.len()),
                    ));
                }
                for (index, argument) in args.iter().enumerate() {
                    let actual = type_of_expr(argument, env, signatures)?;
                    require_type(
                        argument.span,
                        &Type::Str,
                        &actual,
                        match index {
                            0 => "locale.select key",
                            1 => "locale.select selector",
                            _ => "locale.select fallback",
                        },
                    )?;
                }
                return Ok(vec![Type::Str]);
            }
            "plural" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!("locale.plural expects 3 arguments, got {}", args.len()),
                    ));
                }
                let key = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &key, "locale.plural key")?;
                let count = type_of_expr(&args[1], env, signatures)?;
                require_type(args[1].span, &Type::I64, &count, "locale.plural count")?;
                let fallback = type_of_expr(&args[2], env, signatures)?;
                require_type(
                    args[2].span,
                    &Type::Str,
                    &fallback,
                    "locale.plural fallback",
                )?;
                return Ok(vec![Type::Str]);
            }
            "formatNumber" | "formatDateTime" | "formatCurrency" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("locale.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let value = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &value,
                    &format!("locale.{name} value"),
                )?;
                let callback = signatures.canonical_type(&type_of_expr(&args[1], env, signatures)?);
                let expected = Type::Function {
                    params: vec![Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[1].span,
                    &expected,
                    &callback,
                    &format!("locale.{name} callback"),
                )?;
                return Ok(vec![Type::Error]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("locale module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "time" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("time.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "unixMillis" | "monotonicMillis" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("time.{name} expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(vec![Type::I64]);
            }
            "sleepMillis" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("time.sleepMillis expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &actual,
                    "time.sleepMillis durationMs",
                )?;
                if matches!(
                    constant_primitive_value(&args[0], signatures),
                    Some(ConstantValue::I64(value)) if value < 0
                ) {
                    return Err(diag(
                        args[0].span,
                        "time.sleepMillis durationMs must be non-negative",
                    ));
                }
                return Ok(Vec::new());
            }
            "sleepUntilMonotonic" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "time.sleepUntilMonotonic expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &actual,
                    "time.sleepUntilMonotonic deadlineMillis",
                )?;
                return Ok(Vec::new());
            }
            "utcUnixMillis" => {
                if args.len() != 7 {
                    return Err(diag(
                        span,
                        &format!("time.utcUnixMillis expects 7 arguments, got {}", args.len()),
                    ));
                }
                for (arg, label) in args.iter().zip([
                    "year",
                    "month",
                    "day",
                    "hour",
                    "minute",
                    "second",
                    "millisecond",
                ]) {
                    let actual = type_of_expr(arg, env, signatures)?;
                    require_type(
                        arg.span,
                        &Type::I64,
                        &actual,
                        &format!("time.utcUnixMillis {label}"),
                    )?;
                }
                for (index, minimum, maximum, label) in [
                    (1, 1, 12, "month"),
                    (2, 1, 31, "day"),
                    (3, 0, 23, "hour"),
                    (4, 0, 59, "minute"),
                    (5, 0, 59, "second"),
                    (6, 0, 999, "millisecond"),
                ] {
                    if matches!(
                        constant_primitive_value(&args[index], signatures),
                        Some(ConstantValue::I64(value)) if value < minimum || value > maximum
                    ) {
                        return Err(diag(
                            args[index].span,
                            &format!("time.utcUnixMillis {label} must be in {minimum}..={maximum}"),
                        ));
                    }
                }
                if let (
                    Some(ConstantValue::I64(year)),
                    Some(ConstantValue::I64(month)),
                    Some(ConstantValue::I64(day)),
                ) = (
                    constant_primitive_value(&args[0], signatures),
                    constant_primitive_value(&args[1], signatures),
                    constant_primitive_value(&args[2], signatures),
                ) {
                    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                    let max_day = match month {
                        2 if leap => 29,
                        2 => 28,
                        4 | 6 | 9 | 11 => 30,
                        _ => 31,
                    };
                    if day > max_day {
                        return Err(diag(
                            args[2].span,
                            &format!(
                                "time.utcUnixMillis day {day} is invalid for year {year}, month {month}"
                            ),
                        ));
                    }
                }
                return Ok(vec![Type::I64]);
            }
            "utcYear" | "utcMonth" | "utcDay" | "utcHour" | "utcMinute" | "utcSecond"
            | "utcMillisecond" | "utcWeekday" | "utcDayOfYear" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("time.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &actual,
                    &format!("time.{name} unixMillis"),
                )?;
                return Ok(vec![Type::I64]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("time module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "fs" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("fs.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "exists" | "isFile" | "isDirectory" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("fs.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &actual,
                    &format!("fs.{name} path"),
                )?;
                return Ok(vec![Type::Bool]);
            }
            "createDirectory" | "createDirectories" | "removeFile" | "removeDirectory"
            | "removeDirectories" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("fs.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &actual,
                    &format!("fs.{name} path"),
                )?;
                return Ok(vec![Type::Error]);
            }
            "writeText" | "appendText" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("fs.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let path_type = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &path_type,
                    &format!("fs.{name} path"),
                )?;
                let text_type = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::Str,
                    &text_type,
                    &format!("fs.{name} text"),
                )?;
                return Ok(vec![Type::Error]);
            }
            "rename" | "copyFile" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("fs.{name} expects 2 arguments, got {}", args.len()),
                    ));
                }
                let source_type = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &source_type,
                    &format!("fs.{name} source"),
                )?;
                let destination_type = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::Str,
                    &destination_type,
                    &format!("fs.{name} destination"),
                )?;
                return Ok(vec![Type::Error]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("fs module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "frame" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("frame.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "request" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("frame.request expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                let expected = Type::Function {
                    params: Vec::new(),
                    returns: Vec::new(),
                };
                require_type(args[0].span, &expected, &actual, "frame.request callback")?;
                return Ok(Vec::new());
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("frame module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "clipboard" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("clipboard.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "setText" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("clipboard.setText expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &actual, "clipboard.setText text")?;
                return Ok(Vec::new());
            }
            "readText" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("clipboard.readText expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                let expected = Type::Function {
                    params: vec![Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[0].span,
                    &expected,
                    &actual,
                    "clipboard.readText callback",
                )?;
                return Ok(Vec::new());
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("clipboard module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "dialog" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("dialog.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "alert" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!("dialog.alert expects 2 arguments, got {}", args.len()),
                    ));
                }
                let title = type_of_expr(&args[0], env, signatures)?;
                let message = type_of_expr(&args[1], env, signatures)?;
                require_type(args[0].span, &Type::Str, &title, "dialog.alert title")?;
                require_type(args[1].span, &Type::Str, &message, "dialog.alert message")?;
                return Ok(Vec::new());
            }
            "confirm" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!("dialog.confirm expects 3 arguments, got {}", args.len()),
                    ));
                }
                let title = type_of_expr(&args[0], env, signatures)?;
                let message = type_of_expr(&args[1], env, signatures)?;
                let callback = type_of_expr(&args[2], env, signatures)?;
                require_type(args[0].span, &Type::Str, &title, "dialog.confirm title")?;
                require_type(args[1].span, &Type::Str, &message, "dialog.confirm message")?;
                require_type(
                    args[2].span,
                    &Type::Function {
                        params: Vec::new(),
                        returns: Vec::new(),
                    },
                    &callback,
                    "dialog.confirm callback",
                )?;
                return Ok(Vec::new());
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("dialog module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "fileDialog" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("fileDialog.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "openFile" | "saveFile" | "selectDirectory" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("fileDialog.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                let expected = Type::Function {
                    params: vec![Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[0].span,
                    &expected,
                    &actual,
                    &format!("fileDialog.{name} callback"),
                )?;
                return Ok(Vec::new());
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("fileDialog module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "focus" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("focus.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "next" | "previous" => {
                if args.len() > 1 {
                    return Err(diag(
                        span,
                        &format!("focus.{name} expects 0 or 1 arguments, got {}", args.len()),
                    ));
                }
                if let Some(wrap) = args.first() {
                    let actual = type_of_expr(wrap, env, signatures)?;
                    require_type(
                        wrap.span,
                        &Type::Bool,
                        &actual,
                        &format!("focus.{name} wrap"),
                    )?;
                }
                return Ok(Vec::new());
            }
            "nextIn" | "previousIn" => {
                if !(1..=2).contains(&args.len()) {
                    return Err(diag(
                        span,
                        &format!("focus.{name} expects 1 or 2 arguments, got {}", args.len()),
                    ));
                }
                let scope_type = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &scope_type,
                    &format!("focus.{name} scope"),
                )?;
                if let Some(wrap) = args.get(1) {
                    let actual = type_of_expr(wrap, env, signatures)?;
                    require_type(
                        wrap.span,
                        &Type::Bool,
                        &actual,
                        &format!("focus.{name} wrap"),
                    )?;
                }
                return Ok(Vec::new());
            }
            "firstIn" | "lastIn" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("focus.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let scope_type = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &scope_type,
                    &format!("focus.{name} scope"),
                )?;
                return Ok(Vec::new());
            }
            "first" | "last" | "clear" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("focus.{name} expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(Vec::new());
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("focus module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "textInput" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("textInput.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "selectionStart" | "selectionEnd" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("textInput.{name} expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(vec![Type::I64]);
            }
            "setCaret" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("textInput.setCaret expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &actual,
                    "textInput.setCaret position",
                )?;
                return Ok(vec![Type::Bool]);
            }
            "setSelection" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!(
                            "textInput.setSelection expects 2 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                for (index, label) in [(0, "start"), (1, "end")] {
                    let actual = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::I64,
                        &actual,
                        &format!("textInput.setSelection {label}"),
                    )?;
                }
                return Ok(vec![Type::Bool]);
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("textInput module has no function '{name}'"),
                ));
            }
        }
    }
    if namespace == "android" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("android.{name} accepts positional arguments only"),
            ));
        }
        match name.as_str() {
            "sdkInt" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("android.sdkInt expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(vec![Type::I64]);
            }
            "hasSystemFeature" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.hasSystemFeature expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &actual,
                    "android.hasSystemFeature feature",
                )?;
                return Ok(vec![Type::Bool]);
            }
            "vibrate" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("android.vibrate expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &actual,
                    "android.vibrate durationMs",
                )?;
                return Ok(Vec::new());
            }
            "keepScreenOn" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.keepScreenOn expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Bool,
                    &actual,
                    "android.keepScreenOn enabled",
                )?;
                return Ok(Vec::new());
            }
            "finishActivity" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!(
                            "android.finishActivity expects 0 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                return Ok(Vec::new());
            }
            "scheduleBackgroundJob" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.scheduleBackgroundJob expects 2 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                for (index, label) in [(0, "jobId"), (1, "delayMs")] {
                    let actual = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::I64,
                        &actual,
                        &format!("android.scheduleBackgroundJob {label}"),
                    )?;
                }
                return Ok(vec![Type::Bool]);
            }
            "cancelBackgroundJob" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.cancelBackgroundJob expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &actual,
                    "android.cancelBackgroundJob jobId",
                )?;
                return Ok(Vec::new());
            }
            "openUrl" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("android.openUrl expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(args[0].span, &Type::Str, &actual, "android.openUrl url")?;
                return Ok(Vec::new());
            }
            "share" | "setClipboardText" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("android.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &actual,
                    &format!("android.{name} text"),
                )?;
                return Ok(Vec::new());
            }
            "focusNext" | "focusPrevious" => {
                if args.len() > 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.{name} expects 0 or 1 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                if let Some(wrap) = args.first() {
                    let actual = type_of_expr(wrap, env, signatures)?;
                    require_type(
                        wrap.span,
                        &Type::Bool,
                        &actual,
                        &format!("android.{name} wrap"),
                    )?;
                }
                return Ok(Vec::new());
            }
            "showKeyboard"
            | "hideKeyboard"
            | "openAppSettings"
            | "openNotificationSettings"
            | "focusFirst"
            | "focusLast"
            | "clearFocus" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("android.{name} expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(Vec::new());
            }
            "selectionStart" | "selectionEnd" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!("android.{name} expects 0 arguments, got {}", args.len()),
                    ));
                }
                return Ok(vec![Type::I64]);
            }
            "setCaret" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("android.setCaret expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &actual,
                    "android.setCaret position",
                )?;
                return Ok(vec![Type::Bool]);
            }
            "setSelection" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.setSelection expects 2 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                for (index, label) in [(0, "start"), (1, "end")] {
                    let actual = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::I64,
                        &actual,
                        &format!("android.setSelection {label}"),
                    )?;
                }
                return Ok(vec![Type::Bool]);
            }
            "setImeAction" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.setImeAction expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &actual,
                    "android.setImeAction action",
                )?;
                return Ok(vec![Type::Bool]);
            }
            "pickFile" | "pickMedia" | "pickDirectory" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!("android.{name} expects 1 argument, got {}", args.len()),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                let expected = Type::Function {
                    params: vec![Type::Str],
                    returns: Vec::new(),
                };
                require_type(
                    args[0].span,
                    &expected,
                    &actual,
                    &format!("android.{name} callback"),
                )?;
                return Ok(Vec::new());
            }
            "createNotificationChannel" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.createNotificationChannel expects 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                for (index, label) in [(0, "id"), (1, "name"), (2, "description")] {
                    let actual = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::Str,
                        &actual,
                        &format!("android.createNotificationChannel {label}"),
                    )?;
                }
                return Ok(Vec::new());
            }
            "permissionGranted" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.permissionGranted expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &actual,
                    "android.permissionGranted permission",
                )?;
                return Ok(vec![Type::Bool]);
            }
            "requestPermission" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.requestPermission expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let actual = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &actual,
                    "android.requestPermission permission",
                )?;
                return Ok(Vec::new());
            }
            "notificationPermissionGranted" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!(
                            "android.notificationPermissionGranted expects 0 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                return Ok(vec![Type::Bool]);
            }
            "requestNotificationPermission" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        &format!(
                            "android.requestNotificationPermission expects 0 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                return Ok(Vec::new());
            }
            "notify" => {
                if args.len() != 4 {
                    return Err(diag(
                        span,
                        &format!("android.notify expects 4 arguments, got {}", args.len()),
                    ));
                }
                let channel = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &channel,
                    "android.notify channelId",
                )?;
                let notification_id = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &notification_id,
                    "android.notify notificationId",
                )?;
                let title = type_of_expr(&args[2], env, signatures)?;
                require_type(args[2].span, &Type::Str, &title, "android.notify title")?;
                let body = type_of_expr(&args[3], env, signatures)?;
                require_type(args[3].span, &Type::Str, &body, "android.notify body")?;
                return Ok(Vec::new());
            }
            "notifyUrlAction" => {
                if args.len() != 6 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.notifyUrlAction expects 6 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let channel = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::Str,
                    &channel,
                    "android.notifyUrlAction channelId",
                )?;
                let notification_id = type_of_expr(&args[1], env, signatures)?;
                require_type(
                    args[1].span,
                    &Type::I64,
                    &notification_id,
                    "android.notifyUrlAction notificationId",
                )?;
                for (index, label) in [(2, "title"), (3, "body"), (4, "actionLabel"), (5, "url")] {
                    let actual = type_of_expr(&args[index], env, signatures)?;
                    require_type(
                        args[index].span,
                        &Type::Str,
                        &actual,
                        &format!("android.notifyUrlAction {label}"),
                    )?;
                }
                return Ok(Vec::new());
            }
            "cancelNotification" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        &format!(
                            "android.cancelNotification expects 1 argument, got {}",
                            args.len()
                        ),
                    ));
                }
                let notification_id = type_of_expr(&args[0], env, signatures)?;
                require_type(
                    args[0].span,
                    &Type::I64,
                    &notification_id,
                    "android.cancelNotification notificationId",
                )?;
                return Ok(Vec::new());
            }
            _ => {
                return Err(diag(
                    *name_span,
                    &format!("android module has no function '{name}'"),
                ));
            }
        }
    }
    if let Some(definition) = signatures.enum_type(namespace) {
        require_visible_declaration(
            *namespace_span,
            definition.span,
            definition.public,
            "enum",
            namespace,
            signatures,
        )?;
        if !named_args.is_empty() {
            return Err(diag(
                span,
                &format!("enum variant '{namespace}.{name}' does not accept named payloads"),
            ));
        }
        let Some(variant_definition) = definition.variant(name) else {
            return Err(diag(
                *name_span,
                &format!("enum '{namespace}' has no variant '{name}'"),
            )
            .with_label(definition.span, format!("'{namespace}' is declared here")));
        };
        if args.len() != variant_definition.payloads.len() {
            return Err(diag(
                span,
                &format!(
                    "variant '{namespace}.{name}' expects {} payload value{}, got {}",
                    variant_definition.payloads.len(),
                    if variant_definition.payloads.len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    args.len()
                ),
            )
            .with_label(
                variant_definition.span,
                format!("'{name}' is declared here"),
            ));
        }
        for (index, (arg, expected)) in args.iter().zip(&variant_definition.payloads).enumerate() {
            let actual = type_of_expr(arg, env, signatures)?;
            require_type(
                arg.span,
                expected,
                &actual,
                &format!("payload {} of '{}.{name}'", index + 1, namespace),
            )?;
        }
        return Ok(vec![Type::Named(namespace.to_string())]);
    }

    let Some(interface) = signatures.interface(namespace) else {
        return Err(diag(
            *namespace_span,
            &format!("unknown enum, interface, or platform namespace '{namespace}'"),
        ));
    };
    require_visible_declaration(
        *namespace_span,
        interface.span,
        interface.public,
        "interface",
        namespace,
        signatures,
    )?;
    let Some(member) = interface.functions.get(name) else {
        return Err(diag(
            *name_span,
            &format!("interface '{namespace}' has no capability '{name}'"),
        )
        .with_label(interface.span, format!("'{namespace}' is declared here")));
    };
    let Some(receiver) = args.first() else {
        return Err(diag(
            span,
            &format!(
                "static interface call '{namespace}.{name}' requires a concrete receiver as its first argument"
            ),
        ));
    };
    let receiver_ty = signatures.canonical_type(&type_of_expr(receiver, env, signatures)?);
    let Type::Named(target_name) = &receiver_ty else {
        return Err(diag(
            receiver.span,
            &format!(
                "static interface call '{namespace}.{name}' requires a concrete struct or enum receiver"
            ),
        ));
    };
    if target_name == namespace && signatures.interface(target_name).is_some() {
        return check_declared_call(
            span,
            &format!("{namespace}.{name}"),
            member,
            &args[1..],
            named_args,
            env,
            signatures,
        );
    }
    let Some(implementation) = signatures.implementation(namespace, target_name) else {
        return Err(diag(
            receiver.span,
            &format!("type '{target_name}' does not implement interface '{namespace}'"),
        ));
    };
    if !implementation.functions.contains_key(name) {
        return Err(diag(
            *name_span,
            &format!(
                "implementation of '{namespace}' for '{target_name}' does not map capability '{name}'"
            ),
        ));
    }
    check_declared_call(
        span,
        &format!("{namespace}.{name}"),
        member,
        &args[1..],
        named_args,
        env,
        signatures,
    )
}

fn check_call(
    span: SourceSpan,
    name: &str,
    args: &[Expr],
    named_args: &[NamedArg],
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<Type>, Diagnostic> {
    let Some(signature) = signatures.get(name) else {
        let Some(Type::Function { params, returns }) = env.get(name) else {
            return Err(diag(
                span,
                &format!("unknown function or callable '{name}'"),
            ));
        };
        if !named_args.is_empty() {
            return Err(diag(
                span,
                "first-class function values accept positional arguments only",
            ));
        }
        if args.len() != params.len() {
            return Err(diag(
                span,
                &format!(
                    "function value '{name}' expects {} arguments, got {}",
                    params.len(),
                    args.len()
                ),
            ));
        }
        for (index, (arg, expected)) in args.iter().zip(params).enumerate() {
            let actual = type_of_expr(arg, env, signatures)?;
            require_type(
                arg.span,
                expected,
                &actual,
                &format!("argument {} to function value '{name}'", index + 1),
            )?;
        }
        return Ok(returns.clone());
    };
    require_visible_declaration(
        span,
        signature.span,
        signature.public,
        "function",
        name,
        signatures,
    )?;
    check_declared_call(span, name, signature, args, named_args, env, signatures)
}

fn check_declared_call(
    span: SourceSpan,
    name: &str,
    signature: &Signature,
    args: &[Expr],
    named_args: &[NamedArg],
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<Type>, Diagnostic> {
    let positional = signature
        .param_details
        .iter()
        .filter(|param| !param.named_only)
        .collect::<Vec<_>>();
    if args.len() > positional.len() {
        return Err(diag(
            span,
            &format!(
                "function '{name}' accepts at most {} positional argument{}, got {}",
                positional.len(),
                if positional.len() == 1 { "" } else { "s" },
                args.len()
            ),
        )
        .with_label(signature.span, format!("'{name}' is declared here")));
    }

    let mut supplied = HashSet::new();
    for (index, (arg, expected)) in args.iter().zip(&positional).enumerate() {
        let actual = type_of_expr(arg, env, signatures)?;
        require_type(
            arg.span,
            &expected.ty,
            &actual,
            &format!("argument {} ('{}') to '{name}'", index + 1, expected.name),
        )
        .map_err(|diagnostic| {
            diagnostic.with_label(signature.span, format!("'{name}' is declared here"))
        })?;
        supplied.insert(expected.name.as_str());
    }

    for arg in named_args {
        let Some(expected) = signature
            .param_details
            .iter()
            .find(|param| param.name == arg.name)
        else {
            return Err(diag(
                arg.name_span,
                &format!("function '{name}' has no named parameter '{}'", arg.name),
            )
            .with_label(signature.span, format!("'{name}' is declared here")));
        };
        if !expected.named_only {
            return Err(diag(
                arg.name_span,
                &format!(
                    "parameter '{}' of '{name}' is positional; it cannot be passed by name",
                    arg.name
                ),
            )
            .with_label(expected.span, format!("'{}' is declared here", arg.name)));
        }
        let actual = type_of_expr(&arg.value, env, signatures)?;
        require_type(
            arg.value.span,
            &expected.ty,
            &actual,
            &format!("named argument '{}' to '{name}'", arg.name),
        )
        .map_err(|diagnostic| {
            diagnostic.with_label(expected.span, format!("'{}' is declared here", arg.name))
        })?;
        supplied.insert(expected.name.as_str());
    }

    let missing = signature
        .param_details
        .iter()
        .filter(|param| !supplied.contains(param.name.as_str()) && param.default.is_none())
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(diag(
            span,
            &format!(
                "function '{name}' is missing required argument{} {}",
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", ")
            ),
        )
        .with_label(signature.span, format!("'{name}' is declared here")));
    }

    Ok(signature.returns.clone())
}

fn block_guarantees_return(body: &[Stmt]) -> bool {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Return(_) => return true,
            StmtKind::If {
                body, else_body, ..
            } if !else_body.is_empty()
                && block_guarantees_return(body)
                && block_guarantees_return(else_body) =>
            {
                return true;
            }
            StmtKind::Match { arms, .. }
                if !arms.is_empty()
                    && arms.iter().all(|arm| block_guarantees_return(&arm.body)) =>
            {
                return true;
            }
            StmtKind::ListMatch { arms, .. }
                if !arms.is_empty()
                    && arms.iter().all(|arm| block_guarantees_return(&arm.body)) =>
            {
                return true;
            }
            StmtKind::If { .. }
            | StmtKind::ForRange { .. }
            | StmtKind::ForEach { .. }
            | StmtKind::While { .. }
            | StmtKind::Match { .. }
            | StmtKind::ListMatch { .. }
            | StmtKind::Let { .. }
            | StmtKind::Var { .. }
            | StmtKind::Assign { .. }
            | StmtKind::AssignMultiDestructure { .. }
            | StmtKind::AssignListDestructure { .. }
            | StmtKind::AssignStructDestructure { .. }
            | StmtKind::LetDestructure { .. }
            | StmtKind::LetMultiDestructure { .. }
            | StmtKind::LetListDestructure { .. }
            | StmtKind::LetStructDestructure { .. }
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Expr(_)
            | StmtKind::Shell { .. } => {}
        }
    }
    false
}

fn return_types_name(types: &[Type]) -> String {
    match types {
        [] => "void".to_string(),
        [ty] => ty.name().to_string(),
        _ => format!(
            "({})",
            types.iter().map(Type::name).collect::<Vec<_>>().join(", ")
        ),
    }
}

pub(crate) fn constant_primitive_value(
    expr: &Expr,
    signatures: &Signatures,
) -> Option<ConstantValue> {
    evaluate_default_expr(expr, signatures).ok()
}

fn evaluate_default_expr(
    expr: &Expr,
    signatures: &Signatures,
) -> Result<ConstantValue, Diagnostic> {
    match &expr.kind {
        ExprKind::Int(value) => Ok(ConstantValue::I64(*value)),
        ExprKind::Bool(value) => Ok(ConstantValue::Bool(*value)),
        ExprKind::Str(value) => Ok(ConstantValue::Str(value.clone())),
        ExprKind::Var(name) => {
            let Some(constant) = signatures.constant(name) else {
                return Err(diag(
                    expr.span,
                    &format!(
                        "parameter defaults may reference only compile-time constants; '{name}' is not one"
                    ),
                ));
            };
            require_visible_declaration(
                expr.span,
                constant.span,
                constant.public,
                "constant",
                name,
                signatures,
            )?;
            Ok(constant.value.clone())
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            let condition = evaluate_default_expr(cond, signatures)?;
            match condition {
                ConstantValue::Bool(true) => evaluate_default_expr(then_expr, signatures),
                ConstantValue::Bool(false) => evaluate_default_expr(else_expr, signatures),
                actual => Err(constant_type_error(
                    cond.span,
                    "conditional expression condition",
                    &Type::Bool,
                    &actual.ty(),
                )),
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let value = evaluate_default_expr(inner, signatures)?;
            match (op, value) {
                (UnaryOp::Neg, ConstantValue::I64(value)) => value
                    .checked_neg()
                    .map(ConstantValue::I64)
                    .ok_or_else(|| diag(expr.span, "constant integer negation overflows i64")),
                (UnaryOp::Not, ConstantValue::Bool(value)) => Ok(ConstantValue::Bool(!value)),
                (UnaryOp::Neg, actual) => Err(constant_type_error(
                    expr.span,
                    "unary '-'",
                    &Type::I64,
                    &actual.ty(),
                )),
                (UnaryOp::Not, actual) => Err(constant_type_error(
                    expr.span,
                    "unary '!'",
                    &Type::Bool,
                    &actual.ty(),
                )),
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left = evaluate_default_expr(left, signatures)?;
            if matches!(op, BinOp::And) && left == ConstantValue::Bool(false) {
                return Ok(ConstantValue::Bool(false));
            }
            if matches!(op, BinOp::Or) && left == ConstantValue::Bool(true) {
                return Ok(ConstantValue::Bool(true));
            }
            let right = evaluate_default_expr(right, signatures)?;
            evaluate_constant_binary(expr.span, *op, left, right)
        }
        ExprKind::Nil
        | ExprKind::None
        | ExprKind::AnonymousFunction { .. }
        | ExprKind::Call { .. }
        | ExprKind::ShellCall { .. }
        | ExprKind::Pipe { .. }
        | ExprKind::List(_)
        | ExprKind::ListSpread { .. }
        | ExprKind::ListOptional { .. }
        | ExprKind::ListIf { .. }
        | ExprKind::Index { .. }
        | ExprKind::Slice { .. }
        | ExprKind::ListComprehension { .. }
        | ExprKind::QualifiedCall { .. }
        | ExprKind::StructLiteral { .. }
        | ExprKind::Field { .. }
        | ExprKind::Match { .. }
        | ExprKind::ListMatch { .. } => Err(diag(
            expr.span,
            "parameter defaults must be compile-time primitive expressions",
        )),
    }
}

fn evaluate_constant(
    name: &str,
    definitions: &HashMap<&str, &ConstantDef>,
    signatures: &Signatures,
    cache: &mut HashMap<String, ConstantSignature>,
    stack: &mut Vec<String>,
) -> Result<ConstantSignature, Diagnostic> {
    if let Some(value) = cache.get(name) {
        return Ok(value.clone());
    }
    let definition = definitions
        .get(name)
        .expect("constant evaluation starts from known definitions");
    if let Some(index) = stack.iter().position(|entry| entry == name) {
        let mut cycle = stack[index..].to_vec();
        cycle.push(name.to_string());
        return Err(diag(
            definition.name_span,
            &format!("constant '{name}' is recursive"),
        )
        .with_note(format!("constant cycle: {}", cycle.join(" -> "))));
    }
    stack.push(name.to_string());
    let value = evaluate_constant_expr(&definition.value, definitions, signatures, cache, stack)?;
    stack.pop();

    let declared = signatures.canonical_type(&definition.ty);
    require_type(
        definition.value.span,
        &declared,
        &value.ty(),
        &format!("constant '{name}'"),
    )?;
    let signature = ConstantSignature {
        public: definition.public,
        ty: declared,
        value,
        span: definition.name_span,
    };
    cache.insert(name.to_string(), signature.clone());
    Ok(signature)
}

fn evaluate_constant_expr(
    expr: &Expr,
    definitions: &HashMap<&str, &ConstantDef>,
    signatures: &Signatures,
    cache: &mut HashMap<String, ConstantSignature>,
    stack: &mut Vec<String>,
) -> Result<ConstantValue, Diagnostic> {
    match &expr.kind {
        ExprKind::Int(value) => Ok(ConstantValue::I64(*value)),
        ExprKind::Bool(value) => Ok(ConstantValue::Bool(*value)),
        ExprKind::Str(value) => Ok(ConstantValue::Str(value.clone())),
        ExprKind::Var(name) => {
            let Some(definition) = definitions.get(name.as_str()) else {
                return Err(diag(
                    expr.span,
                    &format!("unknown constant '{name}' in constant expression"),
                ));
            };
            require_visible_declaration(
                expr.span,
                definition.name_span,
                definition.public,
                "constant",
                name,
                signatures,
            )?;
            evaluate_constant(name, definitions, signatures, cache, stack)
                .map(|constant| constant.value)
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            let condition = evaluate_constant_expr(cond, definitions, signatures, cache, stack)?;
            match condition {
                ConstantValue::Bool(true) => {
                    evaluate_constant_expr(then_expr, definitions, signatures, cache, stack)
                }
                ConstantValue::Bool(false) => {
                    evaluate_constant_expr(else_expr, definitions, signatures, cache, stack)
                }
                actual => Err(constant_type_error(
                    cond.span,
                    "conditional expression condition",
                    &Type::Bool,
                    &actual.ty(),
                )),
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let value = evaluate_constant_expr(inner, definitions, signatures, cache, stack)?;
            match (op, value) {
                (UnaryOp::Neg, ConstantValue::I64(value)) => value
                    .checked_neg()
                    .map(ConstantValue::I64)
                    .ok_or_else(|| diag(expr.span, "constant integer negation overflows i64")),
                (UnaryOp::Not, ConstantValue::Bool(value)) => Ok(ConstantValue::Bool(!value)),
                (UnaryOp::Neg, actual) => Err(constant_type_error(
                    expr.span,
                    "unary '-'",
                    &Type::I64,
                    &actual.ty(),
                )),
                (UnaryOp::Not, actual) => Err(constant_type_error(
                    expr.span,
                    "unary '!'",
                    &Type::Bool,
                    &actual.ty(),
                )),
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left = evaluate_constant_expr(left, definitions, signatures, cache, stack)?;
            if matches!(op, BinOp::And) && left == ConstantValue::Bool(false) {
                return Ok(ConstantValue::Bool(false));
            }
            if matches!(op, BinOp::Or) && left == ConstantValue::Bool(true) {
                return Ok(ConstantValue::Bool(true));
            }
            let right = evaluate_constant_expr(right, definitions, signatures, cache, stack)?;
            evaluate_constant_binary(expr.span, *op, left, right)
        }
        ExprKind::Nil
        | ExprKind::None
        | ExprKind::AnonymousFunction { .. }
        | ExprKind::Call { .. }
        | ExprKind::ShellCall { .. }
        | ExprKind::Pipe { .. }
        | ExprKind::List(_)
        | ExprKind::ListSpread { .. }
        | ExprKind::ListOptional { .. }
        | ExprKind::ListIf { .. }
        | ExprKind::Index { .. }
        | ExprKind::Slice { .. }
        | ExprKind::ListComprehension { .. }
        | ExprKind::QualifiedCall { .. }
        | ExprKind::StructLiteral { .. }
        | ExprKind::Field { .. }
        | ExprKind::Match { .. }
        | ExprKind::ListMatch { .. } => Err(diag(
            expr.span,
            "constant expressions currently support primitive literals, constant references, and primitive operators",
        )),
    }
}

pub(crate) fn evaluate_constant_binary(
    span: SourceSpan,
    op: BinOp,
    left: ConstantValue,
    right: ConstantValue,
) -> Result<ConstantValue, Diagnostic> {
    match (op, left, right) {
        (BinOp::Add, ConstantValue::I64(left), ConstantValue::I64(right)) => left
            .checked_add(right)
            .map(ConstantValue::I64)
            .ok_or_else(|| diag(span, "constant integer addition overflows i64")),
        (BinOp::Sub, ConstantValue::I64(left), ConstantValue::I64(right)) => left
            .checked_sub(right)
            .map(ConstantValue::I64)
            .ok_or_else(|| diag(span, "constant integer subtraction overflows i64")),
        (BinOp::Mul, ConstantValue::I64(left), ConstantValue::I64(right)) => left
            .checked_mul(right)
            .map(ConstantValue::I64)
            .ok_or_else(|| diag(span, "constant integer multiplication overflows i64")),
        (BinOp::Div, ConstantValue::I64(_), ConstantValue::I64(0)) => {
            Err(diag(span, "constant integer division by zero is invalid"))
        }
        (BinOp::Div, ConstantValue::I64(i64::MIN), ConstantValue::I64(-1)) => {
            Err(diag(span, "constant integer division overflows i64"))
        }
        (BinOp::Div, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::I64(left / right))
        }
        (BinOp::Lt, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left < right))
        }
        (BinOp::Le, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left <= right))
        }
        (BinOp::Gt, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left > right))
        }
        (BinOp::Ge, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left >= right))
        }
        (BinOp::Eq, left, right) if left.ty() == right.ty() => {
            Ok(ConstantValue::Bool(left == right))
        }
        (BinOp::Ne, left, right) if left.ty() == right.ty() => {
            Ok(ConstantValue::Bool(left != right))
        }
        (BinOp::And, ConstantValue::Bool(left), ConstantValue::Bool(right)) => {
            Ok(ConstantValue::Bool(left && right))
        }
        (BinOp::Or, ConstantValue::Bool(left), ConstantValue::Bool(right)) => {
            Ok(ConstantValue::Bool(left || right))
        }
        (op, left, right) => Err(diag(
            span,
            &format!(
                "constant operator '{}' does not support {} and {}",
                constant_operator_name(op),
                left.ty().name(),
                right.ty().name()
            ),
        )),
    }
}

fn constant_operator_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::Coalesce => "??",
    }
}

fn constant_type_error(
    span: SourceSpan,
    context: &str,
    expected: &Type,
    actual: &Type,
) -> Diagnostic {
    diag(
        span,
        &format!(
            "{context}: expected {}, got {}",
            expected.name(),
            actual.name()
        ),
    )
}

fn resolve_alias_target(
    ty: &Type,
    aliases: &HashMap<String, Type>,
    chain: &mut Vec<String>,
) -> Result<Type, Vec<String>> {
    let Type::Named(name) = ty else {
        return Ok(ty.clone());
    };
    let Some(target) = aliases.get(name) else {
        return Ok(ty.clone());
    };
    if let Some(index) = chain.iter().position(|entry| entry == name) {
        let mut cycle = chain[index..].to_vec();
        cycle.push(name.clone());
        return Err(cycle);
    }
    chain.push(name.clone());
    let resolved = resolve_alias_target(target, aliases, chain);
    chain.pop();
    resolved
}

fn validate_assignment_target(
    name: &str,
    span: SourceSpan,
    actual: &Type,
    env: &HashMap<String, Type>,
    mutable: &HashSet<String>,
    _signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(expected) = env.get(name) else {
        diagnostics.push(diag(span, &format!("unknown binding '{name}'")));
        return;
    };
    if !mutable.contains(name) {
        diagnostics.push(diag(
            span,
            &format!("cannot assign to immutable binding '{name}'; declare it with 'var'"),
        ));
        return;
    }
    if let Err(diagnostic) = require_type(span, expected, actual, "pattern assignment") {
        diagnostics.push(diagnostic);
    }
}

fn validate_struct_assignment_fields(
    fields: &[StructPatternField],
    concrete_name: &str,
    env: &HashMap<String, Type>,
    mutable: &HashSet<String>,
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(definition) = signatures.struct_type(concrete_name) else {
        return;
    };
    for field in fields {
        let Some(field_signature) = definition.field(&field.field) else {
            diagnostics.push(
                diag(
                    field.field_span,
                    &format!("struct '{concrete_name}' has no field '{}'", field.field),
                )
                .with_label(
                    definition.span,
                    format!("'{concrete_name}' is declared here"),
                ),
            );
            continue;
        };
        if let Some(nested) = &field.nested {
            let expected = signatures.canonical_type(&Type::Named(nested.struct_name.clone()));
            let actual = signatures.canonical_type(&field_signature.ty);
            if let Err(diagnostic) = require_type(
                nested.struct_span,
                &expected,
                &actual,
                "nested struct assignment pattern",
            ) {
                diagnostics.push(diagnostic);
                continue;
            }
            let Type::Named(nested_name) = expected else {
                continue;
            };
            validate_struct_assignment_fields(
                &nested.fields,
                &nested_name,
                env,
                mutable,
                signatures,
                diagnostics,
            );
            continue;
        }
        if field.binding.name != "_" {
            validate_assignment_target(
                &field.binding.name,
                field.binding.span,
                &field_signature.ty,
                env,
                mutable,
                signatures,
                diagnostics,
            );
        }
    }
}

fn bind_struct_pattern_fields(
    fields: &[StructPatternField],
    concrete_name: &str,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(definition) = signatures.struct_type(concrete_name) else {
        return;
    };
    for field in fields {
        let Some(field_signature) = definition.field(&field.field) else {
            diagnostics.push(
                diag(
                    field.field_span,
                    &format!("struct '{concrete_name}' has no field '{}'", field.field),
                )
                .with_label(
                    definition.span,
                    format!("'{concrete_name}' is declared here"),
                ),
            );
            continue;
        };
        if let Some(nested) = &field.nested {
            let expected = signatures.canonical_type(&Type::Named(nested.struct_name.clone()));
            let actual = signatures.canonical_type(&field_signature.ty);
            if let Err(diagnostic) = require_type(
                nested.struct_span,
                &expected,
                &actual,
                "nested struct pattern",
            ) {
                diagnostics.push(diagnostic);
                continue;
            }
            let Type::Named(nested_name) = expected else {
                diagnostics.push(diag(
                    nested.struct_span,
                    &format!(
                        "struct pattern '{}' does not name a struct type",
                        nested.struct_name
                    ),
                ));
                continue;
            };
            if signatures.struct_type(&nested_name).is_none() {
                diagnostics.push(diag(
                    nested.struct_span,
                    &format!(
                        "struct pattern '{}' does not name a struct type",
                        nested.struct_name
                    ),
                ));
                continue;
            }
            bind_struct_pattern_fields(&nested.fields, &nested_name, env, signatures, diagnostics);
            continue;
        }
        if field.binding.name == "_" {
            continue;
        }
        if env.contains_key(&field.binding.name) {
            diagnostics.push(diag(
                field.binding.span,
                &format!("'{}' is already defined in this scope", field.binding.name),
            ));
        } else {
            env.insert(field.binding.name.clone(), field_signature.ty.clone());
        }
    }
}

fn require_storable_value_type(
    span: SourceSpan,
    ty: &Type,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    require_known_type(span, ty, signatures)?;
    if !signatures.is_copy_type(ty) {
        let message = if matches!(signatures.canonical_type(ty), Type::List(_)) {
            "list values are currently local-only and cannot be stored inside structs or enums"
                .to_string()
        } else {
            format!(
                "non-copy value type '{}' cannot be stored inside structs or enums until ownership is implemented",
                signatures.canonical_type(ty).name()
            )
        };
        return Err(diag(span, &message));
    }
    if let Type::Named(name) = signatures.canonical_type(ty)
        && signatures.interface(&name).is_some()
    {
        return Err(diag(
            span,
            "interface values are currently supported in bindings and function boundaries, not inside struct/enum by-value layouts",
        ));
    }
    Ok(())
}

fn resolve_interface_composition(
    name: &str,
    definitions: &HashMap<&str, &crate::ast::InterfaceDef>,
    signatures: &Signatures,
    cache: &mut HashMap<String, HashMap<String, Signature>>,
    visiting: &mut Vec<String>,
) -> Result<HashMap<String, Signature>, Diagnostic> {
    if let Some(cached) = cache.get(name) {
        return Ok(cached.clone());
    }
    let definition = definitions.get(name).copied().ok_or_else(|| {
        Diagnostic::global(DiagnosticStage::Type, format!("unknown interface '{name}'"))
    })?;
    if let Some(index) = visiting.iter().position(|entry| entry == name) {
        let mut cycle = visiting[index..].to_vec();
        cycle.push(name.to_string());
        return Err(diag(
            definition.name_span,
            &format!("interface '{}' has a composition cycle", definition.name),
        )
        .with_note(format!("interface cycle: {}", cycle.join(" -> "))));
    }
    visiting.push(name.to_string());
    let mut functions = signatures
        .interface(name)
        .map(|interface| interface.functions.clone())
        .unwrap_or_default();
    for parent in &definition.parents {
        if !definitions.contains_key(parent.name.as_str()) {
            visiting.pop();
            return Err(diag(
                parent.span,
                &format!("unknown composed interface '{}'", parent.name),
            ));
        }
        let parent_signature = signatures
            .interface(&parent.name)
            .expect("known composed interface has a signature");
        if definition.public && !parent_signature.public {
            visiting.pop();
            return Err(diag(
                parent.span,
                &format!(
                    "public interface '{}' cannot compose private interface '{}'",
                    definition.name, parent.name
                ),
            )
            .with_label(
                parent_signature.span,
                format!("'{}' is declared private here", parent.name),
            ));
        }
        require_visible_declaration(
            parent.span,
            parent_signature.span,
            parent_signature.public,
            "interface",
            &parent.name,
            signatures,
        )?;
        let inherited =
            resolve_interface_composition(&parent.name, definitions, signatures, cache, visiting)?;
        for (member_name, inherited_signature) in inherited {
            if let Some(existing) = functions.get(&member_name) {
                if !same_interface_contract(existing, &inherited_signature) {
                    visiting.pop();
                    return Err(diag(
                        parent.span,
                        &format!(
                            "interface '{}' composes conflicting capability '{}'",
                            definition.name, member_name
                        ),
                    )
                    .with_label(existing.span, "conflicting capability is declared here"));
                }
            } else {
                functions.insert(member_name, inherited_signature);
            }
        }
    }
    visiting.pop();
    cache.insert(name.to_string(), functions.clone());
    Ok(functions)
}

fn same_interface_contract(left: &Signature, right: &Signature) -> bool {
    left.returns == right.returns
        && left.param_details.len() == right.param_details.len()
        && left
            .param_details
            .iter()
            .zip(&right.param_details)
            .all(|(left, right)| {
                left.name == right.name
                    && left.ty == right.ty
                    && left.named_only == right.named_only
            })
}

fn module_import_closure(program: &Program) -> HashMap<SourceId, HashSet<SourceId>> {
    let mut imports = HashMap::<SourceId, HashSet<SourceId>>::new();
    for import in &program.imports {
        let importer = import.span.source_id;
        let Some(target) = import.resolved_source_id else {
            continue;
        };
        if importer == SourceId::UNKNOWN || target == SourceId::UNKNOWN {
            continue;
        }
        imports.entry(importer).or_default().insert(target);
    }
    loop {
        let snapshot = imports.clone();
        let mut changed = false;
        for targets in imports.values_mut() {
            let inherited = targets
                .iter()
                .flat_map(|target| snapshot.get(target).into_iter().flatten().copied())
                .collect::<Vec<_>>();
            for target in inherited {
                changed |= targets.insert(target);
            }
        }
        if !changed {
            return imports;
        }
    }
}

fn same_module(declaration: SourceSpan, usage: SourceSpan) -> bool {
    declaration.source_id == SourceId::UNKNOWN
        || usage.source_id == SourceId::UNKNOWN
        || declaration.source_id == usage.source_id
}

fn require_visible_declaration(
    usage: SourceSpan,
    declaration: SourceSpan,
    public: bool,
    kind: &str,
    name: &str,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    if same_module(declaration, usage) {
        return Ok(());
    }
    if !public {
        return Err(diag(
            usage,
            &format!("private {kind} '{name}' is not accessible from this module"),
        )
        .with_label(declaration, format!("'{name}' is declared private here")));
    }
    if signatures
        .module_imports
        .get(&usage.source_id)
        .is_some_and(|imports| imports.contains(&declaration.source_id))
    {
        return Ok(());
    }
    Err(diag(
        usage,
        &format!("{kind} '{name}' is not imported into this module"),
    )
    .with_label(
        declaration,
        format!("'{name}' is declared in another module"),
    )
    .with_note("add an explicit import path that reaches the declaring module"))
}

fn require_visible_named_type(
    span: SourceSpan,
    name: &str,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    if let Some((declaration, public)) = signatures.alias_declaration(name) {
        return require_visible_declaration(
            span,
            declaration,
            public,
            "type alias",
            name,
            signatures,
        );
    }
    if let Some(definition) = signatures.interface(name) {
        return require_visible_declaration(
            span,
            definition.span,
            definition.public,
            "interface",
            name,
            signatures,
        );
    }
    if let Some(definition) = signatures.struct_type(name) {
        return require_visible_declaration(
            span,
            definition.span,
            definition.public,
            "struct",
            name,
            signatures,
        );
    }
    if let Some(definition) = signatures.enum_type(name) {
        return require_visible_declaration(
            span,
            definition.span,
            definition.public,
            "enum",
            name,
            signatures,
        );
    }
    Ok(())
}

fn require_publicly_nameable_type(
    span: SourceSpan,
    ty: &Type,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    match ty {
        Type::Named(name) => {
            if let Some((declaration, public)) = signatures.alias_declaration(name) {
                if !public {
                    return Err(diag(
                        span,
                        &format!("public API cannot expose private type alias '{name}'"),
                    )
                    .with_label(declaration, format!("'{name}' is declared private here")));
                }
                let canonical = signatures.canonical_type(ty);
                if canonical != *ty {
                    return require_publicly_nameable_type(span, &canonical, signatures);
                }
                return Ok(());
            }
            if let Some(definition) = signatures.interface(name) {
                if !definition.public {
                    return Err(diag(
                        span,
                        &format!("public API cannot expose private interface '{name}'"),
                    )
                    .with_label(
                        definition.span,
                        format!("'{name}' is declared private here"),
                    ));
                }
                return Ok(());
            }
            if let Some(definition) = signatures.struct_type(name) {
                if !definition.public {
                    return Err(diag(
                        span,
                        &format!("public API cannot expose private struct '{name}'"),
                    )
                    .with_label(
                        definition.span,
                        format!("'{name}' is declared private here"),
                    ));
                }
                return Ok(());
            }
            if let Some(definition) = signatures.enum_type(name)
                && !definition.public
            {
                return Err(diag(
                    span,
                    &format!("public API cannot expose private enum '{name}'"),
                )
                .with_label(
                    definition.span,
                    format!("'{name}' is declared private here"),
                ));
            }
            Ok(())
        }
        Type::List(element) => require_publicly_nameable_type(span, element, signatures),
        Type::Function { params, returns } => {
            for ty in params.iter().chain(returns) {
                require_publicly_nameable_type(span, ty, signatures)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn require_known_type(
    span: SourceSpan,
    ty: &Type,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    if let Type::Named(name) = ty {
        require_visible_named_type(span, name, signatures)?;
    }
    if let Type::List(element) | Type::Optional(element) = ty {
        require_known_type(span, element, signatures)?;
    }
    match signatures.canonical_type(ty) {
        Type::Named(name)
            if signatures.struct_type(&name).is_none()
                && signatures.enum_type(&name).is_none()
                && signatures.interface(&name).is_none() =>
        {
            Err(diag(span, &format!("unknown type '{name}'")))
        }
        Type::List(element) => require_known_type(span, &element, signatures),
        Type::Optional(inner) => {
            require_known_type(span, &inner, signatures)?;
            let actual = signatures.canonical_type(&inner);
            match &actual {
                Type::I64 | Type::Bool | Type::Str | Type::Error => Ok(()),
                Type::Named(name)
                    if (signatures.struct_type(name).is_some()
                        || signatures.enum_type(name).is_some())
                        && signatures.is_copy_type(&actual) =>
                {
                    Ok(())
                }
                _ => Err(diag(
                    span,
                    &format!(
                        "bootstrap optional values require a Copy scalar, struct, or enum; got {}?",
                        actual.name()
                    ),
                )),
            }
        }
        Type::Function { params, returns } => {
            if returns.len() > 1 {
                return Err(diag(
                    span,
                    "first-class function types currently support zero or one return value",
                ));
            }
            for ty in params.iter().chain(&returns) {
                require_known_type(span, ty, signatures)?;
                if matches!(signatures.canonical_type(ty), Type::Void) {
                    return Err(diag(span, "void cannot be a function value parameter type"));
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn require_type(
    span: SourceSpan,
    expected: &Type,
    actual: &Type,
    context: &str,
) -> Result<(), Diagnostic> {
    if expected == actual
        || matches!((expected, actual), (Type::Optional(_), Type::Optional(inner)) if **inner == Type::Void)
        || matches!((expected, actual), (Type::Optional(inner), actual) if inner.as_ref() == actual)
    {
        Ok(())
    } else {
        Err(diag(
            span,
            &format!(
                "{context}: expected {}, got {}",
                expected.name(),
                actual.name()
            ),
        ))
    }
}

fn diag(span: SourceSpan, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticStage::Type, span, message)
}

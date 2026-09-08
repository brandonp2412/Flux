use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, ConstantDef, Expr, ExprKind, Function, MatchPattern, NamedArg, Program, Stmt, StmtKind,
    StructPatternField, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceSpan};

#[derive(Debug, Clone)]
pub struct Signature {
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
    pub variants: Vec<EnumVariantSignature>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceSignature {
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
    constants: HashMap<String, ConstantSignature>,
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
            Type::Function { params, returns } => Type::Function {
                params: params.iter().map(|ty| self.canonical_type(ty)).collect(),
                returns: returns.iter().map(|ty| self.canonical_type(ty)).collect(),
            },
            _ => ty.clone(),
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
    let mut signatures = Signatures::default();
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
            }
        }
    }

    for alias in &program.aliases {
        if let Err(diagnostic) = require_known_type(alias.target_span, &alias.target, &signatures) {
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
            }
            members.insert(
                function.name.clone(),
                Signature {
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
        if function.name == "print" || function.name == "error" {
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
        }
        signatures.insert_function(
            function.name.clone(),
            Signature {
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

    match signatures.get("main") {
        None => diagnostics.push(
            Diagnostic::global(
                DiagnosticStage::Type,
                "program requires fn main() -> i64 { ... }",
            )
            .with_note("native executables enter Flux through a parameterless main returning i64"),
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

    for function in &program.functions {
        check_function_all(function, &signatures, &mut diagnostics);
    }

    if diagnostics.is_empty() {
        Ok(signatures)
    } else {
        Err(diagnostics)
    }
}

fn check_function_all(
    function: &Function,
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut env = HashMap::new();
    for param in &function.params {
        env.insert(param.name.clone(), signatures.canonical_type(&param.ty));
    }
    let return_types = function
        .returns
        .iter()
        .map(|ty| signatures.canonical_type(ty))
        .collect::<Vec<_>>();

    check_block_all(
        &function.body,
        &mut env,
        &return_types,
        signatures,
        diagnostics,
        0,
    );

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

fn check_block_all(
    body: &[Stmt],
    env: &mut HashMap<String, Type>,
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
            } => {
                if let Err(diagnostic) = require_known_type(*type_span, ty, signatures) {
                    diagnostics.push(diagnostic);
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
                }
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
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
                        env.insert(binding.name.clone(), signatures.canonical_type(&binding.ty));
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
            StmtKind::LetStructDestructure {
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
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
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
                let mut then_env = env.clone();
                check_block_all(
                    body,
                    &mut then_env,
                    return_types,
                    signatures,
                    diagnostics,
                    loop_depth,
                );
                let mut else_env = env.clone();
                check_block_all(
                    else_body,
                    &mut else_env,
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
                if !shadows {
                    nested.insert(name.clone(), Type::I64);
                }
                check_block_all(
                    body,
                    &mut nested,
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
                let mut seen = HashSet::new();
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
                    if !seen.insert(arm.variant.as_str()) {
                        diagnostics.push(diag(
                            arm.variant_span,
                            &format!("duplicate match arm for '{enum_name}.{}'", arm.variant),
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
                        }
                    }
                    check_block_all(
                        &arm.body,
                        &mut nested,
                        return_types,
                        signatures,
                        diagnostics,
                        loop_depth,
                    );
                }
                let missing = definition
                    .variants
                    .iter()
                    .filter(|variant| !seen.contains(variant.name.as_str()))
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
        }
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
        ExprKind::Var(name) => env
            .get(name)
            .cloned()
            .or_else(|| {
                signatures
                    .constant(name)
                    .map(|constant| constant.ty.clone())
            })
            .or_else(|| {
                signatures.get(name).map(|signature| Type::Function {
                    params: signature.params.clone(),
                    returns: signature.returns.clone(),
                })
            })
            .ok_or_else(|| {
                diag(
                    expr.span,
                    &format!("unknown binding, constant, or function '{name}'"),
                )
            }),
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
            if !named_args.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!("interface value conversion '{name}(...)' does not accept named arguments"),
                ));
            }
            if args.len() != 1 {
                return Err(diag(
                    expr.span,
                    &format!("interface value conversion '{name}(...)' expects exactly one concrete value"),
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
            let mut seen = HashSet::new();
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
                if !seen.insert(arm.variant.as_str()) {
                    return Err(diag(
                        arm.variant_span,
                        &format!("duplicate match arm for '{enum_name}.{}'", arm.variant),
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
                    }
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
                .filter(|variant| !seen.contains(variant.name.as_str()))
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
        ExprKind::Field {
            base,
            name,
            name_span,
        } => {
            let base_ty = type_of_expr(base, env, signatures)?;
            let Type::Named(struct_name) = base_ty else {
                return Err(diag(
                    *name_span,
                    &format!(
                        "field access requires a struct value, got {}",
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
                })
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
                    if matches!(left_ty, Type::Named(_)) || matches!(right_ty, Type::Named(_)) {
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
            }
        }
    }
}

fn value_types_of_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<Type>, Diagnostic> {
    match &expr.kind {
        ExprKind::Call { name, .. } if signatures.interface(name).is_some() => {
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
    if let Some(definition) = signatures.enum_type(namespace) {
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
            &format!("unknown enum or interface namespace '{namespace}'"),
        ));
    };
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
            StmtKind::If { .. }
            | StmtKind::ForRange { .. }
            | StmtKind::Match { .. }
            | StmtKind::Let { .. }
            | StmtKind::LetDestructure { .. }
            | StmtKind::LetStructDestructure { .. }
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Expr(_) => {}
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

fn evaluate_default_expr(
    expr: &Expr,
    signatures: &Signatures,
) -> Result<ConstantValue, Diagnostic> {
    match &expr.kind {
        ExprKind::Int(value) => Ok(ConstantValue::I64(*value)),
        ExprKind::Bool(value) => Ok(ConstantValue::Bool(*value)),
        ExprKind::Str(value) => Ok(ConstantValue::Str(value.clone())),
        ExprKind::Var(name) => signatures
            .constant(name)
            .map(|constant| constant.value.clone())
            .ok_or_else(|| {
                diag(
                    expr.span,
                    &format!(
                        "parameter defaults may reference only compile-time constants; '{name}' is not one"
                    ),
                )
            }),
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
                (UnaryOp::Neg, ConstantValue::I64(value)) => {
                    Ok(ConstantValue::I64(value.wrapping_neg()))
                }
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
        | ExprKind::Call { .. }
        | ExprKind::QualifiedCall { .. }
        | ExprKind::StructLiteral { .. }
        | ExprKind::Field { .. }
        | ExprKind::Match { .. } => Err(diag(
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
            if !definitions.contains_key(name.as_str()) {
                return Err(diag(
                    expr.span,
                    &format!("unknown constant '{name}' in constant expression"),
                ));
            }
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
                (UnaryOp::Neg, ConstantValue::I64(value)) => {
                    Ok(ConstantValue::I64(value.wrapping_neg()))
                }
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
        | ExprKind::Call { .. }
        | ExprKind::QualifiedCall { .. }
        | ExprKind::StructLiteral { .. }
        | ExprKind::Field { .. }
        | ExprKind::Match { .. } => Err(diag(
            expr.span,
            "constant expressions currently support primitive literals, constant references, and primitive operators",
        )),
    }
}

fn evaluate_constant_binary(
    span: SourceSpan,
    op: BinOp,
    left: ConstantValue,
    right: ConstantValue,
) -> Result<ConstantValue, Diagnostic> {
    match (op, left, right) {
        (BinOp::Add, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::I64(left.wrapping_add(right)))
        }
        (BinOp::Sub, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::I64(left.wrapping_sub(right)))
        }
        (BinOp::Mul, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::I64(left.wrapping_mul(right)))
        }
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

fn require_known_type(
    span: SourceSpan,
    ty: &Type,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    match signatures.canonical_type(ty) {
        Type::Named(name)
            if signatures.struct_type(&name).is_none()
                && signatures.enum_type(&name).is_none()
                && signatures.interface(&name).is_none() =>
        {
            Err(diag(span, &format!("unknown type '{name}'")))
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
    if expected == actual {
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

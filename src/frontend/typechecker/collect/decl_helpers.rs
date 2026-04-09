//! Collection helpers for the first pass (fields/methods + derive-driven injections).

use std::collections::HashMap;

use crate::frontend::ast::*;
use crate::frontend::symbols::{FieldInfo, MethodInfo, ResolvedType, TypeBoundInfo};
use crate::frontend::typechecker::TypeChecker;
use incan_core::lang::derives::{self, DeriveId};

/// Collect methods from method declarations into a `HashMap`.
pub(super) fn collect_methods(
    methods: &[Spanned<MethodDecl>],
    checker: &mut TypeChecker,
) -> HashMap<String, MethodInfo> {
    methods
        .iter()
        .map(|m| {
            let type_params: Vec<String> = m.node.type_params.iter().map(|tp| tp.name.clone()).collect();
            let type_param_bounds: HashMap<String, Vec<String>> = m
                .node
                .type_params
                .iter()
                .map(|tp| {
                    (
                        tp.name.clone(),
                        tp.bounds.iter().map(|bound| bound.name.clone()).collect(),
                    )
                })
                .collect();
            let type_param_bound_details: HashMap<String, Vec<TypeBoundInfo>> = m
                .node
                .type_params
                .iter()
                .map(|tp| {
                    (
                        tp.name.clone(),
                        tp.bounds
                            .iter()
                            .map(|bound| TypeBoundInfo {
                                name: bound.name.clone(),
                                type_args: bound
                                    .type_args
                                    .iter()
                                    .map(|type_arg| checker.resolve_type_checked(type_arg))
                                    .collect(),
                            })
                            .collect(),
                    )
                })
                .collect();
            let params = m
                .node
                .params
                .iter()
                .map(|p| (p.node.name.clone(), checker.resolve_type_checked(&p.node.ty)))
                .collect();
            let return_type = checker.resolve_type_checked(&m.node.return_type);
            (
                m.node.name.clone(),
                MethodInfo {
                    type_params,
                    type_param_bounds,
                    type_param_bound_details,
                    receiver: m.node.receiver,
                    params,
                    return_type,
                    is_async: m.node.is_async(),
                    has_body: m.node.body.is_some(),
                },
            )
        })
        .collect()
}

/// Collect fields from field declarations into a `HashMap`.
pub(super) fn collect_fields(fields: &[Spanned<FieldDecl>], checker: &mut TypeChecker) -> HashMap<String, FieldInfo> {
    fields
        .iter()
        .map(|f| {
            (
                f.node.name.clone(),
                FieldInfo {
                    ty: checker.resolve_type_checked(&f.node.ty),
                    has_default: f.node.default.is_some(),
                    alias: f.node.metadata.alias.clone(),
                    description: f.node.metadata.description.clone(),
                },
            )
        })
        .collect()
}

/// Inject to_json/from_json methods based on Serialize/Deserialize derives.
pub(super) fn inject_json_methods(methods: &mut HashMap<String, MethodInfo>, type_name: &str, derives: &[String]) {
    if derives
        .iter()
        .any(|d| derives::from_str(d.as_str()) == Some(DeriveId::Serialize))
    {
        methods.insert(
            "to_json".to_string(),
            MethodInfo {
                type_params: Vec::new(),
                type_param_bounds: HashMap::new(),
                type_param_bound_details: HashMap::new(),
                receiver: Some(Receiver::Immutable),
                params: vec![],
                return_type: ResolvedType::Str,
                is_async: false,
                has_body: true,
            },
        );
    }
    if derives
        .iter()
        .any(|d| derives::from_str(d.as_str()) == Some(DeriveId::Deserialize))
    {
        methods.insert(
            "from_json".to_string(),
            MethodInfo {
                type_params: Vec::new(),
                type_param_bounds: HashMap::new(),
                type_param_bound_details: HashMap::new(),
                receiver: None, // Static method
                params: vec![("json_str".to_string(), ResolvedType::Str)],
                return_type: ResolvedType::Generic(
                    "Result".to_string(),
                    vec![ResolvedType::Named(type_name.to_string()), ResolvedType::Str],
                ),
                is_async: false,
                has_body: true,
            },
        );
    }
}

/// Inject a `TypeName.new(...) -> Result[TypeName, E]` constructor for `@derive(Validate)` models.
///
/// This is a *typechecker-only* method injection to allow `User.new(...)` calls to typecheck even though the backend
/// generates the actual Rust implementation.
pub(super) fn inject_validate_methods(
    methods: &mut HashMap<String, MethodInfo>,
    _type_name: &str,
    fields: &HashMap<String, FieldInfo>,
    field_order: &[Ident],
    derives: &[String],
) {
    let has_validate = derives
        .iter()
        .any(|d| derives::from_str(d.as_str()) == Some(DeriveId::Validate));
    if !has_validate {
        return;
    }

    // Only inject if the user didn't already define it.
    if methods.contains_key("new") {
        return;
    }

    // Use the return type of validate() if present; otherwise use Unknown (second pass will report a better error).
    let return_type = methods
        .get("validate")
        .map(|m| m.return_type.clone())
        .unwrap_or(ResolvedType::Unknown);

    // Prefer required fields only (no defaults). This keeps the signature stable and avoids needing default args.
    let mut params: Vec<(String, ResolvedType)> = Vec::new();
    for field_name in field_order {
        if let Some(info) = fields.get(field_name)
            && !info.has_default
        {
            params.push((field_name.clone(), info.ty.clone()));
        }
    }

    methods.insert(
        "new".to_string(),
        MethodInfo {
            type_params: Vec::new(),
            type_param_bounds: HashMap::new(),
            type_param_bound_details: HashMap::new(),
            receiver: None, // associated function via `TypeName.new(...)`
            params,
            return_type,
            is_async: false,
            has_body: true,
        },
    );
}

mod fixer;
mod utils;

use fixer::{Fix, Fixer};
use oxc_allocator::Allocator;
use oxc_ast::{
    ast::{
        BindingPatternKind, CallExpression, Declaration, ExportDefaultDeclarationKind, Expression,
    },
    AstKind,
};
use oxc_parser::Parser;
use oxc_semantic::{AstNode, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};

use serde_json::Value;
use std::{fs, process, vec};

fn main() {
    let resolved_dir = utils::get_resolved_dir();

    println!("Working directory: {}", resolved_dir);

    let routes_raw = utils::get_remix_routes_json(&resolved_dir);
    let routes_json: Value = serde_json::from_str(&routes_raw).expect("Failed to parse JSON");

    if let Some(array) = routes_json.as_array() {
        let file_paths: Vec<String> = array
            .iter()
            .flat_map(|item| {
                let relative_files = utils::traverse_route_entry(item.clone());
                utils::get_absolute_files(relative_files, &resolved_dir)
            })
            .collect();

        println!("Found {} route files", file_paths.len());

        for file_path in file_paths.iter() {
            process_file(&file_path);
        }
    } else {
        eprintln!("Failed to parse JSON: expected an array");
        process::exit(1)
    }
}

fn process_file(file_path: &str) {
    println!("Processing file: {}", file_path);

    let source_text = fs::read_to_string(file_path).unwrap();
    let source_type = SourceType::from_path(file_path).unwrap();

    let first_pass_code = first_pass(&source_text, source_type);

    if let Err(_) = first_pass_code {
        println!("Failed to process file: {}", file_path);
        process::exit(1);
    }

    let second_pass_code = second_pass(&first_pass_code.unwrap(), source_type);

    if let Err(_) = second_pass_code {
        println!("Failed to process file: {}", file_path);
        process::exit(1);
    }

    fs::write(file_path, second_pass_code.unwrap()).expect("Failed to write file");
}

fn first_pass(source_text: &String, source_type: SourceType) -> Result<String, ()> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source_text, source_type).parse();

    if !ret.errors.is_empty() {
        for error in ret.errors {
            let error = error.with_source_code(source_text.clone());
            println!("{error:?}");
        }
        process::exit(1);
    }

    let semantic_ret = SemanticBuilder::new(&source_text, source_type)
        .with_trivias(ret.trivias)
        .build(&ret.program);

    let mut first_pass_fixes: Vec<Fix> = vec![];

    for node in semantic_ret.semantic.nodes().iter() {
        if let AstKind::CallExpression(call_expr) = node.kind() {
            if let Some(loader_span) = find_hook_type_param(call_expr, "useLoaderData") {
                first_pass_fixes.push(Fix::delete(loader_span));
            }
            if let Some(action_span) = find_hook_type_param(call_expr, "useActionData") {
                first_pass_fixes.push(Fix::delete(action_span));
            }
        }
    }

    let first_pass_code = Fixer::new(&source_text, first_pass_fixes).fix().fixed_code;

    Ok(first_pass_code.to_string())
}

fn second_pass(source_text: &String, source_type: SourceType) -> Result<String, ()> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source_text, source_type).parse();

    if !ret.errors.is_empty() {
        for error in ret.errors {
            let error = error.with_source_code(source_text.clone());
            println!("{error:?}");
        }
        process::exit(1);
    }

    let semantic_ret = SemanticBuilder::new(&source_text, source_type)
        .with_trivias(ret.trivias)
        .build(&ret.program);

    let known_remix_exports = vec![
        "handle",
        "meta",
        "links",
        "loader",
        "clientLoader",
        "action",
        "clientAction",
        "headers",
        "default",
        "ErrorBoundary",
        "shouldRevalidate",
    ];

    let mut remix_exports = vec![];

    let mut second_pass_fixes: Vec<Fix> = vec![];

    for node in semantic_ret.semantic.nodes().iter() {
        if let AstKind::ExportNamedDeclaration(named_export) = node.kind() {
            if let Some(name) = get_export_name(&node) {
                if known_remix_exports.contains(&name) {
                    remix_exports.push(RemixModuleExport {
                        key: name,
                        span: get_export_span(&node),
                    });
                    second_pass_fixes.push(Fix::delete(named_export.span));
                }
            }
        }
        if let AstKind::ExportDefaultDeclaration(default_export) = node.kind() {
            if is_new_module_default_export(&node) {
                println!("File already has a new module default export");
                return Err(());
            }
            remix_exports.push(RemixModuleExport {
                key: "Component",
                span: get_export_span(&node),
            });
            second_pass_fixes.push(Fix::delete(default_export.span));
        }
    }

    let new_export_position = source_text.len() as u32;

    second_pass_fixes.push(Fix::insert(
        construct_new_module_object(&remix_exports, source_text),
        Span::new(new_export_position, new_export_position),
    ));

    let second_pass_code = Fixer::new(&source_text, second_pass_fixes).fix().fixed_code;

    Ok(second_pass_code.to_string())
}

#[derive(Debug)]
struct RemixModuleExport<'a> {
    key: &'a str,
    span: Option<Span>,
}

fn construct_new_module_object(
    remix_exports: &Vec<RemixModuleExport>,
    source_text: &String,
) -> String {
    let mut module_object = String::from("\nexport default defineRoute({\n");

    // module_object.push_str("  params: [],\n");

    let mut exports_with_span: Vec<_> = remix_exports
        .iter()
        .filter(|export| export.span.is_some())
        .collect();

    // Keep the original order of exports
    exports_with_span.sort_by(|a, b| a.span.unwrap().start.cmp(&b.span.unwrap().start));

    for export in exports_with_span.iter() {
        module_object.push_str(&format!(
            "  {}: {},\n",
            export.key,
            export.span.unwrap().source_text(source_text)
        ));
    }

    module_object.push_str("});\n");

    module_object
}

fn is_new_module_default_export(node: &AstNode) -> bool {
    if let AstKind::ExportDefaultDeclaration(default_export) = node.kind() {
        if let ExportDefaultDeclarationKind::CallExpression(call_expr) = &default_export.declaration
        {
            if let Expression::Identifier(ident) = &call_expr.callee {
                return ident.name == "defineRoute";
            }
        }
    }

    false
}

fn get_export_name<'a>(node: &'a AstNode<'a>) -> Option<&'a str> {
    match node.kind() {
        AstKind::ExportNamedDeclaration(named_export) => {
            if let Some(Declaration::FunctionDeclaration(decl)) = &named_export.declaration {
                decl.id.as_ref().map(|id| id.name.as_str())
            } else if let Some(Declaration::VariableDeclaration(decl)) = &named_export.declaration {
                decl.declarations.iter().find_map(|d| {
                    if let BindingPatternKind::BindingIdentifier(bind) = &d.id.kind {
                        Some(bind.name.as_str())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        }
        AstKind::ExportDefaultDeclaration(default_export) => {
            if let ExportDefaultDeclarationKind::FunctionDeclaration(decl) =
                &default_export.declaration
            {
                decl.id.as_ref().map(|id| id.name.as_str())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn get_export_span(node: &AstNode) -> Option<Span> {
    match node.kind() {
        AstKind::ExportNamedDeclaration(named_export) => {
            if let Some(Declaration::FunctionDeclaration(decl)) = &named_export.declaration {
                Some(decl.span)
            } else if let Some(Declaration::VariableDeclaration(decl)) = &named_export.declaration {
                decl.declarations.iter().find_map(|d| {
                    if let BindingPatternKind::BindingIdentifier(_) = &d.id.kind {
                        if let Some(init) = &d.init {
                            Some(init.span())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        }
        AstKind::ExportDefaultDeclaration(default_export) => {
            if let ExportDefaultDeclarationKind::FunctionDeclaration(decl) =
                &default_export.declaration
            {
                Some(decl.span)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn find_hook_type_param(call_expr: &CallExpression, hook_name: &str) -> Option<Span> {
    if call_expr
        .callee_name()
        .is_some_and(|name| name == hook_name)
    {
        if let Some(type_params) = &call_expr.type_parameters {
            return Some(type_params.span);
        }
    }

    None
}

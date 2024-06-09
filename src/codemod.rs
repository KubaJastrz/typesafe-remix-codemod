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

use std::{process, vec};

use crate::fixer::{Fix, Fixer};

pub fn codemod<'a>(source_text: &'a str, source_type: SourceType) -> Result<String, ()> {
    let first_pass_code = first_pass(&source_text.to_owned(), source_type);

    if first_pass_code.is_err() {
        return Err(());
    }

    let second_pass_code = second_pass(&first_pass_code.unwrap(), source_type);

    second_pass_code
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
        "HydrateFallback",
        "shouldRevalidate",
    ];

    let mut remix_exports = vec![];

    let mut second_pass_fixes: Vec<Fix> = vec![];

    for node in semantic_ret.semantic.nodes().iter() {
        if let AstKind::ExportNamedDeclaration(named_export) = node.kind() {
            if let Some(name) = get_named_export_name(&node) {
                if known_remix_exports.contains(&name) {
                    let export_meta = get_export_function_meta(&node, source_text);
                    remix_exports.push(RemixModuleExport {
                        key: name,
                        span: export_meta.span,
                        args: export_meta.args,
                        body: export_meta.body,
                        is_async: export_meta.is_async,
                    });
                    second_pass_fixes.push(Fix::delete(named_export.span));
                }
            }
        } else if let AstKind::ExportDefaultDeclaration(default_export) = node.kind() {
            if is_new_module_default_export(&node) {
                println!("File already has a new module default export");
                return Err(());
            }
            let export_meta = get_export_function_meta(&node, source_text);
            remix_exports.push(RemixModuleExport {
                key: "Component",
                span: export_meta.span,
                args: export_meta.args,
                body: export_meta.body,
                is_async: export_meta.is_async,
            });
            second_pass_fixes.push(Fix::delete(default_export.span));
        }
    }

    if remix_exports.len() == 0 {
        return Ok("".to_owned());
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
    args: Option<&'a str>,
    body: Option<&'a str>,
    is_async: bool,
}

fn construct_new_module_object(
    remix_exports: &Vec<RemixModuleExport>,
    source_text: &String,
) -> String {
    let mut module_object = String::from("export default defineRoute({\n");

    // module_object.push_str("  params: [],\n");

    let mut exports_with_span: Vec<_> = remix_exports
        .iter()
        .filter(|export| export.span.is_some())
        .collect();

    // Keep the original order of exports
    exports_with_span.sort_by(|a, b| a.span.unwrap().start.cmp(&b.span.unwrap().start));

    for export in exports_with_span.iter() {
        if let Some(body) = export.body {
            module_object.push_str(&format!(
                "{}{}({}) {},\n",
                if export.is_async { "async " } else { "" },
                export.key,
                export.args.unwrap_or(""),
                body
            ));
        } else {
            module_object.push_str(&format!(
                "{}: {},\n",
                export.key,
                export.span.unwrap().source_text(source_text)
            ));
        }
    }

    module_object = indent::indent_all_by(2, module_object);

    module_object.push_str("});\n");

    format!("\n{}", module_object.trim_start().to_owned())
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

fn get_named_export_name<'a>(node: &'a AstNode<'a>) -> Option<&'a str> {
    match node.kind() {
        AstKind::ExportNamedDeclaration(named_export) => {
            if let Some(Declaration::FunctionDeclaration(decl)) = &named_export.declaration {
                decl.id.as_ref().map(|id| id.name.as_str())
            } else if let Some(Declaration::VariableDeclaration(decl)) = &named_export.declaration {
                decl.declarations.iter().find_map(|d| {
                    if let BindingPatternKind::BindingIdentifier(ident) = &d.id.kind {
                        Some(ident.name.as_str())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

#[derive(Debug, Default)]
struct ExportFunctionMeta<'a> {
    span: Option<Span>,
    args: Option<&'a str>,
    body: Option<&'a str>,
    is_async: bool,
}

fn get_export_function_meta<'a>(
    node: &'a AstNode<'a>,
    source_text: &'a str,
) -> ExportFunctionMeta<'a> {
    let mut meta = ExportFunctionMeta::default();

    match node.kind() {
        AstKind::ExportNamedDeclaration(named_export) => {
            if let Some(Declaration::FunctionDeclaration(decl)) = &named_export.declaration {
                meta.span = Some(decl.span);
                meta.args = Some(
                    // remove the parentheses
                    Span::new(decl.params.span.start + 1, decl.params.span.end - 1)
                        .source_text(&source_text),
                );
                if let Some(body) = &decl.body {
                    meta.body = Some(body.span.source_text(&source_text));
                }
                meta.is_async = decl.r#async;
            } else if let Some(Declaration::VariableDeclaration(decl)) = &named_export.declaration {
                if decl.declarations.len() != 1 {
                    return meta;
                }

                if let Some(d) = decl.declarations.first() {
                    if let BindingPatternKind::BindingIdentifier(_) = &d.id.kind {
                        if let Some(init) = &d.init {
                            meta.span = Some(init.span());

                            match init {
                                Expression::FunctionExpression(func) => {
                                    meta.args = Some(
                                        // remove the parentheses
                                        Span::new(
                                            func.params.span.start + 1,
                                            func.params.span.end - 1,
                                        )
                                        .source_text(&source_text),
                                    );
                                    if let Some(body) = &func.body {
                                        meta.body = Some(body.span.source_text(&source_text))
                                    }
                                    meta.is_async = func.r#async;
                                }
                                Expression::ArrowFunctionExpression(arrow_func) => {
                                    // Don't use shorthand for arrow functions with implicit returns, like `() => stuff`
                                    if arrow_func.expression {
                                        return meta;
                                    }
                                    meta.args = Some(
                                        // remove the parentheses
                                        Span::new(
                                            arrow_func.params.span.start + 1,
                                            arrow_func.params.span.end - 1,
                                        )
                                        .source_text(&source_text),
                                    );
                                    meta.body =
                                        Some(&arrow_func.body.span.source_text(&source_text));
                                    meta.is_async = arrow_func.r#async;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        AstKind::ExportDefaultDeclaration(default_export) => {
            match &default_export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(decl) => {
                    meta.span = Some(decl.span);
                    meta.args = Some(
                        // remove the parentheses
                        Span::new(decl.params.span.start + 1, decl.params.span.end - 1)
                            .source_text(&source_text),
                    );
                    if let Some(body) = &decl.body {
                        meta.body = Some(body.span.source_text(&source_text))
                    }
                    meta.is_async = decl.r#async;
                }
                ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow_func) => {
                    // Don't use shorthand for arrow functions with implicit returns, like `() => stuff`
                    if arrow_func.expression {
                        meta.span = Some(arrow_func.span);
                        return meta;
                    }
                    meta.span = Some(arrow_func.span);
                    meta.args = Some(
                        // remove the parentheses
                        Span::new(
                            arrow_func.params.span.start + 1,
                            arrow_func.params.span.end - 1,
                        )
                        .source_text(&source_text),
                    );
                    meta.body = Some(&arrow_func.body.span.source_text(&source_text));
                    meta.is_async = arrow_func.r#async;
                }
                _ => {}
            }
        }
        _ => {}
    }

    meta
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

#[cfg(test)]
mod tests {
    use oxc_span::SourceType;

    use std::cmp;

    use super::codemod;

    #[test]
    fn test_empty() {
        let source_type = SourceType::from_path("path/to/file.tsx").unwrap();
        assert_eq!(codemod("", source_type).unwrap(), "");
    }

    #[test]
    fn test_kitchen_sink() {
        let input = r#"
            import { ActionFunctionArgs, LoaderFunctionArgs } from "@remix-run/node";
            import { useLoaderData } from "@remix-run/react";

            export const meta = () => [{ title }];

            const title = "User page";

            export function action({ params, response }: ActionFunctionArgs) {
              response.status = 307;
              response.headers.set("Location", "/login");
              return response;
            }

            export const loader = async ({ params }: LoaderFunctionArgs) => {
              const { userId } = params;
              return { userId };
            };

            export default function Splat() {
              const data = useLoaderData<typeof loader>();
              return <h1>User: {data.userId}</h1>;
            }
        "#;
        assert_snapshot("kitchen_sink", input);
    }

    #[test]
    fn test_component_function_anonymous() {
        let input = r#"
            export default function() {
              return <div>hello</div>;
            }
        "#;
        assert_snapshot("component_function_anonymous", input);
    }

    #[test]
    fn test_component_function_named() {
        let input = r#"
            export default function Route() {
              return <div>hello</div>;
            }
        "#;
        assert_snapshot("component_function_named", input);
    }

    #[test]
    fn test_component_arrow_function() {
        let input = r#"
            export default () => {
              return <div>hello</div>;
            }
        "#;
        assert_snapshot("component_arrow_function", input);
    }

    #[test]
    fn test_component_arrow_function_expression() {
        let input = r#"
            export default () => <div>hello</div>;
        "#;
        assert_snapshot("component_arrow_function_expression", input);
    }

    #[test]
    fn test_loader_function_named() {
        let input = r#"
            export function loader() {
              return { hello: "world" };
            }
        "#;
        assert_snapshot("loader_function_named", input);
    }

    #[test]
    fn test_loader_function_named_args() {
        let input = r#"
            import type { LoaderFunctionArgs } from "@remix-run/node";

            export function loader({ params, context, request, response }: LoaderFunctionArgs) {
              return { hello: "world" };
            }
        "#;
        assert_snapshot("loader_function_named_args", input);
    }

    #[test]
    fn test_loader_arrow_function() {
        let input = r#"
            export const loader = () => {
              return { hello: "world" };
            }
        "#;
        assert_snapshot("loader_arrow_function", input);
    }

    #[test]
    fn test_loader_arrow_function_args() {
        let input = r#"
            import type { LoaderFunctionArgs } from "@remix-run/node";

            export const loader = ({ params, context, request, response }: LoaderFunctionArgs) => {
              return { hello: "world" };
            }
        "#;
        assert_snapshot("loader_arrow_function_args", input);
    }

    #[test]
    fn test_loader_arrow_function_expression() {
        let input = r#"
            export const loader = () => ({ hello: "world" });
        "#;
        assert_snapshot("loader_arrow_function_expression", input);
    }

    #[test]
    fn test_loader_arrow_function_expression_args() {
        let input = r#"
            import type { LoaderFunctionArgs } from "@remix-run/node";

            export const loader = ({ params, context, request, response }: LoaderFunctionArgs) => ({ hello: "world" });
        "#;
        assert_snapshot("loader_arrow_function_expression_args", input);
    }

    #[test]
    fn test_loader_function_named_async() {
        let input = r#"
            export async function loader() {
              return { hello: "world" };
            }
        "#;
        assert_snapshot("loader_function_named_async", input);
    }

    #[test]
    fn test_loader_arrow_function_async() {
        let input = r#"
            export const loader = async () => {
              return { hello: "world" };
            }
        "#;
        assert_snapshot("loader_arrow_function_async", input);
    }

    #[test]
    fn test_loader_arrow_function_expression_async() {
        let input = r#"
            export const loader = async () => ({ hello: "world" });
        "#;
        assert_snapshot("loader_arrow_function_expression_async", input);
    }

    #[test]
    fn test_component_loader() {
        let input = r#"
            import { useLoaderData } from "@remix-run/react";

            export function loader() {
              return { hello: "world" };
            }

            export default function() {
              const data = useLoaderData<typeof loader>();
              return <h1>{data.hello}</h1>;
            }
        "#;
        assert_snapshot("component_loader", input);
    }

    #[test]
    fn test_component_action() {
        let input = r#"
            import { useActionData } from "@remix-run/react";

            export function action() {
              return { hello: "world" };
            }

            export default function() {
              const data = useActionData<typeof loader>();
              return <h1>{data.hello}</h1>;
            }
        "#;
        assert_snapshot("component_action", input);
    }

    #[test]
    fn test_component_loader_action() {
        let input = r#"
            import { useActionData, useLoaderData } from "@remix-run/react";

            export function loader() {
              return { loader: "hello" };
            }

            export function action() {
              return { action: "world" };
            }

            export default function() {
              const loaderData = useActionData<typeof loader>();
              const actionData = useActionData<typeof action>();
              return <h1>{loaderData.loader} {actionData.action}</h1>;
            }
        "#;
        assert_snapshot("component_loader_action", input);
    }

    fn assert_snapshot(name: &str, input: &str) {
        let input = outdent(input);
        let source_type = SourceType::from_path("path/to/file.tsx").unwrap();
        insta::with_settings!({
            prepend_module_to_snapshot => false,
            description => &input,
        }, {
            insta::assert_snapshot!(name, codemod(&input, source_type).unwrap());
        })
    }

    /// Remove leading whitespace from each line, preserving relative indentation.
    /// Remove the first and the last lines.
    fn outdent(input: &str) -> String {
        let length = input.len();
        let mut output = String::with_capacity(length);

        let mut base_indent = 0;

        let input_body = skip_last(input.lines().skip(1));

        for (i, line) in input_body.enumerate() {
            if i == 0 {
                base_indent = line.chars().take_while(|c| c.is_whitespace()).count();
            }

            let indent = line.chars().take_while(|c| c.is_whitespace()).count();
            let indent = cmp::min(base_indent, indent);

            output.push_str(&line[indent..]);
            output.push('\n');
        }

        output
    }

    fn skip_last<T>(mut iter: impl Iterator<Item = T>) -> impl Iterator<Item = T> {
        let last = iter.next();
        iter.scan(last, |state, item| std::mem::replace(state, Some(item)))
    }
}

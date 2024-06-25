use oxc_allocator::Allocator;
use oxc_ast::{
    ast::{
        AssignmentTarget, BindingPatternKind, Declaration, ExportDefaultDeclarationKind,
        Expression, VariableDeclaration,
    },
    AstKind,
};
use oxc_parser::Parser;
use oxc_semantic::{AstNode, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};

use std::{cmp::Ordering, process, vec};

use crate::{
    codemod_models::{self, DefineRouteProperty, Method, StaticProperty},
    fixer::{Fix, Fixer},
};

use codemod_models::HookDeclarator;

pub fn codemod(source_text: &String, source_type: SourceType) -> Result<String, ()> {
    //==========================================================================
    // First pass
    //==========================================================================

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

    let mut fixes = vec![];
    let mut remix_exports = vec![];
    let mut hook_declarators = vec![];

    for node in semantic_ret.semantic.nodes().iter() {
        match node.kind() {
            AstKind::VariableDeclaration(var_decl) => {
                if let Some((whole_declaration, declarator_id)) =
                    find_hook_usage(var_decl, "useLoaderData")
                {
                    fixes.push(Fix::delete_with_leading_whitespace(whole_declaration));
                    hook_declarators.push(HookDeclarator {
                        name: "loaderData",
                        source_text: declarator_id.source_text(source_text),
                    });
                }
                if let Some((whole_declaration, declarator_id)) =
                    find_hook_usage(var_decl, "useActionData")
                {
                    fixes.push(Fix::delete_with_leading_whitespace(whole_declaration));
                    hook_declarators.push(HookDeclarator {
                        name: "actionData",
                        source_text: declarator_id.source_text(source_text),
                    });
                }
            }
            AstKind::ExpressionStatement(expr_stmt) => {
                if let Expression::AssignmentExpression(assignment_expr) = &expr_stmt.expression {
                    if matches!(
                        &assignment_expr.left,
                        AssignmentTarget::StaticMemberExpression(member_expr)
                            if matches!(&member_expr.object, Expression::Identifier(ident) if ident.name == "clientLoader")
                            && member_expr.property.name == "hydrate"
                    ) {
                        let value = assignment_expr.right.span().source_text(source_text);
                        remix_exports.push(DefineRouteProperty::StaticProperty(StaticProperty {
                            key: "clientLoaderHydrate",
                            value,
                        }));
                        fixes.push(Fix::delete_with_leading_whitespace(expr_stmt.span));
                    }
                }
            }
            _ => {}
        }
    }

    let source_text = Fixer::new(&source_text, fixes).fix().fixed_code;

    //==========================================================================
    // Second pass
    //==========================================================================

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source_text, source_type).parse();

    let semantic_ret = SemanticBuilder::new(&source_text, source_type)
        .with_trivias(ret.trivias)
        .build(&ret.program);

    // TODO: There are also `headers` and `handle`
    let known_remix_exports = vec![
        "links",
        "HydrateFallback",
        "loader",
        "clientLoader",
        "action",
        "clientAction",
        "meta",
        "default",
        "ErrorBoundary",
        "shouldRevalidate",
    ];

    let mut fixes = vec![];

    for node in semantic_ret.semantic.nodes().iter() {
        if let AstKind::ExportNamedDeclaration(named_export) = node.kind() {
            if let Some(name) = get_named_export_name(&node) {
                if known_remix_exports.contains(&name) {
                    let export_meta = get_export_function_meta(&node, &source_text);
                    remix_exports.push(DefineRouteProperty::Method(Method {
                        key: export_meta.new_name.unwrap_or(name),
                        span: export_meta.span,
                        args: export_meta.args,
                        body: export_meta.body,
                        is_async: export_meta.is_async,
                    }));
                    fixes.push(Fix::delete_with_leading_whitespace(named_export.span));
                }
            }
        } else if let AstKind::ExportDefaultDeclaration(default_export) = node.kind() {
            if is_new_module_default_export(&node) {
                println!("File already has a new module default export");
                return Err(());
            }
            let export_meta = get_export_function_meta(&node, &source_text);
            remix_exports.push(DefineRouteProperty::Method(Method {
                key: "Component",
                span: export_meta.span,
                args: construct_component_params(&hook_declarators),
                body: export_meta.body,
                is_async: export_meta.is_async,
            }));
            fixes.push(Fix::delete_with_leading_whitespace(default_export.span));
        }
    }

    if remix_exports.len() == 0 {
        return Ok("".to_owned());
    }

    let new_export_position = source_text.len() as u32;

    fixes.push(Fix::insert(
        construct_new_module_object(&mut remix_exports, &source_text),
        Span::new(new_export_position, new_export_position),
    ));

    let fixed_code = Fixer::new(&source_text, fixes).fix().fixed_code.to_string();

    Ok(fixed_code)
}

fn construct_new_module_object<'a>(
    properties: &mut Vec<DefineRouteProperty>,
    source_text: &'a str,
) -> String {
    let mut module_object = String::from("export default defineRoute({\n");

    // module_object.push_str("  params: [],\n");

    // Keep the original order of exports, put static properties at the end
    // TODO: sort by predefined order, as in `known_remix_exports`
    properties.sort_by(|a, b| match (a, b) {
        (DefineRouteProperty::StaticProperty(a), DefineRouteProperty::StaticProperty(b)) => {
            a.key.cmp(&b.key)
        }
        (DefineRouteProperty::Method(a), DefineRouteProperty::Method(b)) => {
            a.span.unwrap().cmp(&b.span.unwrap())
        }
        (DefineRouteProperty::StaticProperty(_), DefineRouteProperty::Method(_)) => {
            Ordering::Greater
        }
        (DefineRouteProperty::Method(_), DefineRouteProperty::StaticProperty(_)) => Ordering::Less,
    });

    for export in properties.iter() {
        match export {
            DefineRouteProperty::StaticProperty(static_prop) => {
                module_object.push_str(&format!("{}: {},\n", static_prop.key, static_prop.value));
            }
            DefineRouteProperty::Method(method) => {
                // TODO: a Method should always have `body`, otherwise it should be a StaticProperty
                if let Some(body) = method.body {
                    module_object.push_str(&format!(
                        "{}{}({}) {},\n",
                        if method.is_async { "async " } else { "" },
                        method.key,
                        method.args.as_ref().unwrap_or(&String::from("")),
                        body,
                    ));
                } else {
                    module_object.push_str(&format!(
                        "{}: {},\n",
                        method.key,
                        method.span.unwrap().source_text(source_text)
                    ));
                }
            }
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
    args: Option<String>,
    body: Option<&'a str>,
    is_async: bool,
    new_name: Option<&'a str>,
}

// TODO: what about class components? (default Component, named ErrorBoundary, named HydrateFallback)
fn get_export_function_meta<'a>(
    node: &'a AstNode<'a>,
    source_text: &'a str,
) -> ExportFunctionMeta<'a> {
    let mut meta = ExportFunctionMeta::default();

    match node.kind() {
        AstKind::ExportNamedDeclaration(named_export) => {
            if let Some(Declaration::FunctionDeclaration(decl)) = &named_export.declaration {
                meta.new_name = rename_exports(decl.id.as_ref().map(|id| id.name.as_str()));
                meta.span = Some(decl.span);
                meta.args = Some(
                    // remove the parentheses
                    Span::new(decl.params.span.start + 1, decl.params.span.end - 1)
                        .source_text(&source_text)
                        .to_owned(),
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
                            meta.new_name =
                                rename_exports(d.id.get_identifier().map(|i| i.as_str()));
                            meta.span = Some(init.span());

                            match init {
                                Expression::FunctionExpression(func) => {
                                    meta.args = Some(
                                        // remove the parentheses
                                        Span::new(
                                            func.params.span.start + 1,
                                            func.params.span.end - 1,
                                        )
                                        .source_text(&source_text)
                                        .to_owned(),
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
                                        .source_text(&source_text)
                                        .to_owned(),
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
                            .source_text(&source_text)
                            .to_owned(),
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
                        .source_text(&source_text)
                        .to_owned(),
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

fn rename_exports<'a>(old_name: Option<&'a str>) -> Option<&str> {
    match old_name {
        Some("loader") => Some("serverLoader"),
        Some("action") => Some("serverAction"),
        _ => None,
    }
}

fn find_hook_usage(var_decl: &VariableDeclaration, hook_name: &str) -> Option<(Span, Span)> {
    // Let's only care about single declarator, for now
    if var_decl.declarations.len() != 1 {
        return None;
    }

    let declarator = var_decl.declarations.first().unwrap();

    if let Some(Expression::CallExpression(call_expr)) = &declarator.init {
        if let Expression::Identifier(ident) = &call_expr.callee {
            if ident.name == hook_name {
                return Some((var_decl.span, declarator.id.span()));
            }
        }
    }

    None
}

fn construct_component_params(hook_declarators: &Vec<HookDeclarator>) -> Option<String> {
    let mut params = vec![];

    for declarator in hook_declarators.iter() {
        let param = if declarator.name == declarator.source_text {
            format!("{}", declarator.name)
        } else {
            format!("{}: {}", declarator.name, declarator.source_text)
        };
        params.push(param.to_owned());
    }

    if params.len() == 0 {
        return None;
    }

    Some(format!("{{ {} }}", params.join(", ")))
}

#[cfg(test)]
mod tests {
    use oxc_span::SourceType;

    use std::cmp;

    use super::codemod;

    #[test]
    fn test_empty() {
        let source_type = SourceType::from_path("path/to/file.tsx").unwrap();
        assert_eq!(codemod(&"".to_owned(), source_type).unwrap(), "");
    }

    #[test]
    fn test_kitchen_sink() {
        let input = r#"
            import {
              ActionFunctionArgs, LoaderFunctionArgs, LinksFunction, HeadersFunction,
              ClientActionFunctionArgs, ClientLoaderFunctionArgs, ShouldRevalidateFunction
            } from "@remix-run/node";
            import { useLoaderData } from "@remix-run/react";

            export const handle = {
              its: "all yours",
            };

            export const headers: HeadersFunction = ({ actionHeaders, errorHeaders, loaderHeaders, parentHeaders }) => ({
              "X-Stretchy-Pants": "its for fun",
              "Cache-Control": loaderHeaders.get("Cache-Control"),
            });

            export const meta = () => [{ title }];
            const title = "User page";

            export function action({ params, response }: ActionFunctionArgs) {
              response.status = 307;
              response.headers.set("Location", "/login");
              return response;
            }

            export const clientAction = async ({ request, params, serverAction }: ClientActionFunctionArgs) => {
              console.log('I am a client action');
              return await serverAction();
            };

            export const loader = async ({ params }: LoaderFunctionArgs) => {
              const { userId } = params;
              return { userId };
            };

            export const clientLoader = async ({ request, params, serverLoader }: ClientLoaderFunctionArgs) => {
              const serverData = await serverLoader();
              const data = getDataFromClient();
              return data;
            };

            export function HydrateFallback() {
              return <p>Loading Game...</p>;
            }

            export default function Splat() {
              const data = useLoaderData<typeof loader>();
              return <h1>User: {data.userId}</h1>;
            }

            export function ErrorBoundary() {
              const error = useRouteError();
              return <h1>Something went wrong</h1>;
            }

            export const links: LinksFunction = () => ([
              { rel: "icon", href: "/favicon.png", type: "image/png" },
              { rel: "stylesheet", href: "https://example.com/some/styles.css" },
            ]);

            export const shouldRevalidate: ShouldRevalidateFunction = ({
              actionResult, currentParams, currentUrl, defaultShouldRevalidate,
              formAction, formData, formEncType, formMethod, nextParams, nextUrl
            }) => {
              return true;
            };
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
    fn test_component_loader_binding_identifier() {
        let input = r#"
            import { useLoaderData } from "@remix-run/react";

            export function loader() {
              return { hello: "world" };
            }

            export default function() {
              const { hello } = useLoaderData<typeof loader>();
              return <h1>{hello}</h1>;
            }
        "#;
        assert_snapshot("component_loader_binding_identifier", input);
    }

    #[test]
    fn test_component_loader_binding_rest() {
        let input = r#"
            import { useLoaderData } from "@remix-run/react";

            export function loader() {
              return { hello: "world", foo: "bar" };
            }

            export default function() {
              const { hello, ...rest } = useLoaderData<typeof loader>();
              return <h1>{hello}</h1>;
            }
        "#;
        assert_snapshot("component_loader_binding_identifier_rest", input);
    }

    #[test]
    fn test_component_loader_binding_assignment() {
        let input = r#"
            import { useLoaderData } from "@remix-run/react";

            export function loader() {
              return { hello: "world", foo: "bar" };
            }

            export default function() {
              const { hello, maybe = "not" } = useLoaderData<typeof loader>();
              return <h1>{hello}</h1>;
            }
        "#;
        assert_snapshot("component_loader_binding_assignment", input);
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
              const loaderData = useLoaderData<typeof loader>();
              const actionData = useActionData<typeof action>();
              return <h1>{loaderData.loader} {actionData.action}</h1>;
            }
        "#;
        assert_snapshot("component_loader_action", input);
    }

    #[test]
    fn test_component_client_loader_hydrate() {
        let input = r#"
            import type { LoaderFunctionArgs, ClientLoaderFunctionArgs } from "@remix-run/node";
            import { useLoaderData } from "@remix-run/react";

            export async function loader({ request }: LoaderFunctionArgs) {
              const partialData = await getPartialDataFromDb({ request });
              return json(partialData);
            }

            export async function clientLoader({ request, serverLoader }: ClientLoaderFunctionArgs) {
              const [serverData, clientData] = await Promise.all([
                serverLoader(),
                getClientData(request),
              ]);
              return { ...serverData, ...clientData };
            }
            clientLoader.hydrate = true;

            export function HydrateFallback() {
              return <p>Skeleton rendered during SSR</p>;
            }

            export default function Component() {
              const data = useLoaderData();
              return <pre>{JSON.stringify(data, null, 2)}</pre>;
            }
        "#;
        assert_snapshot("component_client_loader_hydrate", input);
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

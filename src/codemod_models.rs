use oxc_span::Span;

pub struct HookDeclarator<'a> {
    pub name: &'a str,
    pub source_text: &'a str,
}

pub enum DefineRouteProperty<'a> {
    StaticProperty(StaticProperty<'a>),
    Method(Method<'a>),
}

#[derive(Debug)]
pub struct StaticProperty<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug)]
pub struct Method<'a> {
    pub key: &'a str,
    pub span: Option<Span>,
    pub args: Option<String>,
    pub body: Option<&'a str>,
    pub is_async: bool,
}

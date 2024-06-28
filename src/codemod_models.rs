use oxc_span::Span;

#[derive(Debug, Clone)]
pub struct HookDeclarator<'a> {
    pub name: &'a str,
    pub source_text: &'a str,
}

#[derive(Debug, Clone)]
pub enum DefineRouteProperty<'a> {
    StaticProperty(StaticProperty<'a>),
    Method(Method<'a>),
}

impl<'a> DefineRouteProperty<'a> {
    pub fn default_name(&self, new_key: &'a str) -> Self {
        match self {
            DefineRouteProperty::StaticProperty(p) => {
                DefineRouteProperty::StaticProperty(StaticProperty {
                    key: if p.key == "$" { new_key } else { p.key },
                    value: p.value,
                })
            }
            DefineRouteProperty::Method(p) => DefineRouteProperty::Method(Method {
                key: if p.key == "$" { new_key } else { p.key },
                span: p.span,
                args: p.args.clone(),
                body: p.body,
                is_async: p.is_async,
            }),
        }
    }

    pub fn set_args(&self, new_args: String) -> Self {
        match self {
            DefineRouteProperty::Method(p) => DefineRouteProperty::Method(Method {
                key: p.key,
                span: p.span,
                args: new_args,
                body: p.body,
                is_async: p.is_async,
            }),
            _ => self.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StaticProperty<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone)]
pub struct Method<'a> {
    pub key: &'a str,
    pub span: Span,
    pub args: String,
    pub body: &'a str,
    pub is_async: bool,
}

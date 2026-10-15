/// Accept nonempty documentation; inheritance placeholders need an eligible parent.
pub(super) fn has_documentation(attrs: &[syn::Attribute], allow_inheritance: bool) -> bool {
    attrs.iter().any(|attr| {
        if let Some(text) = doc_text(attr) {
            text.lines().any(|line| {
                let line = line.trim();
                if is_inheritance_marker(line) {
                    allow_inheritance
                } else {
                    !line.is_empty()
                }
            })
        } else if attr.path().is_ident("doc") {
            matches!(&attr.meta, syn::Meta::NameValue(doc) if matches!(doc.value, syn::Expr::Macro(_)))
        } else {
            false
        }
    })
}

fn is_inheritance_marker(line: &str) -> bool {
    let line = line.trim();
    let marker = line
        .strip_prefix("<!--")
        .and_then(|comment| comment.strip_suffix("-->"))
        .unwrap_or(line)
        .trim();
    matches!(marker, "<inheritdoc />" | "<inheritdoc/>")
}

fn doc_text(attr: &syn::Attribute) -> Option<String> {
    if !attr.path().is_ident("doc") {
        return None;
    }
    match &attr.meta {
        syn::Meta::NameValue(syn::MetaNameValue {
            value:
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }),
            ..
        }) => Some(text.value()),
        _ => None,
    }
}

//! Plain substitution of live quantities into lesson text. `{{path}}`,
//! `{{path:.3f}}` and `{{path:,}}` only; nothing is evaluated, and a value
//! the experiment did not produce is shown as `[missing: path]` rather than
//! blank or zero.
use serde_json::Value;

/// A nested object of live quantities; paths are dotted, array indices are
/// plain numbers (`byhand.round.0.mse_after`).
#[derive(Clone, Debug, Default)]
pub struct Context(pub Value);

impl Context {
    pub fn empty() -> Self {
        Context(Value::Object(Default::default()))
    }
    pub fn get(&self, path: &str) -> Option<&Value> {
        let mut cur = &self.0;
        for part in path.split('.') {
            cur = match cur {
                Value::Object(o) => o.get(part)?,
                Value::Array(a) => a.get(part.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(cur)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub text: String,
    /// Paths that resolved to nothing (`coef.x9`) and invalid placeholders
    /// (`invalid: #if a`), each listed once.
    pub missing: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum Format {
    Plain,
    Fixed(usize),
    Thousands,
}

fn is_path(s: &str) -> bool {
    !s.is_empty()
        && s.split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
}

fn parse_placeholder(inner: &str) -> Option<(&str, Format)> {
    match inner.split_once(':') {
        None => is_path(inner).then_some((inner, Format::Plain)),
        Some((path, spec)) => {
            if !is_path(path) {
                return None;
            }
            if spec == "," {
                return Some((path, Format::Thousands));
            }
            let digits = spec.strip_prefix('.')?.strip_suffix('f')?;
            let n: usize = digits.parse().ok()?;
            (n <= 10).then_some((path, Format::Fixed(n)))
        }
    }
}

fn thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

fn format_value(v: &Value, fmt: &Format) -> Option<String> {
    match (v, fmt) {
        (Value::Number(n), Format::Plain) => Some(n.to_string()),
        (Value::Number(n), Format::Fixed(d)) => n.as_f64().map(|f| format!("{f:.d$}")),
        (Value::Number(n), Format::Thousands) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.round() as i64))
            .map(thousands),
        (Value::String(s), Format::Plain) => Some(s.clone()),
        (Value::Bool(b), Format::Plain) => Some(b.to_string()),
        _ => None,
    }
}

/// Substitute every placeholder in `template`.
pub fn render(template: &str, ctx: &Context) -> Rendered {
    let mut text = String::with_capacity(template.len());
    let mut missing: Vec<String> = vec![];
    let mut note = |m: String, missing: &mut Vec<String>| {
        if !missing.contains(&m) {
            missing.push(m);
        }
    };
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        text.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            text.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &after[..end];
        let raw = &rest[start..start + 2 + end + 2];
        match parse_placeholder(inner) {
            Some((path, fmt)) => match ctx.get(path).and_then(|v| format_value(v, &fmt)) {
                Some(s) => text.push_str(&s),
                None => {
                    text.push_str(&format!("[missing: {path}]"));
                    note(path.to_string(), &mut missing);
                }
            },
            None => {
                text.push_str(raw);
                note(format!("invalid: {inner}"), &mut missing);
            }
        }
        rest = &after[end + 2..];
    }
    text.push_str(rest);
    Rendered { text, missing }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_nested_paths_and_formats() {
        let ctx = Context(json!({"metric":{"valid_mse":3.25},"rows":{"train":2250}}));
        let r = render("{{metric.valid_mse:.3f}} on {{rows.train:,}} rows", &ctx);
        assert_eq!(r.text, "3.250 on 2,250 rows");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn missing_path_is_visible_and_reported() {
        let r = render("{{coef.x9}}", &Context::empty());
        assert_eq!(r.text, "[missing: coef.x9]");
        assert_eq!(r.missing, vec!["coef.x9"]);
    }

    #[test]
    fn expressions_are_not_evaluated() {
        let r = render("{{#if a}}x{{/if}}", &Context(json!({"a": true})));
        assert_eq!(r.text, "{{#if a}}x{{/if}}");
        assert!(r.missing.contains(&"invalid: #if a".to_string()));
    }

    #[test]
    fn array_indices_and_integers() {
        let ctx = Context(json!({"byhand":{"round":[{"mse_after":3.25}]},"n":4}));
        assert_eq!(render("{{byhand.round.0.mse_after}}", &ctx).text, "3.25");
        assert_eq!(render("{{n}}", &ctx).text, "4");
    }

    #[test]
    fn bad_format_spec_is_invalid() {
        let r = render("{{x:.12f}}", &Context(json!({"x": 1.0})));
        assert_eq!(r.text, "{{x:.12f}}");
        assert_eq!(r.missing, vec!["invalid: x:.12f"]);
    }
}

//! Lesson Markdown to HTML: mathematics is lifted out before parsing,
//! the HTML is sanitized, and the mathematics is put back as elements
//! carrying their TeX source (`data-tex`), which the webview renders with
//! KaTeX and which remains the text alternative if it does not.
use pulldown_cmark::{html, Options, Parser};

struct Math {
    tex: String,
    display: bool,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Replace `$…$` and `$$…$$` outside code with placeholders. Code spans and
/// fenced blocks are tracked so a dollar sign inside them is left alone.
fn extract_math(src: &str) -> (String, Vec<Math>) {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut maths = vec![];
    let mut i = 0;
    let mut in_fence = false;
    let mut at_line_start = true;
    while i < bytes.len() {
        if at_line_start && src[i..].starts_with("```") {
            in_fence = !in_fence;
            let end = src[i..].find('\n').map(|e| i + e + 1).unwrap_or(bytes.len());
            out.push_str(&src[i..end]);
            i = end;
            at_line_start = true;
            continue;
        }
        let c = bytes[i];
        if in_fence {
            out.push(c as char);
            at_line_start = c == b'\n';
            i += 1;
            continue;
        }
        if c == b'`' {
            // copy a code span verbatim
            let ticks = src[i..].chars().take_while(|&ch| ch == '`').count();
            let fence = "`".repeat(ticks);
            if let Some(close) = src[i + ticks..].find(&fence) {
                let end = i + ticks + close + ticks;
                out.push_str(&src[i..end]);
                i = end;
                at_line_start = false;
                continue;
            }
        }
        if c == b'$' {
            let display = src[i..].starts_with("$$");
            let delim = if display { "$$" } else { "$" };
            let start = i + delim.len();
            if let Some(close) = src[start..].find(delim) {
                let tex = &src[start..start + close];
                if !tex.is_empty() && !tex.starts_with(char::is_whitespace) || display {
                    maths.push(Math { tex: tex.trim().to_string(), display });
                    out.push_str(&format!("MLMATH{}MLMATH", maths.len() - 1));
                    i = start + close + delim.len();
                    at_line_start = false;
                    continue;
                }
            }
        }
        out.push(c as char);
        at_line_start = c == b'\n';
        i += 1;
    }
    (out, maths)
}

/// Lesson Markdown to sanitized HTML with math placeholders filled in.
pub fn to_html(markdown: &str) -> String {
    let (stripped, maths) = extract_math(markdown);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let mut raw = String::new();
    html::push_html(&mut raw, Parser::new_ext(&stripped, options));
    let mut builder = ammonia::Builder::default();
    builder.add_tag_attributes("span", &["class", "data-tex"]);
    builder.add_tag_attributes("div", &["class", "data-tex"]);
    builder.add_tag_attributes("table", &["class"]);
    builder.add_tag_attributes("td", &["align"]);
    builder.add_tag_attributes("th", &["align"]);
    // Lessons do not get hyperlinks: a link would navigate the webview
    // away from the application. The link text stays.
    builder.rm_tags(&["a"]);
    let mut clean = builder.clean(&raw).to_string();
    for (k, m) in maths.iter().enumerate() {
        let tex = escape(&m.tex);
        let element = if m.display {
            format!("<div class=\"math math-display\" data-tex=\"{tex}\">{tex}</div>")
        } else {
            format!("<span class=\"math\" data-tex=\"{tex}\">{tex}</span>")
        };
        let placeholder = format!("MLMATH{k}MLMATH");
        clean = clean.replace(&format!("<p>{placeholder}</p>"), &element).replace(&placeholder, &element);
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_survives_markdown_and_sanitizer() {
        let h = to_html("Loss $\\frac{1}{n}\\sum e_i^2$ here");
        assert!(h.contains("data-tex=\"\\frac{1}{n}\\sum e_i^2\""), "{h}");
        assert!(h.contains("class=\"math\""), "{h}");
    }

    #[test]
    fn display_math_becomes_a_block() {
        let h = to_html("$$ y = f(x) $$");
        assert!(h.contains("<div class=\"math math-display\""), "{h}");
    }

    #[test]
    fn scripts_handlers_and_javascript_links_are_removed() {
        let h = to_html(
            "<script>x</script>\n\n<a onclick=\"evil()\" href=\"#\">a</a>\n\n[a](javascript:alert(1))\n\nA following paragraph.",
        );
        assert!(!h.contains("script"), "{h}");
        assert!(!h.contains("onclick"), "{h}");
        assert!(!h.contains("javascript:"), "{h}");
        assert!(h.contains("A following paragraph."), "{h}");
    }

    #[test]
    fn dollar_inside_code_is_not_math() {
        let h = to_html("Pay `$5` now\n\n```\n$x$\n```\n");
        assert!(!h.contains("data-tex"), "{h}");
        assert!(h.contains("$5"), "{h}");
    }

    #[test]
    fn links_do_not_leave_the_webview() {
        let h = to_html("See [the paper](https://example.org/x) and <a href=\"https://example.org\">this</a>.");
        assert!(!h.contains("<a "), "{h}");
        assert!(!h.contains("href"), "{h}");
        assert!(h.contains("the paper") && h.contains("this"), "{h}");
    }

    #[test]
    fn tables_render() {
        let h = to_html("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(h.contains("<table>"), "{h}");
    }
}

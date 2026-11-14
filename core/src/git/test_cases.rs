//! Syntax trees keep fixture strings, JSX text and regexes distinct from executable tests.
use super::tamper::TestCase;
use tree_sitter::{Node, Parser};

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn case(cases: &mut Vec<TestCase>, scope: &str, title: String) -> usize {
    let base = format!("{scope}{title}");
    let mut name = base.clone();
    let mut suffix = 1;
    while cases.iter().any(|c| c.name == name) {
        suffix += 1;
        name = format!("{base}#{suffix}");
    }
    cases.push(TestCase {
        name,
        title,
        assertions: 0,
    });
    cases.len() - 1
}

fn is_rust_test(node: Node<'_>, source: &str) -> bool {
    let mut previous = node.prev_named_sibling();
    while let Some(attr) = previous {
        match attr.kind() {
            "attribute_item" => {
                let attr: String = text(attr, source)
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                if attr == "#[test]" || attr.starts_with("#[tokio::test") {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        previous = attr.prev_named_sibling();
    }
    false
}

fn rust_walk(
    node: Node<'_>,
    source: &str,
    scope: &str,
    active: Option<usize>,
    cases: &mut Vec<TestCase>,
) {
    if node.kind() == "mod_item" {
        if let (Some(name), Some(body)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("body"),
        ) {
            rust_walk(
                body,
                source,
                &format!("{scope}{}::", text(name, source)),
                active,
                cases,
            );
        }
        return;
    }
    if node.kind() == "function_item" {
        if is_rust_test(node, source)
            && let (Some(name), Some(body)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("body"),
            )
        {
            let index = case(cases, scope, text(name, source).to_owned());
            rust_walk(body, source, scope, Some(index), cases);
        }
        return;
    }
    if let Some(index) = active {
        let called = if node.kind() == "macro_invocation" {
            node.child_by_field_name("macro")
        } else if node.kind() == "call_expression" {
            node.child_by_field_name("function")
        } else {
            None
        };
        if called.is_some_and(|n| {
            text(n, source)
                .rsplit("::")
                .next()
                .is_some_and(|s| s.starts_with("assert"))
        }) {
            cases[index].assertions += 1;
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        rust_walk(child, source, scope, active, cases);
    }
}

fn callee(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, source).to_owned()),
        "member_expression" => Some(format!(
            "{}.{}",
            callee(node.child_by_field_name("object")?, source)?,
            text(node.child_by_field_name("property")?, source)
        )),
        "call_expression" => callee(node.child_by_field_name("function")?, source),
        _ => None,
    }
}

/// Decode literal string values so changing quote style does not rename the test.
fn title(node: Node<'_>, source: &str) -> Option<String> {
    if !matches!(node.kind(), "string" | "template_string") {
        return None;
    }
    let raw = text(node, source);
    let mut chars = raw[1..raw.len().checked_sub(1)?].chars().peekable();
    let mut out = Vec::<u16>::new();
    while let Some(mut c) = chars.next() {
        if c == '\\' {
            c = chars.next()?;
            match c {
                '\n' => continue,
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    continue;
                }
                'n' => c = '\n',
                'r' => c = '\r',
                't' => c = '\t',
                'b' => c = '\u{8}',
                'f' => c = '\u{c}',
                'v' => c = '\u{b}',
                '0' => c = '\0',
                'x' | 'u' => {
                    let braced = c == 'u' && chars.peek() == Some(&'{');
                    let digits: String = if braced {
                        chars.next();
                        chars.by_ref().take_while(|c| *c != '}').collect()
                    } else {
                        chars.by_ref().take(if c == 'x' { 2 } else { 4 }).collect()
                    };
                    let value = u32::from_str_radix(&digits, 16).ok()?;
                    if !braced {
                        out.push(u16::try_from(value).ok()?);
                        continue;
                    }
                    c = char::from_u32(value)?;
                }
                _ => {}
            }
        }
        out.extend(c.encode_utf16(&mut [0; 2]).iter().copied());
    }
    String::from_utf16(&out).ok()
}

fn js_walk(
    node: Node<'_>,
    source: &str,
    scope: &str,
    active: Option<usize>,
    cases: &mut Vec<TestCase>,
) {
    if node.kind() == "call_expression"
        && let Some(function) = node.child_by_field_name("function")
    {
        let path = callee(function, source);
        if let (Some(path), Some(args)) = (path.as_deref(), node.child_by_field_name("arguments")) {
            let root = path.split('.').next().unwrap_or_default();
            if matches!(root, "test" | "it" | "describe" | "suite") {
                let mut cursor = args.walk();
                let args: Vec<_> = args.named_children(&mut cursor).collect();
                if let Some(label) = args.first().and_then(|n| title(*n, source)) {
                    let suite = matches!(root, "describe" | "suite")
                        || path.split('.').any(|s| s == "describe");
                    let next_scope = if suite {
                        format!("{scope}{label} > ")
                    } else {
                        scope.to_owned()
                    };
                    let active = if suite {
                        active
                    } else {
                        Some(case(cases, scope, label))
                    };
                    for arg in args.iter().skip(1).filter(|a| {
                        matches!(
                            a.kind(),
                            "arrow_function" | "function_expression" | "generator_function"
                        )
                    }) {
                        js_walk(*arg, source, &next_scope, active, cases);
                    }
                    return;
                }
            }
        }
        if let Some(index) = active {
            let assertion = match function.kind() {
                "identifier" => {
                    text(function, source) == "expect"
                        || text(function, source).starts_with("assert")
                }
                "member_expression" => function.child_by_field_name("object").is_some_and(|obj| {
                    obj.kind() == "identifier" && matches!(text(obj, source), "expect" | "assert")
                }),
                _ => false,
            };
            if assertion {
                cases[index].assertions += 1;
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        js_walk(child, source, scope, active, cases);
    }
}

pub(super) fn parse(source: &str) -> Vec<TestCase> {
    parse_as(source, None)
}

pub(super) fn parse_path(path: &str, source: &str) -> Vec<TestCase> {
    parse_as(source, Some(path.ends_with(".rs")))
}

fn parse_as(source: &str, rust: Option<bool>) -> Vec<TestCase> {
    let mut parser = Parser::new();
    let mut cases = Vec::new();
    if rust != Some(false) {
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("bundled Rust grammar ABI matches");
        let tree = parser
            .parse(source, None)
            .expect("parser has no cancellation or timeout");
        rust_walk(tree.root_node(), source, "", None, &mut cases);
        if rust == Some(true) || !cases.is_empty() {
            return cases;
        }
    }
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .expect("bundled TSX grammar ABI matches");
    let tree = parser
        .parse(source, None)
        .expect("parser has no cancellation or timeout");
    js_walk(tree.root_node(), source, "", None, &mut cases);
    cases
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tamper::check_tests_kept;

    #[test]
    fn formatting_and_quote_changes_keep_executed_tests() {
        for (base, head) in [
            (
                r#"test("a", () => { expect(1).toBe(1); });"#,
                r#"test( "a", () => { expect (1).toBe(1); });"#,
            ),
            (
                r#"test("can't fail", () => { expect(1).toBe(1); });"#,
                r#"test('can\'t fail', () => { expect(1).toBe(1); });"#,
            ),
            (
                "#[test]\nfn a() { assert!(true); }",
                "#[test]\npub(crate) fn a() { assert!(true); }",
            ),
            (
                "#[tokio::test(flavor = \"multi_thread\")]\nasync fn a() { assert!(true); }",
                "#[tokio::test(\n flavor = \"multi_thread\"\n)]\nasync fn a() { assert!(true); }",
            ),
        ] {
            assert_eq!(parse(base).len(), 1, "{base}");
            assert_eq!(parse(base), parse(head), "{head}");
            check_tests_kept(base, head, &[]).unwrap();
        }
    }

    #[test]
    fn jsx_text_does_not_hide_executable_assertions() {
        let before = "test('a', () => {\n render(<div>Do panic</div>);\n expect(1).toBe(1);\n});";
        let after = before.replace("Do panic", "Don't panic");
        assert_eq!(parse(before), parse(&after));
        assert_eq!(parse(&after)[0].assertions, 1);
        assert!(check_tests_kept(&after, &after.replace("expect(1).toBe(1);", ""), &[]).is_err());
    }

    #[test]
    fn parameterized_tests_and_expect_methods_require_removal_approval() {
        for source in [
            "it.each([1, 2])('a', (n) => { expect.soft(n).toBe(n); });",
            "test.each([1, 2])('a', async (n) => { await expect.poll(() => n).toBe(n); });",
        ] {
            assert_eq!(parse(source).len(), 1);
            assert_eq!(parse(source)[0].assertions, 1);
            assert!(check_tests_kept(source, "", &[]).is_err());
        }
    }
}

#[cfg(test)]
mod language_selection_tests {
    #[test]
    fn typescript_fixture_text_cannot_switch_the_production_guard_to_rust() {
        let source = "const rust = `#[test] fn fixture() { assert!(true); }`; test('real', () => { expect(true).toBe(true); });";
        let cases = crate::git::tamper::test_cases_for_path("sample.test.ts", source);
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].name, "real");
        assert!(
            crate::git::tamper::check_tests_kept_for_path(
                "sample.test.ts",
                source,
                "const rust = `#[test] fn fixture() { assert!(true); }`;",
                &[]
            )
            .is_err()
        );
    }
}

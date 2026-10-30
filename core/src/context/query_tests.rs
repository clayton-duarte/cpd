//! Red tests of the query sanitiser (docs/research/context-holographic.md, `## Search contract`).

use super::query::sanitise;

fn check(cases: &[(&str, &str)]) {
    for (typed, sent) in cases {
        assert_eq!(sanitise(typed), *sent, "typed query: {typed}");
    }
}

#[test]
fn quote_bare_record_id() {
    check(&[("hs-d655", "\"hs-d655\""), ("pro-n210", "\"pro-n210\""), ("pro-d204", "\"pro-d204\"")]);
}

#[test]
fn quote_dotted_token() {
    check(&[("db.migrations", "\"db.migrations\""), ("v1.2.3", "\"v1.2.3\"")]);
}

#[test]
fn quote_slashed_token() {
    check(&[("plan/key", "\"plan/key\""), ("a/b/c.rs", "\"a/b/c.rs\"")]);
}

#[test]
fn quote_apostrophe_token() {
    check(&[("don't", "\"don't\""), ("it's", "\"it's\"")]);
}

#[test]
fn quote_colon_token() {
    check(&[("a:b", "\"a:b\""), ("path/to/file.rs:12", "\"path/to/file.rs:12\"")]);
}

#[test]
fn quote_id_among_plain_words() {
    check(&[
        ("retrieval of hs-d655 today", "retrieval of \"hs-d655\" today"),
        ("first-message digests", "\"first-message\" digests"),
    ]);
}

#[test]
fn quote_prefix_star_stays_outside_the_quotes() {
    check(&[("pro-d217*", "\"pro-d217\"*"), ("retriev*", "retriev*"), ("*", "\"*\"")]);
}

#[test]
fn quote_leaves_plain_words_unchanged() {
    for q in ["digest", "two words", "snake_case", "café", "and or not near"] {
        assert_eq!(sanitise(q), q, "typed query: {q}");
    }
}

#[test]
fn quote_leaves_phrases_unchanged() {
    for q in ["\"first message\"", "\"a \"\"b\"\" c\"", "\"hs-d655\"", "\"hs-d655\"*"] {
        assert_eq!(sanitise(q), q, "typed query: {q}");
    }
}

#[test]
fn quote_keeps_the_boolean_operators() {
    check(&[
        ("a OR hs-d655", "a OR \"hs-d655\""),
        ("a AND b-c", "a AND \"b-c\""),
        ("a NOT b-c", "a NOT \"b-c\""),
        ("\"first message\" OR hs-d655", "\"first message\" OR \"hs-d655\""),
    ]);
}

#[test]
fn quote_keeps_groups_with_quoted_terms() {
    check(&[
        ("(hs-d655 OR pro-n210) NOT draft", "(\"hs-d655\" OR \"pro-n210\") NOT draft"),
        ("((a OR b) AND c-d)", "((a OR b) AND \"c-d\")"),
        ("( hs-d655 )", "( \"hs-d655\" )"),
    ]);
}

#[test]
fn quote_keeps_near_with_its_distance() {
    check(&[
        ("NEAR(plan/key hs-d655, 3)", "NEAR(\"plan/key\" \"hs-d655\", 3)"),
        ("NEAR(a b)", "NEAR(a b)"),
    ]);
}

#[test]
fn quote_a_paren_inside_a_word_is_text() {
    check(&[("f(x)", "\"f(x)\""), ("search()", "\"search()\""), ("a)", "\"a)\""), ("(f(x))", "(\"f(x)\")")]);
}

#[test]
fn quote_a_comma_outside_near_is_text() {
    check(&[("foo, bar", "\"foo,\" bar"), ("hs-d655,", "\"hs-d655,\"")]);
}

#[test]
fn quote_lone_punctuation_is_text() {
    check(&[("-", "\"-\""), ("+", "\"+\""), ("'", "\"'\""), ("^foo", "\"^foo\""), ("-foo", "\"-foo\"")]);
}

#[test]
fn quote_every_ascii_punctuation_between_letters() {
    for c in (0u8..128).map(char::from).filter(|c| c.is_ascii_punctuation() && *c != '"' && *c != '_') {
        let typed = format!("a{c}b");
        assert_eq!(sanitise(&typed), format!("\"{typed}\""), "typed query: {typed}");
    }
}

#[test]
fn quote_unterminated_phrase_and_unclosed_group_are_left_for_fts5() {
    for q in ["\"unterminated", "(a", "((x", "5\" pipe"] {
        assert_eq!(sanitise(q), q, "typed query: {q}");
    }
}

#[test]
fn quote_is_idempotent() {
    let typed = [
        "hs-d655", "pro-n210", "pro-d204", "db.migrations", "v1.2.3", "plan/key", "a/b/c.rs", "don't", "it's", "a:b",
        "path/to/file.rs:12", "retrieval of hs-d655 today", "first-message digests", "pro-d217*", "retriev*", "*",
        "digest", "two words", "snake_case", "café", "and or not near", "\"first message\"", "\"a \"\"b\"\" c\"",
        "\"hs-d655\"", "\"hs-d655\"*", "a OR hs-d655", "a AND b-c", "a NOT b-c", "\"first message\" OR hs-d655",
        "(hs-d655 OR pro-n210) NOT draft", "((a OR b) AND c-d)", "( hs-d655 )", "NEAR(plan/key hs-d655, 3)",
        "NEAR(a b)", "f(x)", "search()", "a)", "(f(x))", "foo, bar", "hs-d655,", "-", "+", "'", "^foo", "-foo",
    ];
    for q in typed {
        let once = sanitise(q);
        assert_eq!(sanitise(&once), once, "typed query: {q}");
    }
}

//! `{{name}}` substitution.

use std::collections::HashMap;

/// Replaces every `{{name}}` with its value. Err lists each undefined name once, in order of first appearance.
/// A name is `[A-Za-z_][A-Za-z0-9_.-]*`, with optional spaces inside the braces; anything else stays as written.
/// Values are inserted as they are, never scanned again.
pub fn render(template: &str, vars: &HashMap<String, String>) -> Result<String, Vec<String>> {
    let mut out = String::with_capacity(template.len());
    let mut missing: Vec<String> = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match variable(after) {
            Some((name, len)) => {
                match vars.get(name) {
                    Some(value) => out.push_str(value),
                    None if !missing.iter().any(|m| m == name) => missing.push(name.to_string()),
                    None => {}
                }
                rest = &after[len..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    if missing.is_empty() { Ok(out) } else { Err(missing) }
}

/// Parses `  name  }}` at the start of `s` (just after `{{`): the name and the bytes consumed through `}}`.
fn variable(s: &str) -> Option<(&str, usize)> {
    let trimmed = s.trim_start_matches(' ');
    let lead = s.len() - trimmed.len();
    let first = trimmed.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let name_len = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
        .unwrap_or(trimmed.len());
    let after_name = &trimmed[name_len..];
    let tail = after_name.trim_start_matches(' ');
    tail.starts_with("}}")
        .then(|| (&trimmed[..name_len], lead + name_len + (after_name.len() - tail.len()) + 2))
}

#[cfg(test)]
mod tests {
    use super::render;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn replaces_variables_with_optional_spaces() {
        let v = vars(&[("a", "1"), ("b", "2"), ("c.d-e_f", "3")]);
        assert_eq!(render("{{a}}-{{ b }}-{{c.d-e_f}}", &v), Ok("1-2-3".to_string()));
    }

    #[test]
    fn reports_each_missing_name_once_in_order() {
        assert_eq!(render("{{x}} {{y}} {{x}}", &vars(&[])), Err(vec!["x".to_string(), "y".to_string()]));
    }

    #[test]
    fn leaves_text_that_is_not_a_variable() {
        let t = "{{}} {{a b}} {{1a}} {{ a {{ }} {";
        assert_eq!(render(t, &vars(&[])), Ok(t.to_string()));
    }

    #[test]
    fn values_are_not_expanded_again() {
        assert_eq!(render("{{a}}", &vars(&[("a", "{{b}}")])), Ok("{{b}}".to_string()));
    }

    #[test]
    fn handles_multibyte_text_and_adjacent_variables() {
        assert_eq!(render("é{{a}}{{a}}ç", &vars(&[("a", "ü")])), Ok("éüüç".to_string()));
    }
}

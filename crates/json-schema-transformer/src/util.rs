pub fn to_pascal_case(s: &str) -> String {
    let result: String = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect();

    if result.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        format!("Event{result}")
    } else {
        result
    }
}

pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            result.push('_');
            result.push(c.to_ascii_lowercase());
        } else if !c.is_ascii_alphanumeric() && c != '_' {
            result.push('_');
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

pub fn sanitise_identifier(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn escape_string(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            '\r' => vec!['\\', 'r'],
            '\t' => vec!['\\', 't'],
            '\0' => vec!['\\', '0'],
            other => vec![other],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_basic() {
        assert_eq!(to_pascal_case("something-happened"), "SomethingHappened");
        assert_eq!(to_pascal_case("foo_bar"), "FooBar");
        assert_eq!(to_pascal_case("foo.bar"), "FooBar");
    }

    #[test]
    fn pascal_case_digit_prefix() {
        assert_eq!(to_pascal_case("123-event"), "Event123Event");
    }
}

// Minimal shell-like tokenizer for user input and config strings.
// Supports single/double quotes and backslash escaping.

pub fn parse_shell_tokens(input: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut mode: Option<char> = None;
    let mut it = input.chars().peekable();

    while let Some(ch) = it.next() {
        match mode {
            None => match ch {
                '\'' | '"' => mode = Some(ch),
                '\\' => {
                    if let Some(next) = it.next() {
                        cur.push(next);
                    } else {
                        cur.push('\\');
                    }
                }
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(ch),
            },
            Some('\'') => match ch {
                '\'' => mode = None,
                '\\' => {
                    if let Some(next) = it.next() {
                        cur.push(next);
                    } else {
                        cur.push('\\');
                    }
                }
                _ => cur.push(ch),
            },
            Some('"') => match ch {
                '"' => mode = None,
                '\\' => {
                    if let Some(next) = it.next() {
                        cur.push(next);
                    } else {
                        cur.push('\\');
                    }
                }
                _ => cur.push(ch),
            },
            _ => {}
        }
    }

    if mode.is_some() {
        return Err("unterminated quote".to_string());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

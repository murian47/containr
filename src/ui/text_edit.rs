pub(in crate::ui) fn clamp_cursor_to_text(text: &str, cursor: usize) -> usize {
    cursor.min(text.chars().count())
}

pub(in crate::ui) fn insert_char_at_cursor(text: &mut String, cursor: &mut usize, ch: char) {
    let mut out = String::new();
    let target = clamp_cursor_to_text(text, *cursor);
    let mut inserted = false;
    for (idx, c) in text.chars().enumerate() {
        if idx == target {
            out.push(ch);
            inserted = true;
        }
        out.push(c);
    }
    if !inserted {
        out.push(ch);
    }
    *text = out;
    *cursor = target.saturating_add(1);
}

pub(in crate::ui) fn backspace_at_cursor(text: &mut String, cursor: &mut usize) {
    let target = clamp_cursor_to_text(text, *cursor);
    if target == 0 {
        return;
    }
    let del = target - 1;
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i != del {
            out.push(c);
        }
    }
    *text = out;
    *cursor = del;
}

pub(in crate::ui) fn delete_at_cursor(text: &mut String, cursor: &mut usize) {
    let target = clamp_cursor_to_text(text, *cursor);
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i != target {
            out.push(c);
        }
    }
    *text = out;
    *cursor = target.min(text.chars().count());
}

pub(in crate::ui) fn set_text_and_cursor(text: &mut String, cursor: &mut usize, new_text: String) {
    *text = new_text;
    *cursor = text.chars().count();
}

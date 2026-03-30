const MAX_SLUG_LENGTH: usize = 120;

pub fn slugify(input: &str, fallback: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;

    for ch in input.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
            previous_was_separator = false;
            continue;
        }

        if !slug.is_empty() && !previous_was_separator {
            slug.push('-');
            previous_was_separator = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    let base = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    truncate_slug(base, MAX_SLUG_LENGTH)
}

pub fn append_slug_suffix(base: &str, suffix: u32) -> String {
    if suffix <= 1 {
        return truncate_slug(base, MAX_SLUG_LENGTH);
    }

    let suffix_text = format!("-{}", suffix);
    let allowed_base_length = MAX_SLUG_LENGTH.saturating_sub(suffix_text.chars().count());
    let truncated_base = truncate_slug(base, allowed_base_length);

    format!("{}{}", truncated_base, suffix_text)
}

pub fn normalize_slug_lookup(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('/');

    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.to_lowercase())
}

fn truncate_slug(value: &str, max_chars: usize) -> String {
    let truncated = value.chars().take(max_chars).collect::<String>();
    let trimmed = truncated.trim_matches('-');

    if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_collapses_separators() {
        assert_eq!(slugify("  Ribeye Steak !!!  ", "product"), "ribeye-steak");
    }

    #[test]
    fn slugify_falls_back_when_name_has_no_slug_characters() {
        assert_eq!(slugify("   ---   ", "category"), "category");
    }

    #[test]
    fn append_slug_suffix_preserves_suffix_room() {
        assert_eq!(append_slug_suffix("ribeye-steak", 2), "ribeye-steak-2");
    }

    #[test]
    fn normalize_slug_lookup_trims_and_lowercases() {
        assert_eq!(
            normalize_slug_lookup(" /Ribeye-Steak/ "),
            Some("ribeye-steak".to_string())
        );
    }
}

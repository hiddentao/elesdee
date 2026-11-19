pub(crate) fn slugify(input: &str, fallback: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;

    for character in input.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else if !slug.is_empty() && !separator {
            slug.push('-');
            separator = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        return fallback.to_owned();
    }

    let clipped = slug.chars().take(48).collect::<String>();
    clipped.trim_end_matches('-').to_owned()
}

pub(crate) fn with_suffix(base: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(base) {
        return base.to_owned();
    }

    let mut counter = 2_u32;
    loop {
        let candidate = format!("{base}-{counter}");
        if !exists(&candidate) {
            return candidate;
        }
        counter = counter.saturating_add(1);
    }
}

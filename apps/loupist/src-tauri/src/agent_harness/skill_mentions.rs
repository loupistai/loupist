const MAX_SKILL_NAMES: usize = 64;
const MAX_SKILL_NAME_BYTES: usize = 64;

#[derive(Clone, Copy)]
enum Span {
    Quote(char),
    Code(usize),
    Brackets(usize),
    Destination(usize),
    Angle,
}

/// Extract explicit `$name` / `$skill:name` requests from the current user
/// prompt, never from history or rendered turn envelopes. This deliberately
/// skips ambiguous Markdown examples; selection is not inferred from prose.
/// The caller must still intersect these names with the enabled Skill registry.
pub fn explicit_skill_names(prompt: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut fence = None;
    let mut span = None;
    let mut link_tail = false;

    for line in prompt.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, width)) = fence {
            if let Some((candidate, count)) = fence_marker(trimmed) {
                if candidate == marker
                    && count >= width
                    && trimmed.chars().skip(count).all(char::is_whitespace)
                {
                    fence = None;
                }
            }
            continue;
        }
        if trimmed.starts_with('>') || line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        if let Some(delimiter) = opening_fence_marker(trimmed) {
            fence = Some(delimiter);
            continue;
        }

        let chars = line.chars().collect::<Vec<_>>();
        let mut index = 0;
        while index < chars.len() {
            let current = chars[index];
            if let Some(Span::Code(width)) = span {
                if current == '`' {
                    let count = repeated(&chars, index, '`');
                    if count == width {
                        span = None;
                    }
                    index += count;
                } else {
                    index += 1;
                }
                continue;
            }
            if current == '\\' {
                index += 2;
                continue;
            }
            if let Some(active) = span {
                span = match active {
                    Span::Quote(close)
                        if current == close
                            && !(matches!(close, '\'' | '’')
                                && index > 0
                                && chars[index - 1].is_alphanumeric()
                                && chars
                                    .get(index + 1)
                                    .is_some_and(|value| value.is_alphanumeric())) =>
                    {
                        None
                    }
                    Span::Brackets(depth) if current == '[' => Some(Span::Brackets(depth + 1)),
                    Span::Brackets(1) if current == ']' => {
                        link_tail = true;
                        None
                    }
                    Span::Brackets(depth) if current == ']' => Some(Span::Brackets(depth - 1)),
                    Span::Destination(depth) if current == '(' => {
                        Some(Span::Destination(depth + 1))
                    }
                    Span::Destination(1) if current == ')' => None,
                    Span::Destination(depth) if current == ')' => {
                        Some(Span::Destination(depth - 1))
                    }
                    Span::Angle if current == '>' => None,
                    _ => Some(active),
                };
                index += 1;
                continue;
            }
            if link_tail {
                if current.is_whitespace() {
                    index += 1;
                    continue;
                }
                link_tail = false;
                match current {
                    '(' => {
                        span = Some(Span::Destination(1));
                        index += 1;
                        continue;
                    }
                    ':' => break, // A Markdown reference-link definition.
                    _ => {}
                }
            }
            match current {
                '`' => {
                    let width = repeated(&chars, index, '`');
                    span = Some(Span::Code(width));
                    index += width;
                    continue;
                }
                '[' => span = Some(Span::Brackets(1)),
                '<' => span = Some(Span::Angle),
                '"' => span = Some(Span::Quote('"')),
                '“' => span = Some(Span::Quote('”')),
                '\'' | '‘' if index == 0 || !chars[index - 1].is_alphanumeric() => {
                    span = Some(Span::Quote(if current == '‘' { '’' } else { '\'' }));
                }
                '$' if index == 0 || chars[index - 1].is_whitespace() => {
                    if let Some(name) = mention_name(&chars[index + 1..]) {
                        if !names.contains(&name) {
                            names.push(name);
                            if names.len() == MAX_SKILL_NAMES {
                                return names;
                            }
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }
    names
}

fn repeated(chars: &[char], start: usize, marker: char) -> usize {
    chars[start..]
        .iter()
        .take_while(|value| **value == marker)
        .count()
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let width = line.chars().take_while(|value| *value == marker).count();
    (width >= 3).then_some((marker, width))
}

fn opening_fence_marker(mut line: &str) -> Option<(char, usize)> {
    loop {
        if let Some(marker) = fence_marker(line) {
            return Some(marker);
        }
        let marker_end = if line.starts_with(['-', '+', '*']) {
            1
        } else {
            let digits = line.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 || !matches!(line.as_bytes().get(digits), Some(b'.' | b')')) {
                return None;
            }
            digits + 1
        };
        let rest = &line[marker_end..];
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        line = rest.trim_start();
    }
}

fn mention_name(chars: &[char]) -> Option<String> {
    let chars = chars
        .strip_prefix(&['s', 'k', 'i', 'l', 'l', ':'])
        .unwrap_or(chars);
    let length = chars
        .iter()
        .take_while(|value| value.is_ascii_alphanumeric() || **value == '-')
        .count();
    let name = &chars[..length];
    if length == 0
        || length > MAX_SKILL_NAME_BYTES
        || !name.first()?.is_ascii_alphanumeric()
        || !name.last()?.is_ascii_alphanumeric()
        || name.iter().any(|value| value.is_ascii_uppercase())
    {
        return None;
    }
    if let Some(next) = chars.get(length) {
        // Do not accept a prefix of an identifier, path, URL, or edited mention.
        if next.is_alphanumeric() || matches!(next, '_' | '/' | '\\' | '$' | '@') {
            return None;
        }
        if matches!(next, '.' | ':' | '?' | '#')
            && chars
                .get(length + 1)
                .is_some_and(|value| !value.is_whitespace())
        {
            return None;
        }
    }
    Some(name.iter().collect())
}

#[cfg(test)]
mod tests {
    use super::explicit_skill_names;

    #[test]
    fn selects_only_plain_explicit_requests_in_order() {
        let cases: &[(&str, &[&str])] = &[
            (
                "$research compare this with $skill:house-style, then $world. $research again",
                &["research", "house-style", "world"],
            ),
            ("다음은 $research 로 확인해줘\n- $visualize 적용", &["research", "visualize"]),
            ("Explain '$world'; then use $research.", &["research"]),
            ("$world-extra", &["world-extra"]),
            // Quotes, escapes, code, block quotes, indented code and links are examples.
            (r#"Explain "$world" and ‘$lens’ as text."#, &[]),
            (r"Escaped \$world and \\$lens stay text.", &[]),
            ("\"quoted first line\n$world\nlast line\"", &[]),
            (
                "```text\n$world\n````\n~~~\n$lens\n~~~\n   > $research\n\t$journal\n    $comparison\nUse `$world` and ``code `$lens` text`` as examples.\n$visualize",
                &["visualize"],
            ),
            ("```\n$world\n", &[]),
            (
                "[$world](https://example.test/$lens) ![ $journal ](image.png)\n[ $research ][ref]\n[ref]: https://example.test/ '$valuation'\n<https://example.test/$comparison> https://example.test/$world\n[$lens] and $visualize",
                &["visualize"],
            ),
            // Partial or invalid names never guess a known prefix.
            ("$WORLD $world_extra $world/file $world.example $world:other", &[]),
            ("$world한국 prefix$world $skill: $$world", &[]),
        ];
        for (prompt, expected) in cases {
            assert_eq!(explicit_skill_names(prompt), *expected, "{prompt}");
        }
    }

    #[test]
    fn bounds_name_length_and_unique_selection_count() {
        let longest = "a".repeat(64);
        assert_eq!(
            explicit_skill_names(&format!("${longest}")),
            std::slice::from_ref(&longest)
        );
        assert!(explicit_skill_names(&format!("${longest}a")).is_empty());
        let prompt = (0..100)
            .map(|index| format!("$skill-{index} "))
            .collect::<String>();
        let names = explicit_skill_names(&prompt);
        assert_eq!(names.len(), 64);
        assert_eq!(names.last().unwrap(), "skill-63");
    }
}

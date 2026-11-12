//! Page-number patterns used for output file names.

use std::fmt;

/// The default pattern: four digits, starting at zero.
pub const DEFAULT_PATTERN: &str = "{page:04}";

/// A validated page-number pattern.
///
/// A pattern is a literal prefix, one page-number placeholder, and a literal
/// suffix. Examples include `{page:04}`, `slide-{:03}`, `%04d`, and the
/// shorthand `0000`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageFormat {
    prefix: String,
    suffix: String,
    min_width: usize,
}

impl PageFormat {
    /// Parses a page-number pattern.
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            return Err("page format must not be empty".to_owned());
        }
        if input.chars().all(|character| character == '0') {
            return Self::from_parts(String::new(), String::new(), input.len());
        }
        if input.bytes().all(|byte| byte.is_ascii_digit())
            && let Ok(width) = input.parse::<usize>()
            && (1..=20).contains(&width)
        {
            return Self::from_parts(String::new(), String::new(), width);
        }

        let mut prefix = String::new();
        let mut suffix = String::new();
        let mut current = &mut prefix;
        let mut min_width = None;
        let mut index = 0;
        let bytes = input.as_bytes();

        while index < bytes.len() {
            if bytes[index..].starts_with(b"{{") {
                current.push('{');
                index += 2;
                continue;
            }
            if bytes[index..].starts_with(b"}}") {
                current.push('}');
                index += 2;
                continue;
            }
            if bytes[index..].starts_with(b"%%") {
                current.push('%');
                index += 2;
                continue;
            }
            if bytes[index] == b'{' {
                let Some(relative_end) = input[index + 1..].find('}') else {
                    return Err("page format contains an unmatched '{'".to_owned());
                };
                let end = index + 1 + relative_end;
                if min_width.is_some() {
                    return Err("page format must contain exactly one page placeholder".to_owned());
                }
                min_width = Some(parse_braced_placeholder(&input[index + 1..end])?);
                current = &mut suffix;
                index = end + 1;
                continue;
            }
            if bytes[index] == b'}' {
                return Err("page format contains an unmatched '}'".to_owned());
            }
            if bytes[index] == b'%' {
                if min_width.is_some() {
                    return Err("page format must contain exactly one page placeholder".to_owned());
                }
                let (width, consumed) = parse_percent_placeholder(&input[index..])?;
                min_width = Some(width);
                current = &mut suffix;
                index += consumed;
                continue;
            }
            let character = input[index..]
                .chars()
                .next()
                .ok_or_else(|| "page format is not valid UTF-8".to_owned())?;
            current.push(character);
            index += character.len_utf8();
        }

        let Some(min_width) = min_width else {
            return Err(
                "page format must contain one placeholder such as {page:04}, %04d, or 0000"
                    .to_owned(),
            );
        };
        Self::from_parts(prefix, suffix, min_width)
    }

    fn from_parts(prefix: String, suffix: String, min_width: usize) -> Result<Self, String> {
        if !(1..=20).contains(&min_width) {
            return Err("page number width must be between 1 and 20".to_owned());
        }
        validate_literal("prefix", &prefix)?;
        validate_literal("suffix", &suffix)?;
        let format = Self {
            prefix,
            suffix,
            min_width,
        };
        let stem = format.format(0);
        if stem.is_empty() || stem == "." || stem == ".." {
            return Err("page format produces an invalid file name".to_owned());
        }
        Ok(format)
    }

    /// Formats one absolute page number.
    pub fn format(&self, page_number: u64) -> String {
        format!(
            "{}{:0>width$}{}",
            self.prefix,
            page_number,
            self.suffix,
            width = self.min_width
        )
    }
}

impl Default for PageFormat {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            suffix: String::new(),
            min_width: 4,
        }
    }
}

impl fmt::Display for PageFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}{{page:0{}}}{}",
            escape_literal(&self.prefix),
            self.min_width,
            escape_literal(&self.suffix)
        )
    }
}

fn parse_braced_placeholder(body: &str) -> Result<usize, String> {
    let width_spec = if body == "page" {
        None
    } else if let Some(spec) = body.strip_prefix("page:") {
        Some(spec)
    } else if let Some(spec) = body.strip_prefix(':') {
        Some(spec)
    } else {
        return Err(format!(
            "unsupported page placeholder '{{{body}}}'; use {{page:04}}, {{:04}}, %04d, or 0000"
        ));
    };

    match width_spec {
        None => Ok(1),
        Some(spec) => parse_width(spec).ok_or_else(|| {
            format!("invalid placeholder width in '{{{body}}}'; use a zero-padded width such as 04")
        }),
    }
}

fn parse_percent_placeholder(input: &str) -> Result<(usize, usize), String> {
    let Some(rest) = input.strip_prefix('%') else {
        return Err("internal page format error".to_owned());
    };
    if rest.strip_prefix('d').is_some() {
        return Ok((1, 2));
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || !rest[digits..].starts_with('d') {
        return Err("unsupported percent placeholder; use %d or %04d".to_owned());
    }
    let width_spec = &rest[..digits];
    let width = parse_width(width_spec).ok_or_else(|| {
        "invalid percent placeholder; use %d or a zero-padded width such as %04d".to_owned()
    })?;
    Ok((width, digits + 2))
}

fn parse_width(spec: &str) -> Option<usize> {
    if spec.is_empty() || !spec.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !spec.starts_with('0') {
        return None;
    }
    let width = spec.parse::<usize>().ok()?;
    (1..=20).contains(&width).then_some(width)
}

fn validate_literal(label: &str, value: &str) -> Result<(), String> {
    if value.contains(['/', '\\', '\0']) {
        return Err(format!(
            "page format {label} must not contain path separators or NUL bytes"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "page format {label} must not contain control characters"
        ));
    }
    Ok(())
}

fn escape_literal(value: &str) -> String {
    value
        .replace('{', "{{")
        .replace('}', "}}")
        .replace('%', "%%")
}

#[cfg(test)]
mod tests {
    use super::PageFormat;

    #[test]
    fn default_format_starts_at_four_digits() {
        let format = PageFormat::default();
        assert_eq!(format.format(0), "0000");
        assert_eq!(format.format(1), "0001");
        assert_eq!(format.format(42), "0042");
        assert_eq!(format.format(9_999), "9999");
        assert_eq!(format.format(10_000), "10000");
    }

    #[test]
    fn parses_supported_patterns() {
        assert_eq!(PageFormat::parse("0000").unwrap().format(7), "0007");
        assert_eq!(PageFormat::parse("{page:04}").unwrap().format(7), "0007");
        assert_eq!(PageFormat::parse("{:03}").unwrap().format(7), "007");
        assert_eq!(PageFormat::parse("%04d").unwrap().format(7), "0007");
        assert_eq!(PageFormat::parse("4").unwrap().format(7), "0007");
        assert_eq!(PageFormat::parse("04").unwrap().format(7), "0007");
        assert_eq!(PageFormat::parse("%d").unwrap().format(7), "7");
        assert_eq!(
            PageFormat::parse("slide-{page:03}").unwrap().format(7),
            "slide-007"
        );
        assert_eq!(
            PageFormat::parse("slide-%03d-final").unwrap().format(7),
            "slide-007-final"
        );
    }

    #[test]
    fn supports_escaped_literal_characters() {
        assert_eq!(
            PageFormat::parse("slide-{{x}}-{page:02}")
                .unwrap()
                .format(3),
            "slide-{x}-03"
        );
        assert_eq!(PageFormat::parse("%%{page:02}").unwrap().format(3), "%03");
    }

    #[test]
    fn rejects_invalid_patterns() {
        assert!(PageFormat::parse("").is_err());
        assert!(PageFormat::parse("image").is_err());
        assert!(PageFormat::parse("{page} {page}").is_err());
        assert!(PageFormat::parse("../{page:04}").is_err());
        assert!(PageFormat::parse("{page:4}").is_err());
        assert!(PageFormat::parse("%4d").is_err());
        assert!(PageFormat::parse("%x").is_err());
        assert!(PageFormat::parse("{index:04}").is_err());
        assert!(PageFormat::parse("{page:04").is_err());
    }
}

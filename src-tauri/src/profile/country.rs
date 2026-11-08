const REGIONAL_A: u32 = 0x1F1E6;

pub fn infer_country_code(remark: &str) -> String {
    if let Some(code) = flag_emoji_code(remark) {
        return code;
    }
    if let Some(code) = keyword_code(remark) {
        return code;
    }
    "xx".into()
}

fn flag_emoji_code(text: &str) -> Option<String> {
    let mut chars = text.chars().peekable();
    while let Some(first) = chars.next() {
        let first_cp = first as u32;
        if !(REGIONAL_A..=REGIONAL_A + 25).contains(&first_cp) {
            continue;
        }
        let second = chars.next()?;
        let second_cp = second as u32;
        if !(REGIONAL_A..=REGIONAL_A + 25).contains(&second_cp) {
            continue;
        }
        let a = char::from_u32(b'a' as u32 + (first_cp - REGIONAL_A))?;
        let b = char::from_u32(b'a' as u32 + (second_cp - REGIONAL_A))?;
        return Some(format!("{a}{b}"));
    }
    None
}

fn keyword_code(remark: &str) -> Option<String> {
    let compact = normalize(remark);
    let tokens: Vec<&str> = remark
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();

    for (needle, code) in KEYWORDS {
        if needle.contains(' ') {
            let compact_needle: String = needle.chars().filter(|ch| !ch.is_whitespace()).collect();
            if compact.contains(&compact_needle) {
                return Some((*code).into());
            }
        } else if tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case(needle))
        {
            return Some((*code).into());
        }
    }
    None
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

const KEYWORDS: &[(&str, &str)] = &[
    ("hong kong", "hk"),
    ("hongkong", "hk"),
    ("singapore", "sg"),
    ("japan", "jp"),
    ("tokyo", "jp"),
    ("osaka", "jp"),
    ("taiwan", "tw"),
    ("taipei", "tw"),
    ("korea", "kr"),
    ("seoul", "kr"),
    ("united states", "us"),
    ("los angeles", "us"),
    ("new york", "us"),
    ("america", "us"),
    ("germany", "de"),
    ("frankfurt", "de"),
    ("netherlands", "nl"),
    ("amsterdam", "nl"),
    ("united kingdom", "gb"),
    ("britain", "gb"),
    ("london", "gb"),
    ("australia", "au"),
    ("sydney", "au"),
    ("vietnam", "vn"),
    ("malaysia", "my"),
    ("kuala lumpur", "my"),
    ("indonesia", "id"),
    ("jakarta", "id"),
    ("france", "fr"),
    ("paris", "fr"),
    ("canada", "ca"),
    ("india", "in"),
    ("hk", "hk"),
    ("sg", "sg"),
    ("jp", "jp"),
    ("tw", "tw"),
    ("kr", "kr"),
    ("us", "us"),
    ("usa", "us"),
    ("de", "de"),
    ("nl", "nl"),
    ("uk", "gb"),
    ("gb", "gb"),
    ("au", "au"),
    ("vn", "vn"),
    ("my", "my"),
    ("id", "id"),
    ("fr", "fr"),
    ("ca", "ca"),
    ("in", "in"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flag_emoji() {
        assert_eq!(infer_country_code("🇯🇵 Japan Tokyo"), "jp");
        assert_eq!(infer_country_code("Edge 🇸🇬 Premium"), "sg");
    }

    #[test]
    fn reads_country_keywords() {
        assert_eq!(infer_country_code("Hong Kong 01"), "hk");
        assert_eq!(infer_country_code("SG-lite"), "sg");
        assert_eq!(infer_country_code("Frankfurt DE"), "de");
        assert_eq!(infer_country_code("malaysia-1-thinhh"), "my");
        assert_eq!(infer_country_code("indonesia-1-thinhh"), "id");
        assert_eq!(infer_country_code("MY-lite"), "my");
        assert_eq!(infer_country_code("Jakarta ID"), "id");
    }

    #[test]
    fn unknown_falls_back_to_xx() {
        assert_eq!(infer_country_code("Premium Node 12"), "xx");
    }
}

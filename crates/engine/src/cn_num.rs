//! 中文數字（0～99）與阿拉伯數字轉成 u32。

fn digit(c: char) -> Option<u32> {
    Some(match c {
        '零' => 0, '一' => 1, '二' | '兩' | '两' => 2, '三' => 3, '四' => 4,
        '五' => 5, '六' => 6, '七' => 7, '八' => 8, '九' => 9,
        _ => return None,
    })
}

/// 解析 0～99 的阿拉伯數字或中文數字；超出範圍或格式不符回 `None`。
pub fn parse_cn_number(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse::<u32>().ok().filter(|n| *n <= 99);
    }
    let chars: Vec<char> = s.chars().collect();
    match chars.as_slice() {
        ['十'] => Some(10),
        [d] => digit(*d),
        ['十', d] => Some(10 + digit(*d)?),
        [t, '十'] => Some(digit(*t)? * 10).filter(|n| *n >= 20),
        [t, '十', d] => Some(digit(*t)? * 10 + digit(*d)?).filter(|n| *n >= 20),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cn_numbers() {
        for (s, n) in [("十", 10), ("十五", 15), ("二十", 20), ("二十三", 23), ("三", 3), ("兩", 2), ("九十九", 99), ("12", 12)] {
            assert_eq!(parse_cn_number(s), Some(n), "{s}");
        }
        assert_eq!(parse_cn_number("一百零一"), None);
    }
}

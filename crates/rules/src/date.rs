pub(crate) fn is_valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let Some(year) = parse_u32(&bytes[0..4]) else {
        return false;
    };
    let Some(month) = parse_u32(&bytes[5..7]) else {
        return false;
    };
    let Some(day) = parse_u32(&bytes[8..10]) else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }
    (1..=days_in_month(year, month)).contains(&day)
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for byte in bytes {
        let digit = byte.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value * 10 + u32::from(digit);
    }
    Some(value)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: u32) -> bool {
    year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100))
}

#[cfg(test)]
mod tests {
    use super::is_valid_date;

    #[test]
    fn accepts_calendar_dates() {
        assert!(is_valid_date("2024-01-15"));
        assert!(is_valid_date("2024-02-29"));
        assert!(is_valid_date("2000-02-29"));
    }

    #[test]
    fn rejects_impossible_dates() {
        assert!(!is_valid_date("2023-02-29"));
        assert!(!is_valid_date("1900-02-29"));
        assert!(!is_valid_date("2024-13-01"));
        assert!(!is_valid_date("2024-00-10"));
        assert!(!is_valid_date("2024-1-01"));
        assert!(!is_valid_date("20240101"));
    }
}

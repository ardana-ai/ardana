//! How API numbers are shown, in the playground and by `ardana run`: probabilities as percent with one decimal,
//! confidence, noul and score with two decimals. Rounding works on the number's shortest decimal text (the JSON the API sent), half up, so a value
//! displays the way its decimal digits read, never the way its binary approximation happens to fall.

/// `0.97315` as `97.3%`.
pub fn percent(value: f64) -> String {
    format!("{}%", round_decimal(&raw(value), 2, 1))
}

/// `0.8349` as `0.83`.
pub fn fixed2(value: f64) -> String {
    round_decimal(&raw(value), 0, 2)
}

/// A number exactly as JSON writes it: the shortest text that reads back to the same `f64`. This is what
/// `data-value` carries.
pub fn raw(value: f64) -> String {
    serde_json::Number::from_f64(value).map_or_else(|| value.to_string(), |n| n.to_string())
}

/// Rounds the decimal number `text` (`0.9731`, `1.0`, `1e-7`, `1.5e-5`), multiplied by `10^shift`, to `places`
/// decimals, half up (away from zero).
pub fn round_decimal(text: &str, shift: i32, places: usize) -> String {
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (text, 0),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    // value = digits * 10^scale
    let mut digits: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes())
        .map(|b| b.wrapping_sub(b'0'))
        .collect();
    let mut scale = exponent + shift - frac_part.len() as i32;
    let places = places as i32;
    if scale < -places {
        let drop = (-places - scale) as usize;
        let round_up = if drop > digits.len() {
            false
        } else {
            digits[digits.len() - drop] >= 5
        };
        digits.truncate(digits.len().saturating_sub(drop));
        scale = -places;
        if round_up {
            increment(&mut digits);
        }
    }
    while scale > -places {
        digits.push(0);
        scale -= 1;
    }
    // digits * 10^-places
    let places = places as usize;
    while digits.len() <= places {
        digits.insert(0, 0);
    }
    let split = digits.len() - places;
    let mut int_digits = &digits[..split];
    while int_digits.len() > 1 && int_digits[0] == 0 {
        int_digits = &int_digits[1..];
    }
    let int_text: String = int_digits.iter().map(|d| char::from(b'0' + d)).collect();
    let frac_text: String = digits[split..]
        .iter()
        .map(|d| char::from(b'0' + d))
        .collect();
    let zero = digits.iter().all(|&d| d == 0);
    let sign = if negative && !zero { "-" } else { "" };
    if places == 0 {
        format!("{sign}{int_text}")
    } else {
        format!("{sign}{int_text}.{frac_text}")
    }
}

/// Adds one to the decimal digits, carrying.
fn increment(digits: &mut Vec<u8>) {
    for digit in digits.iter_mut().rev() {
        if *digit == 9 {
            *digit = 0;
        } else {
            *digit += 1;
            return;
        }
    }
    digits.insert(0, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_has_one_decimal_half_up() {
        assert_eq!(percent(0.9731), "97.3%");
        assert_eq!(percent(0.1235), "12.4%");
        assert_eq!(percent(0.9996), "100.0%");
        assert_eq!(percent(1.0), "100.0%");
        assert_eq!(percent(0.0), "0.0%");
        assert_eq!(percent(0.0004), "0.0%");
        assert_eq!(percent(0.0005), "0.1%");
        assert_eq!(percent(1e-7), "0.0%");
    }

    #[test]
    fn fixed2_has_two_decimals_half_up() {
        assert_eq!(fixed2(0.8349), "0.83");
        assert_eq!(fixed2(0.125), "0.13");
        assert_eq!(fixed2(2.0), "2.00");
        assert_eq!(fixed2(1.995), "2.00");
        assert_eq!(fixed2(0.005), "0.01");
        assert_eq!(fixed2(1.5e-5), "0.00");
        assert_eq!(fixed2(-0.125), "-0.13");
        assert_eq!(fixed2(-0.001), "0.00");
    }

    #[test]
    fn raw_is_the_shortest_json_text() {
        assert_eq!(raw(0.9731), "0.9731");
        assert_eq!(raw(1.0), "1.0");
        assert_eq!(round_decimal("1e-7", 0, 0), "0");
        assert_eq!(round_decimal("15", 0, 0), "15");
    }
}

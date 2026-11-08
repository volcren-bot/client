//! The keys that type letters, digits and space: the client turns key codes into characters itself, and for these the
//! code is the same on every common layout, while the layout Wine reports can name another key for them (a digit asked
//! for with Shift, or on the keypad). Punctuation is left to the layout, which the client also reads it through.

/// The virtual key code of `c` and whether Shift is held, for a letter, a digit or a space; `None` for anything else.
pub fn us_key(c: char) -> Option<(u8, bool)> {
    match c {
        'a'..='z' => Some((c.to_ascii_uppercase() as u8, false)),
        'A'..='Z' => Some((c as u8, true)),
        '0'..='9' => Some((c as u8, false)),
        ' ' => Some((0x20, false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_space_use_their_own_keys() {
        assert_eq!(us_key('3'), Some((b'3', false)));
        assert_eq!(us_key('0'), Some((b'0', false)));
        assert_eq!(us_key('a'), Some((b'A', false)));
        assert_eq!(us_key('Q'), Some((b'Q', true)));
        assert_eq!(us_key(' '), Some((0x20, false)));
        assert_eq!(us_key('-'), None);
        assert_eq!(us_key('é'), None);
    }
}

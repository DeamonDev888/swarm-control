//! Décodage UTF-8 par morceaux (un caractère peut être coupé entre deux lectures).

/// Extrait la partie décodable de `carry` ; garde une séquence incomplète en
/// fin de tampon pour le prochain morceau.
pub fn take_utf8(carry: &mut Vec<u8>) -> String {
    match std::str::from_utf8(carry) {
        Ok(s) => {
            let out = s.to_owned();
            carry.clear();
            out
        }
        Err(e) if e.error_len().is_none() => {
            let valid = e.valid_up_to();
            let out = String::from_utf8_lossy(&carry[..valid]).into_owned();
            carry.drain(..valid);
            out
        }
        Err(_) => {
            let out = String::from_utf8_lossy(carry).into_owned();
            carry.clear();
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::take_utf8;

    #[test]
    fn carries_a_split_character() {
        let bytes = "é".as_bytes();
        let mut carry = vec![b'a', bytes[0]];
        assert_eq!(take_utf8(&mut carry), "a");
        assert_eq!(carry, vec![bytes[0]]);
        carry.push(bytes[1]);
        assert_eq!(take_utf8(&mut carry), "é");
        assert!(carry.is_empty());
    }
}

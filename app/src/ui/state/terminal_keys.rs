//! xterm's default legacy bindings stay unchanged. Ambiguous keys are encoded only when pi has
//! explicitly enabled xterm modifyOtherKeys (CSI > 4 ; 2 m).

#[derive(Debug, Clone, Copy, Default)]
pub struct KeyEvent<'a> {
    /// DOM-style key: "Enter", "Tab", "p", "P", …
    pub key: &'a str,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
    pub down: bool,
}

pub fn modified_key(e: &KeyEvent, enabled: bool) -> Option<String> {
    if !enabled || !e.down || e.meta {
        return None;
    }
    let special = e.key == "Enter" && (e.shift || e.ctrl || e.alt);
    let letter = e.key.chars().count() == 1 && e.shift && (e.ctrl || e.alt);
    if !special && !letter {
        return None;
    }
    let code = if special { 13 } else { e.key.to_lowercase().chars().next()? as u32 };
    let modifier = 1 + if e.shift { 1 } else { 0 } + if e.alt { 2 } else { 0 } + if e.ctrl { 4 } else { 0 };
    Some(format!("\x1b[27;{modifier};{code}~"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_keys_only_with_negotiated_modify_other_keys() {
        let e = KeyEvent { key: "Enter", shift: true, down: true, ..Default::default() };
        assert_eq!(modified_key(&e, false), None);
        assert_eq!(modified_key(&e, true).as_deref(), Some("\x1b[27;2;13~"));
        assert_eq!(modified_key(&KeyEvent { key: "P", ctrl: true, ..e }, true).as_deref(), Some("\x1b[27;6;112~"));
        assert_eq!(modified_key(&KeyEvent { key: "p", shift: false, ctrl: true, ..e }, true), None);
        assert_eq!(modified_key(&KeyEvent { down: false, ..e }, true), None);
        assert_eq!(modified_key(&KeyEvent { key: "Tab", ..e }, true), None);
    }
}

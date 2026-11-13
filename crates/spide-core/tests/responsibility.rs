use spide_core::*;

fn apply(ops_json: &str) -> Result<Design, OpError> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None))
}

#[test]
fn role_and_concern_are_validated_and_stored() {
    let d = apply(
        r#"[{"op":"add_class","name":"Panel","role":"presentation","methods":[{"name":"refresh","concern":"presentation"}]},
            {"op":"update_class","name":"Panel","role":"controller"},
            {"op":"update_method","class":"Panel","name":"refresh","method":{"name":"refresh","concern":"coordination"}}]"#,
    )
    .unwrap();
    let c = d.class("Panel").unwrap();
    assert_eq!(c.role.as_deref(), Some("controller"));
    assert_eq!(c.methods[0].concern.as_deref(), Some("coordination"));
    assert!(apply(r#"[{"op":"add_class","name":"X","role":"wizard"}]"#).is_err());
    assert!(apply(r#"[{"op":"add_class","name":"X","methods":[{"name":"f","concern":"magic"}]}]"#).is_err());
    let cleared = apply(r#"[{"op":"add_class","name":"X","role":"domain"},{"op":"update_class","name":"X","role":""}]"#).unwrap();
    assert_eq!(cleared.class("X").unwrap().role, None);
}

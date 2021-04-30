use super::*;

#[derive(PartialEq, Debug, Clone)]
pub(super) enum ParsedKeyAction {
    KeyAction(KeyActionWithMods),
    KeyClickAction(KeyClickActionWithMods),
}

pub(super) trait ParsedKeyActionVecExt {
    fn to_key_actions(self) -> Vec<KeyAction>;
}

impl ParsedKeyActionVecExt for Vec<ParsedKeyAction> {
    fn to_key_actions(self) -> Vec<KeyAction> {
        self.into_iter()
            .fold(vec![], |mut acc, v| match v {
                ParsedKeyAction::KeyAction(action) => {
                    if action.modifiers.shift { acc.push(KeyAction::new(*KEY_LEFT_SHIFT, TYPE_DOWN)); }
                    acc.push(KeyAction::new(action.key, action.value));
                    acc
                }
                ParsedKeyAction::KeyClickAction(action) => {
                    if action.modifiers.shift { acc.push(KeyAction::new(*KEY_LEFT_SHIFT, TYPE_DOWN)); }
                    acc.push(KeyAction::new(action.key, TYPE_DOWN));
                    acc.push(KeyAction::new(action.key, TYPE_UP));
                    acc
                }
            })
    }
}

pub(super) fn key_action(input: &str) -> Res<&str, ParsedKeyAction> {
    context(
        "key_action",
        alt((
            map(tuple((tag("{"), key_with_state, tag("}"))), |(_, v, _)| (v.0, Some(v.1))),
            map(
                alt((
                    key,
                    map(tuple((tag("{"), key, tag("}"))), |v| v.1),
                )),
                |v| (v, None),
            ),
        )),
    )(input).and_then(|(next, (parsed_key, state))| {
        let mut mods = KeyModifierFlags::new();
        let key;

        match parsed_key {
            ParsedSingleKey::Key(k) => { key = k; }
            ParsedSingleKey::CapitalKey(k) => {
                mods.shift();
                key = k;
            }
        }

        let action = match state {
            Some(state) => {
                ParsedKeyAction::KeyAction(KeyActionWithMods::new(key, state, mods))
            }
            None => {
                ParsedKeyAction::KeyClickAction(KeyClickActionWithMods::new_with_mods(key, mods))
            }
        };

        Ok((next, action))
    })
}

pub(super) fn key_action_with_flags(input: &str) -> Res<&str, ParsedKeyAction> {
    context(
        "key_action_with_flags",
        tuple((
            key_flags,
            key_action,
        )),
    )(input).and_then(|(next, parts)| {
        let flags = parts.0;
        let mut action = parts.1;

        match &mut action {
            ParsedKeyAction::KeyAction(action) => { action.modifiers.apply_from(&flags) }
            ParsedKeyAction::KeyClickAction(action) => { action.modifiers.apply_from(&flags) }
        }

        Ok((next, action))
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_action() {
        assert_eq!(key_action("{a down}"), Ok(("", ParsedKeyAction::KeyAction(
            KeyActionWithMods::new(Key::from_str(&EventType::EV_KEY, "KEY_A").unwrap(), 1, KeyModifierFlags::new())
        ))));

        assert_eq!(key_action("{btn_forward down}"), Ok(("", ParsedKeyAction::KeyAction(
            KeyActionWithMods::new(Key::from_str(&EventType::EV_KEY, "BTN_FORWARD").unwrap(), 1, KeyModifierFlags::new())
        ))));
    }

    #[test]
    fn test_flags() {
        assert_eq!(key_action_with_flags("+{a down}"), Ok(("", ParsedKeyAction::KeyAction(
            KeyActionWithMods::new(Key::from_str(&EventType::EV_KEY, "KEY_A").unwrap(), 1, KeyModifierFlags::new().tap_mut(|v| v.shift()))
        ))));
    }
}
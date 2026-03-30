use crate::device::NormalizedMouseEvent;
use evdev::KeyCode;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MouseButtonTrigger {
    pub code: KeyCode,
    pub value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyStroke {
    pub key: KeyCode,
    pub value: i32,
}

#[derive(Clone, Debug, Default)]
pub struct CompiledRules {
    remaps: HashMap<MouseButtonTrigger, Vec<KeyStroke>>,
    registered_keys: Vec<KeyCode>,
}

impl CompiledRules {
    /// Builds lookup tables for runtime routing.
    pub fn new(remaps: HashMap<MouseButtonTrigger, Vec<KeyStroke>>) -> Self {
        let mut registered_keys = remaps
            .values()
            .flat_map(|sequence| sequence.iter().map(|stroke| stroke.key))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        registered_keys.sort_unstable_by_key(|key| key.code());

        Self {
            remaps,
            registered_keys,
        }
    }

    /// Returns key capabilities required by the virtual keyboard.
    pub fn registered_keys(&self) -> &[KeyCode] {
        &self.registered_keys
    }
}

pub enum RoutedAction<'a> {
    PassThrough,
    Remap(&'a [KeyStroke]),
    Flush,
    Ignore,
}

/// Resolves one normalized mouse event into either passthrough or remap output.
pub fn route<'a>(event: &NormalizedMouseEvent, rules: &'a CompiledRules) -> RoutedAction<'a> {
    match event {
        NormalizedMouseEvent::Button { code, value } => rules
            .remaps
            .get(&MouseButtonTrigger {
                code: *code,
                value: *value,
            })
            .map_or(RoutedAction::PassThrough, |sequence| {
                RoutedAction::Remap(sequence.as_slice())
            }),
        NormalizedMouseEvent::Relative { .. } => RoutedAction::PassThrough,
        NormalizedMouseEvent::SyncReport => RoutedAction::Flush,
        NormalizedMouseEvent::OtherIgnored => RoutedAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::NormalizedMouseEvent;

    #[test]
    fn button_match_remaps_to_keyboard() {
        let rules = CompiledRules::new(HashMap::from([(
            MouseButtonTrigger {
                code: KeyCode::BTN_RIGHT,
                value: 1,
            },
            vec![KeyStroke {
                key: KeyCode::KEY_LEFTMETA,
                value: 1,
            }],
        )]));

        let action = route(
            &NormalizedMouseEvent::Button {
                code: KeyCode::BTN_RIGHT,
                value: 1,
            },
            &rules,
        );

        match action {
            RoutedAction::Remap(sequence) => {
                assert_eq!(sequence.len(), 1);
                assert_eq!(sequence[0].key, KeyCode::KEY_LEFTMETA);
            }
            _ => panic!("expected remap"),
        }
    }

    #[test]
    fn unmatched_button_passes_through() {
        let rules = CompiledRules::default();
        let action = route(
            &NormalizedMouseEvent::Button {
                code: KeyCode::BTN_LEFT,
                value: 1,
            },
            &rules,
        );
        assert!(matches!(action, RoutedAction::PassThrough));
    }

    #[test]
    fn relative_events_pass_through() {
        let rules = CompiledRules::default();
        let action = route(
            &NormalizedMouseEvent::Relative {
                code: evdev::RelativeAxisCode::REL_X,
                value: 10,
            },
            &rules,
        );
        assert!(matches!(action, RoutedAction::PassThrough));
    }
}

use std::borrow::Borrow;
use std::{collections::HashMap, convert::TryInto};

use common::index::{self, StateTransition};
use common::index::{Check, ConfigFile, State, StateIndex};
use heapless::Vec;
use toml::Spanned;

use crate::{upper, CheckConditionError, Error, StateCountError};
use nova_software_common as common;

struct Temp<'s>(HashMap<&'s str, (StateIndex, &'s Spanned<upper::State>)>);

impl<'s> Temp<'s> {
    fn new(states: &'s [Spanned<upper::State>]) -> Self {
        Self(
            states
                .iter()
                .enumerate()
                .map(|(i, state)| {
                    let i: u8 = i.try_into().unwrap();

                    // SAFETY: `i` comes from enumerate, which only yields indices in range
                    let index = unsafe { StateIndex::new_unchecked(i) };
                    (state.get_ref().name.borrow(), (index, state))
                })
                .collect(),
        )
    }

    fn get_index(&self, name: &str) -> Result<StateIndex, Error> {
        self.0
            .get(name)
            .ok_or_else(|| Error::StateNotFound(name.into()))
            .map(|v| v.0)
    }

    fn get_span(&self, name: &str) -> Result<&'s Spanned<upper::State>, Error> {
        self.0
            .get(name)
            .ok_or_else(|| Error::StateNotFound(name.into()))
            .map(|v| v.1.clone())
    }
}

//When we go to a low level file, the default state must be first
pub fn verify(mid: upper::ConfigFile) -> Result<ConfigFile, Error> {
    if mid.states.get_ref().is_empty() {
        return Err(Error::StateCount(StateCountError::NoStates));
    }
    if mid.states.get_ref().len() > u8::MAX as usize {
        return Err(Error::StateCount(StateCountError::TooManyStates(
            mid.states.get_ref().len(),
        )));
    }

    let temp = Temp::new(mid.states.get_ref().as_slice());

    let mut states: Vec<State, { common::MAX_STATES }> = mid
        .states
        .get_ref()
        .iter()
        // At this point we dont know the indices for the checks or commands so put in filler data
        .map(|_| State::new(Vec::new(), Vec::new(), None))
        .collect();

    let default_state = mid.default_state.map_or_else(
        // SAFETY: We have checked that there is at least one state above, so index 0 is in bounds
        || Ok(unsafe { StateIndex::new_unchecked(0) }),
        |name| temp.get_index(name.borrow()),
    )?;

    for (src_state, dst_state) in mid.states.get_ref().iter().zip(states.iter_mut()) {
        for src_check in &src_state.get_ref().checks {
            let check: index::Check = convert_check(src_check.get_ref(), &temp)?;
            dst_state.checks.push(check).unwrap();
        }

        for src_command in &src_state.get_ref().commands {
            let command: index::Command = src_command.get_ref().try_into().unwrap();
            dst_state.commands.push(command).unwrap();
        }
    }

    Ok(ConfigFile {
        default_state,
        states,
    })
}

fn convert_check(check: &upper::Check, temp: &Temp<'_>) -> Result<Check, Error> {
    if check.upper_bound.is_some() && check.lower_bound.is_none()
        || check.upper_bound.is_none() && check.lower_bound.is_some()
    {
        panic!(
            "Unmatched bound! if one of `lower_bound` or `higher_bound` is used, both must be set"
        );
    }
    let mut count = 0;
    if check.greater_than.is_some() {
        count += 1;
    }
    if check.upper_bound.is_some() && check.lower_bound.is_some() {
        count += 1;
    }
    if check.flag.is_some() {
        count += 1;
    }
    if count == 0 {
        return Err(Error::CheckConditionError(CheckConditionError::NoCondition));
    }
    if count > 1 {
        return Err(Error::CheckConditionError(
            CheckConditionError::TooManyConditions(count),
        ));
    }

    enum CheckKind {
        Apogee,
        Altitude,
        Pyro1Continuity,
        Pyro2Continuity,
        Pyro3Continuity,
    }
    let check_kind = match check.check.borrow() {
        "apogee" => CheckKind::Apogee,
        "altitude" => CheckKind::Altitude,
        "pyro1_continuity" => CheckKind::Pyro1Continuity,
        "pyro2_continuity" => CheckKind::Pyro2Continuity,
        "pyro3_continuity" => CheckKind::Pyro3Continuity,
        other => panic!("Bad check {}", other), // TODO: Better error handling
    };

    pub enum CheckCondition {
        FlagEq(bool),
        // Equals { value: f32 },
        GreaterThan(f32),
        LessThan(f32),
        Between { upper_bound: f32, lower_bound: f32 },
    }

    //The user only set one option, now map that to an object and state
    let condition = {
        if let Some(gt) = &check.greater_than {
            CheckCondition::GreaterThan(*gt.get_ref())
        } else if let (Some(u), Some(l)) = (&check.upper_bound, &check.lower_bound) {
            CheckCondition::Between {
                upper_bound: *u.get_ref(),
                lower_bound: *l.get_ref(),
            }
        } else if let Some(flag) = check.flag.borrow() {
            match flag.borrow() {
                "set" => CheckCondition::FlagEq(true),
                "unset" => CheckCondition::FlagEq(false),
                _ => panic!("Unknown flag: {}", flag.get_ref()), // TODO: Better error handling
            }
        } else {
            unreachable!()
        }
    };

    use common::{CheckData, FloatCondition, NativeFlagCondition, PyroContinuityCondition};
    // Perform type checking on kind and condition
    let data = match check_kind {
        CheckKind::Apogee => match condition {
            CheckCondition::FlagEq(val) => CheckData::ApogeeFlag(NativeFlagCondition(val)),
            _ => panic!(),
        },
        CheckKind::Altitude => match condition {
            CheckCondition::Between {
                upper_bound,
                lower_bound,
            } => CheckData::Altitude(FloatCondition::Between {
                upper_bound,
                lower_bound,
            }),
            CheckCondition::GreaterThan(val) => {
                CheckData::Altitude(FloatCondition::GreaterThan(val))
            }
            CheckCondition::LessThan(val) => CheckData::Altitude(FloatCondition::LessThan(val)),
            _ => panic!(),
        },
        CheckKind::Pyro1Continuity => match condition {
            CheckCondition::FlagEq(val) => CheckData::Pyro1Continuity(PyroContinuityCondition(val)),
            _ => panic!(),
        },
        CheckKind::Pyro2Continuity => match condition {
            CheckCondition::FlagEq(val) => CheckData::Pyro2Continuity(PyroContinuityCondition(val)),
            _ => panic!(),
        },
        CheckKind::Pyro3Continuity => match condition {
            CheckCondition::FlagEq(val) => CheckData::Pyro3Continuity(PyroContinuityCondition(val)),
            _ => panic!(),
        },
    };

    let transition = match &check.transition {
        Some(state) => Some(temp.get_index(state.borrow())?),
        None => None,
    };

    let abort = match &check.abort {
        Some(state) => Some(temp.get_index(state.borrow())?),
        None => None,
    };

    let transition = match (transition, abort) {
        (Some(t), None) => Some(StateTransition::Transition(t)),
        (None, Some(a)) => Some(StateTransition::Abort(a)),
        (None, None) => None,
        (Some(_), Some(_)) => panic!("Cannot abort and transition in the same check!"), // TODO: fix
    };

    Ok(index::Check::new(data, transition))
}

#[cfg(test)]
mod tests {
    use common::{index::StateIndex, CheckData, FloatCondition, PyroContinuityCondition};

    use crate::{upper, upper::cs, CheckConditionError};
    use super::{common, index};

    #[test]
    fn basic1() {
        let upper = upper::ConfigFile {
            default_state: Some(cs("PowerOn".to_owned())),
            states: cs(vec![cs(upper::State {
                name: cs("PowerOn".to_owned()),
                timeout: None,
                checks: vec![cs(upper::Check {
                    name: cs("Takeoff".to_owned()),
                    check: cs("altitude".to_owned()),
                    greater_than: Some(cs(100.0)),
                    transition: None,
                    upper_bound: None,
                    flag: None,
                    lower_bound: None,
                    abort: None,
                })],
                commands: vec![],
            })]),
        };
        use heapless::Vec;

        let expected = index::ConfigFile {
            default_state: unsafe { StateIndex::new_unchecked(0) },
            states: [index::State {
                timeout: None,
                checks: [index::Check::new(
                    CheckData::Altitude(FloatCondition::GreaterThan(100.0)),
                    None,
                )]
                .into_iter()
                .collect(),
                commands: Vec::new(),
            }]
            .into_iter()
            .collect(),
        };

        let real = super::verify(upper).unwrap();
        assert_eq!(expected, real);
    }

    #[test]
    fn basic2() {
        let upper = upper::ConfigFile {
            default_state: None,
            states: cs(vec![
                cs(upper::State {
                    name: cs("Ground".to_owned()),
                    timeout: None,
                    checks: vec![cs(upper::Check {
                        name: cs("Takeoff".to_owned()),
                        check: cs("altitude".to_owned()),
                        greater_than: Some(cs(100.0)),
                        transition: None,
                        upper_bound: None,
                        flag: None,
                        lower_bound: None,
                        abort: None,
                    })],
                    commands: vec![],
                }),
                cs(upper::State {
                    name: cs("Launch".to_owned()),
                    timeout: None,
                    checks: vec![cs(upper::Check {
                        name: cs("Pyro1Cont".to_owned()),
                        check: cs("pyro1_continuity".to_owned()),
                        greater_than: None,
                        transition: None,
                        upper_bound: None,
                        flag: Some(cs("set".to_owned())),
                        lower_bound: None,
                        abort: None,
                    })],
                    commands: vec![],
                }),
            ]),
        };
        use heapless::Vec;

        let expected = index::ConfigFile {
            default_state: unsafe { StateIndex::new_unchecked(0) },
            states: [
                index::State {
                    timeout: None,
                    checks: [index::Check::new(
                        CheckData::Altitude(FloatCondition::GreaterThan(100.0)),
                        None,
                    )]
                    .into_iter()
                    .collect(),
                    commands: Vec::new(),
                },
                index::State {
                    timeout: None,
                    checks: [index::Check::new(
                        CheckData::Pyro1Continuity(PyroContinuityCondition(true)),
                        None,
                    )]
                    .into_iter()
                    .collect(),
                    commands: Vec::new(),
                },
            ]
            .into_iter()
            .collect(),
        };

        let real = super::verify(upper).unwrap();
        assert_eq!(expected, real);
    }

    fn check_error(err: Result<index::ConfigFile, crate::Error>, expected_err: crate::Error) {
        match err {
            Ok(c) => {
                panic!(
                    "Low level verify should have failed with: {:?}, decoded to {:?}",
                    expected_err, c
                );
            }
            Err(e) => {
                assert_eq!(e, expected_err);
            }
        }
    }

    #[test]
    fn error_unknown_state() {
        let bad_name = "I do not exist!".to_owned();
        let upper = upper::ConfigFile {
            default_state: Some(cs(bad_name.clone())),
            states: cs(vec![cs(upper::State {
                name: cs("PowerOn".to_owned()),
                timeout: None,
                checks: vec![cs(upper::Check {
                    name: cs("Takeoff".to_owned()),
                    check: cs("altitude".to_owned()),
                    greater_than: Some(cs(100.0)),
                    transition: None,
                    upper_bound: None,
                    flag: None,
                    lower_bound: None,
                    abort: None,
                })],
                commands: vec![],
            })]),
        };
        check_error(super::verify(upper), crate::Error::StateNotFound(bad_name));
    }

    #[test]
    fn error_mutiple_subchecks() {
        let upper = upper::ConfigFile {
            default_state: None,
            states: cs(vec![cs(upper::State {
                name: cs("PowerOn".to_owned()),
                timeout: None,
                checks: vec![cs(upper::Check {
                    name: cs("Check".to_owned()),
                    check: cs("pyro1_continuity".to_owned()),
                    greater_than: Some(cs(100.0)),
                    transition: None,
                    upper_bound: Some(cs(0.0)),
                    flag: Some(cs("set".to_owned())),
                    lower_bound: Some(cs(0.5)),
                    abort: None,
                })],
                commands: vec![],
            })]),
        };
        check_error(
            super::verify(upper),
            crate::Error::CheckConditionError(CheckConditionError::TooManyConditions(3)),
        );
    }
}

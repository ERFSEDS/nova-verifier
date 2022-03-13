use std::borrow::Borrow;
use std::{collections::HashMap, convert::TryInto};

use common::index::{self, StateTransition};
use common::index::{Check, Command, ConfigFile, State, StateIndex};
use heapless::Vec;
use toml::Spanned;

use crate::{upper, Context, Span};
use nova_software_common as common;

pub(crate) struct Temp<'s>(HashMap<&'s str, StateIndex>);

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
                    (state.get_ref().name.borrow(), index)
                })
                .collect(),
        )
    }

    fn get_index(&self, name: &Spanned<String>, context: &mut Context) -> Result<StateIndex, ()> {
        match self.0.get(name.get_ref().as_str()) {
            Some(v) => Ok(*v),
            None => {
                let span = Span::from_spanned(context, name);
                context
                    .error(format!("state not found `{}`", name.get_ref()))
                    .set_primary_span(span, "not found")
                    .emit();
                Err(())
            }
        }
    }
}

// When we go to a low level file, the default state must be first
pub fn verify(mid: upper::ConfigFile, context: &mut crate::Context) -> Result<ConfigFile, ()> {
    let span = Span::from_spanned(context, &mid.states);
    if mid.states.get_ref().is_empty() {
        context
            .error("states missing")
            .set_primary_span(span, "you need to have at least one state")
            .emit();

        return Err(());
    }
    if mid.states.get_ref().len() > common::MAX_STATES as usize {
        context
            .error("too many states")
            .set_primary_span(
                span,
                format!("the maxinum number of states is {}", common::MAX_STATES),
            )
            .emit();
        return Err(());
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
        |name| temp.get_index(&name, context),
    )?;

    for (src_state, dst_state) in mid.states.get_ref().iter().zip(states.iter_mut()) {
        for src_check in &src_state.get_ref().checks {
            let check: index::Check = convert_check(src_check, &temp, context)?;
            dst_state.checks.push(check).unwrap();
        }

        for src_command in &src_state.get_ref().commands {
            let command: index::Command = convert_command(src_command, context)?;
            dst_state.commands.push(command).unwrap();
        }
    }

    Ok(ConfigFile {
        default_state,
        states,
    })
}

pub(crate) fn convert_command(
    command: &Spanned<upper::Command>,
    context: &mut Context,
) -> Result<Command, ()> {
    let span = Span::from_spanned(context, command);
    let command = command.get_ref();
    println!("Converting {command:?}");

    let mut count = 0;
    if command.pyro1.is_some() {
        count += 1;
    }
    if command.pyro2.is_some() {
        count += 1;
    }
    if command.pyro3.is_some() {
        count += 1;
    }
    if command.data_rate.is_some() {
        count += 1;
    }
    if command.beacon.is_some() {
        count += 1;
    }
    if count == 0 {
        // TODO: emit better errors
        // Zero assignments fond, expected one
        context
            .error("command action missing")
            .set_primary_span(span, "you must specify one command action")
            .emit();
        return Err(());
    } else if count > 1 {
        // More than one assignment found, expected one
        let mut values: std::vec::Vec<Span> = std::vec::Vec::new();
        if let Some(s) = &command.pyro1 {
            values.push(Span::from_spanned(context, s));
        }
        if let Some(s) = &command.pyro2 {
            values.push(Span::from_spanned(context, s));
        }
        if let Some(s) = &command.pyro3 {
            values.push(Span::from_spanned(context, s));
        }
        if let Some(s) = &command.data_rate {
            values.push(Span::from_spanned(context, s));
        }
        if let Some(s) = &command.beacon {
            values.push(Span::from_spanned(context, s));
        }

        let mut err = context.error("too many command actions").set_primary_span(
            span,
            format!(
                "you must specify exactly one command action, not {}",
                values.len()
            ),
        );

        for span in values {
            err = err.span_label(span, "declared here");
        }
        err.emit();
    }
    use common::CommandObject;
    //The user only set one option, now map that to an object and state
    let object = {
        if let Some(pyro1) = &command.pyro1 {
            CommandObject::Pyro1(pyro1.clone().into_inner().into())
        } else if let Some(pyro2) = &command.pyro2 {
            CommandObject::Pyro2(pyro2.clone().into_inner().into())
        } else if let Some(pyro3) = &command.pyro3 {
            CommandObject::Pyro3(pyro3.clone().into_inner().into())
        } else if let Some(data_rate) = &command.data_rate {
            CommandObject::DataRate(data_rate.clone().into_inner())
        } else if let Some(beacon) = &command.beacon {
            CommandObject::Beacon(beacon.clone().into_inner().into())
        } else {
            // We return an error if fewer or more than one of the options are set
            unreachable!("{command:?}")
        }
    };
    Ok(index::Command {
        object,
        delay: common::Seconds(
            command
                .delay
                .as_ref()
                .map(|d| d.clone().into_inner())
                .unwrap_or(0.0),
        ),
    })
}

pub(crate) fn convert_check(
    check: &Spanned<upper::Check>,
    temp: &Temp<'_>,
    context: &mut Context,
) -> Result<Check, ()> {
    let full_span = Span::from_spanned(context, check);
    let check = check.get_ref();
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
        context
            .error("too many check conditions")
            .set_primary_span(full_span, "you must specify one check condition per check")
            .emit();
    }
    if count > 1 {
        let mut spans: std::vec::Vec<Span> = std::vec::Vec::new();
        if let Some(gt) = &check.greater_than {
            spans.push(Span::from_spanned(context, gt));
        }
        if let (Some(u), Some(l)) = (&check.greater_than, &check.lower_bound) {
            spans.push(Span::from_spanned(context, u));
            spans.push(Span::from_spanned(context, l));
        }
        if let Some(flag) = &check.flag {
            spans.push(Span::from_spanned(context, flag));
        }

        let mut err = context.error("too many command actions").set_primary_span(
            full_span,
            format!(
                "you must specify exactly one check condition, not {}",
                spans.len()
            ),
        );

        for span in spans {
            err = err.span_label(span, "declared here");
        }
        err.emit();
        return Err(());
    }

    enum CheckKind {
        Apogee,
        Altitude,
        Pyro1Continuity,
        Pyro2Continuity,
        Pyro3Continuity,
    }
    let check_name = check.check.borrow();
    let check_kind = match check_name {
        "apogee" => CheckKind::Apogee,
        "altitude" => CheckKind::Altitude,
        "pyro1_continuity" => CheckKind::Pyro1Continuity,
        "pyro2_continuity" => CheckKind::Pyro2Continuity,
        "pyro3_continuity" => CheckKind::Pyro3Continuity,
        _ => {
            context
                .error(format!("no check with name `{check_name}` exists"))
                .set_primary_span(full_span, format!("unknown check `{check_name}`"))
                .emit();

            context
                .help("try `apogee`, or `altitude`, or `pyroX_continuity`")
                .set_primary_span_no_msg(full_span)
                .emit();

            return Err(());
        }
    };

    #[allow(dead_code)]
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
                _ => {
                    let span = Span::from_spanned(context, flag);
                    context
                        .error("flag values must be `set` or `unset`")
                        .set_primary_span(span, format!("unknown flag value `{check_name}`"))
                        .emit();
                    return Err(());
                }
            }
        } else {
            unreachable!()
        }
    };

    let mismatch_err = |context: &mut Context, span, span_msg| -> Result<!, ()> {
        let span = Span::from_spanned(context, span);
        context
            .error("mismatched check type")
            .set_primary_span(span, span_msg)
            .emit();
        Err(())
    };

    use common::{CheckData, FloatCondition, NativeFlagCondition, PyroContinuityCondition};
    // Perform type checking on kind and condition
    let data = match check_kind {
        CheckKind::Apogee => match condition {
            CheckCondition::FlagEq(val) => CheckData::ApogeeFlag(NativeFlagCondition(val)),
            _ => mismatch_err(context, &check.check, "")?,
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
        Some(state) => Some((temp.get_index(state, context)?, state)),
        None => None,
    };

    let abort = match &check.abort {
        Some(state) => Some((temp.get_index(state, context)?, state)),
        None => None,
    };

    let transition = match (transition, abort) {
        (Some(t), None) => Some(StateTransition::Transition(t.0)),
        (None, Some(a)) => Some(StateTransition::Abort(a.0)),
        (None, None) => None,
        (Some(mut t), Some(mut a)) => {
            if t.1.start() > a.1.start() {
                // swap a and t so that t is always first
                std::mem::swap(&mut t, &mut a);
            }
            let s1 = Span::from_spanned(context, t.1);
            let s2 = Span::from_spanned(context, a.1);
            context
                .error("abort and transition cannot be active in the same check")
                .set_primary_span_no_msg(s1)
                .span_label(s2, "second action declared here")
                .emit();

            context
                .help("remove either `abort` or `transition`")
                .add_span(s1)
                .add_span(s2)
                .emit();
            return Err(());
        }
    };

    Ok(index::Check::new(data, transition))
}

#[cfg(test)]
mod tests {
    use common::{index::StateIndex, CheckData, FloatCondition, PyroContinuityCondition};

    use super::{common, index};
    use crate::{upper, upper::cs, Session};

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

        check_ok(upper, expected);
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

        check_ok(upper, expected);
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
        check_error(upper);
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
        check_error(upper);
    }

    fn check_ok(input: upper::ConfigFile, expected: index::ConfigFile) {
        let mut session = Session::new();
        let mut context = session.testing("");
        let cfg_file = super::verify(input, &mut context);
        let errors = context.end_phase();

        match errors {
            Ok(d) => {
                d.emit();
                assert_eq!(expected, cfg_file.unwrap());
            }
            Err(d) => {
                d.emit();
                panic!("Low level verify failed");
            }
        }
    }

    fn check_error(input: upper::ConfigFile) {
        let mut session = Session::new();
        let mut context = session.testing("");
        let cfg_file = super::verify(input, &mut context);
        let errors = context.end_phase();

        match errors {
            Ok(d) => {
                d.emit();
                panic!(
                    "Low level verify should have failed, decoded to {:?}",
                    cfg_file.unwrap()
                );
            }
            Err(d) => {
                d.emit();
            }
        }
    }
}

//! Responsible for converting a high level toml string into a [`ConfigFile`].
//! This module's `ConfigFile` is slightly different from nova_software_common's. This one uses
//! string names to reference states and checks, which may not be linked. This struct server as a
//! high level bridge from the automated toml code and the low-level generator. This verify step only
//! checks for valid toml. The returned [`ConfigFile`] may reference state or check names that don't
//! exist, have negative timeouts, etc. This is the job of the low level verifier to check when it
//! converts our [`ConfigFile`] to [`nova_software_common::index::ConfigFile`]
use common::index;
use common::CommandObject;
use nova_software_common as common;

use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use toml::Spanned;

pub fn verify(toml: &str) -> Result<ConfigFile, crate::Error> {
    Ok(toml::from_str(toml)?)
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct ConfigFile {
    pub default_state: Option<Spanned<String>>,
    pub states: Spanned<Vec<Spanned<State>>>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct Timeout {
    /// How long this state can execute in seconds before the rocket automatically transitions to
    /// `state`
    pub seconds: Option<Spanned<f32>>,

    /// The state to transition to when `state`
    pub transition: Option<Spanned<String>>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct State {
    /// The name of this state
    pub name: Spanned<String>,

    pub timeout: Option<Spanned<Timeout>>,

    #[serde(default)]
    pub checks: Vec<Spanned<Check>>,

    #[serde(default)]
    pub commands: Vec<Spanned<Command>>,
}

/// Something relating to the external environment that the rocket will check to determine a future
/// course of action. Examples include:
/// - Transitioning from the `Ground` state to the `Launched` state if altitude is past a certain
/// threshold
/// - Aborting the flight if there is no continuity on the pyro channels
#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct Check {
    /// The name describing this check
    pub name: Spanned<String>,

    /// The name of the thing to be checked
    /// Currently only the strings `altitude`, `pyro1`, `pyro2`, and `pyro3` are supported, and
    /// enable specific filtering conditions
    pub check: Spanned<String>,

    /// The name of the state to transition to when when the check is tripped
    pub transition: Option<Spanned<String>>,

    /// The name of the state to abort to when this check is trpped.
    /// Muturallay exclusive with `transition`
    pub abort: Option<Spanned<String>>,

    /// If set, this check will execute when the value of `self.check` > the inner value
    /// Only available for `altitude` checks
    pub greater_than: Option<Spanned<f32>>,

    /// Forms a check range with `lower_bound` that checks if `check` is in a particular range
    /// Only available for `altitude` checks
    pub upper_bound: Option<Spanned<f32>>,

    /// Must be Some(...) if `upper_bound` is Some(...), and must be None if `upper_bound` is none
    pub lower_bound: Option<Spanned<f32>>,

    /// Checks if a boolean flag is set or unset
    /// The pyro values are supported
    /// `flag = "set"` or `flag = "unset"`
    ///
    /// If this flag is missing and `check` is set to a pyro value, then this value will default to
    /// checking for "set"
    pub flag: Option<Spanned<String>>,
}

/// Custom boolean that supports deserialising from toml booleans,
/// plus the strings "true", "false", "enable", and "disable"
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TomlBool(bool);

#[derive(Deserialize, Serialize, Debug, PartialEq)]
pub struct Command {
    pub pyro1: Option<Spanned<TomlBool>>,
    pub pyro2: Option<Spanned<TomlBool>>,
    pub pyro3: Option<Spanned<TomlBool>>,
    pub data_rate: Option<Spanned<u16>>,
    pub becan: Option<Spanned<TomlBool>>,
    pub delay: Option<Spanned<f32>>,
}

impl From<TomlBool> for bool {
    fn from(b: TomlBool) -> Self {
        b.0
    }
}

impl<'de> Deserialize<'de> for TomlBool {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use toml::Value;
        let value: Value = Value::deserialize(d)?;
        Ok(TomlBool(match value {
            Value::String(s) if s == "enable" => true,
            Value::String(s) if s == "disable" => false,
            //TODO: Should we support this? Users can do both `value = true` or `value = "true"`
            Value::String(s) if s == "true" => true,
            Value::String(s) if s == "false" => false,
            Value::Boolean(b) => b,
            _ => {
                return Err(serde::de::Error::invalid_value(
                    serde::de::Unexpected::Str(value.to_string().as_str()),
                    &"",
                ))
            }
        }))
    }
}

impl Serialize for TomlBool {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.serialize_bool(self.0)
    }
}

impl TryInto<index::Command> for &Command {
    type Error = crate::Error;

    fn try_into(self) -> Result<index::Command, Self::Error> {
        let mut count = 0;
        if self.pyro1.is_some() {
            count += 1;
        }
        if self.pyro2.is_some() {
            count += 1;
        }
        if self.pyro3.is_some() {
            count += 1;
        }
        if self.data_rate.is_some() {
            count += 1;
        }
        if self.becan.is_some() {
            count += 1;
        }
        if count == 0 {
            // TODO: emit better errors
            // Zero assignments fond, expected one
            return Err(crate::Error::Command(crate::CommandError::NoValues));
        } else if count > 1 {
            // TODO: emit better errors
            // More than one assignment found, expected one
            return Err(crate::Error::Command(crate::CommandError::TooManyValues(
                count,
            )));
        }
        //The user only set one option, now map that to an object and state
        let object = {
            if let Some(pyro1) = &self.pyro1 {
                CommandObject::Pyro1(pyro1.clone().into_inner().into())
            } else if let Some(pyro2) = &self.pyro2 {
                CommandObject::Pyro2(pyro2.clone().into_inner().into())
            } else if let Some(pyro3) = &self.pyro3 {
                CommandObject::Pyro3(pyro3.clone().into_inner().into())
            } else if let Some(data_rate) = &self.data_rate {
                CommandObject::DataRate(data_rate.clone().into_inner())
            } else if let Some(becan) = &self.becan {
                CommandObject::Beacon(becan.clone().into_inner().into())
            } else {
                // We return an error if fewer or more than one of the options are set
                unreachable!()
            }
        };
        Ok(index::Command {
            object,
            delay: common::Seconds(
                self.delay
                    .as_ref()
                    .map(|d| d.clone().into_inner())
                    .unwrap_or(0.0),
            ),
        })
    }
}

impl TryInto<index::Command> for Command {
    type Error = crate::Error;

    fn try_into(self) -> Result<index::Command, Self::Error> {
        (&self).try_into()
    }
}

/// Creates a dummy `toml::Spanned` with `value` inside.
/// Short for create_spanned
#[cfg(test)]
pub(crate) fn cs<T>(value: T) -> Spanned<T> {
    // Very sad. Nothing about Spanned is public, so to make these tests work we need to do
    // a nasty transume to create a dummy span
    // We could avoid this by deserializing from a toml string, but we already do that as
    // part of the integration tests, so we must do this wizardy to test this specific
    // upper -> lower conversion code. Put your pitchforks away and stop crying
    //
    // Spanned struct as of `toml = "0.5.8"`:
    // Lets hope the compiler chooses the same layout as Spanned<T>...
    #[allow(dead_code)]
    pub struct MySpanned<T> {
        /// The start range.
        start: usize,
        /// The end range (exclusive).
        end: usize,
        /// The spanned value.
        value: T,
    }
    let spanned = MySpanned {
        start: 0,// We dont actually care about these values so use 0
        end: 0,
        value,
    };
    assert_eq!(
        std::mem::size_of::<MySpanned<T>>(),
        std::mem::size_of::<Spanned<T>>()
    );
    let ptr: *const MySpanned<T> = &spanned;
    let ptr: *const Spanned<T> = ptr as *const _;
    let result: Spanned<T> = unsafe { std::ptr::read(ptr) };

    std::mem::forget(spanned);
    result
}

#[cfg(test)]
mod tests {

    mod config {

        use crate::upper::{cs, verify, Check, ConfigFile, State};

        #[test]
        fn basic_serialize1() {
            let expected = ConfigFile {
                default_state: Some(cs("PowerOn".to_owned())),
                states: cs(vec![cs(State {
                    name: cs("PowerOn".to_owned()),
                    checks: vec![],
                    commands: vec![],
                    timeout: None,
                })]),
            };
            let config = r#"default_state = "PowerOn"

[[states]]
name = "PowerOn"
checks = []
"#;

            let parsed = verify(config).unwrap();
            assert_eq!(parsed, expected);
        }

        #[test]
        fn basic_serialize2() {
            let expected = ConfigFile {
                default_state: Some(cs("PowerOn".to_owned())),
                states: cs(vec![cs(State {
                    name: cs("PowerOn".to_owned()),
                    timeout: None,
                    checks: vec![cs(Check {
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

            let config = r#"default_state = "PowerOn"

[[states]]
name = "PowerOn"

[[states.checks]]
name = "Takeoff"
check = "altitude"
greater_than = 100.0
"#;

            let parsed = verify(config).unwrap();
            assert_eq!(parsed, expected);
        }
    }

    mod toml_bool {
        use crate::upper::TomlBool;
        use serde::Deserialize;

        /// plus the strings "true", "false", "enable", and "disable"
        #[test]
        fn de() {
            #[derive(Deserialize, PartialEq, Eq, Debug)]
            struct A {
                ok: TomlBool,
            }
            let s = r#"ok = "true""#;
            let e = A { ok: TomlBool(true) };
            assert_eq!(toml::from_str::<A>(s).unwrap(), e);

            let s = r#"ok = true"#;
            let e = A { ok: TomlBool(true) };
            assert_eq!(toml::from_str::<A>(s).unwrap(), e);

            let s = r#"ok = false"#;
            let e = A {
                ok: TomlBool(false),
            };
            assert_eq!(toml::from_str::<A>(s).unwrap(), e);

            let s = r#"ok = "enable""#;
            let e = A { ok: TomlBool(true) };
            assert_eq!(toml::from_str::<A>(s).unwrap(), e);

            let s = r#"ok = "disable""#;
            let e = A {
                ok: TomlBool(false),
            };
            assert_eq!(toml::from_str::<A>(s).unwrap(), e);
        }
    }

    mod command {
        use crate::upper::{cs, Command, TomlBool};
        use nova_software_common as common;
        #[test]
        fn a() {
            let expected = common::index::Command::new(
                common::CommandObject::Pyro1(true),
                common::Seconds(0.0),
            );

            let initial = Command {
                pyro1: Some(cs(TomlBool(true))),
                pyro2: None,
                pyro3: None,
                data_rate: None,
                becan: None,
                delay: None,
            };
            assert_eq!(expected, initial.try_into().unwrap());
        }
    }
}

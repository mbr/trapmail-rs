//! Helper functions to serialize a `Pid` type from the `nix` crate.

use nix::unistd::Pid;
use serde::{Deserialize, Deserializer, Serializer};

/// Serialization function, to be used with `serialize_with`.
pub fn serialize<S>(pid: &Pid, ser: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    ser.serialize_i32(pid.as_raw())
}

/// Serialization function, to be used with `deserialize_with`.
pub fn deserialize<'de, D>(de: D) -> Result<Pid, D::Error>
where
    D: Deserializer<'de>,
{
    i32::deserialize(de).map(Pid::from_raw)
}

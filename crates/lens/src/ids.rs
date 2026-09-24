//! Identifiers: taps, listeners and captured exchanges.

use std::{fmt, str::FromStr, sync::Arc};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::LensError;

/// Identifies a tap (one inspected share, route or folder).
///
/// The embedder chooses it (e.g. `share-3f2a` or `route-8c1d`) so captures persisted by
/// a [`crate::CaptureStore`] stay attributable across restarts, or lets Lens generate
/// one. 1–64 characters of `A–Z a–z 0–9 . _ : -`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(transparent))]
pub struct TapId(#[cfg_attr(feature = "specta", specta(type = String))] Arc<str>);

impl TapId {
    /// Validates and wraps an id.
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] when empty, longer than 64 characters, or containing
    /// other characters.
    pub fn new(id: &str) -> Result<Self, LensError> {
        let valid = !id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
        if valid {
            Ok(Self(Arc::from(id)))
        } else {
            Err(LensError::InvalidConfig(format!(
                "tap id {id:?} must be 1–64 characters of letters, digits, '.', '_', ':' or '-'"
            )))
        }
    }

    /// A fresh random id, `tap-` followed by 12 hex digits.
    pub(crate) fn random() -> Self {
        let uuid = Uuid::new_v4().simple().to_string();
        Self(Arc::from(format!("tap-{}", &uuid[..12]).as_str()))
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for TapId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TapId({})", self.0)
    }
}

impl fmt::Display for TapId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TapId {
    type Err = LensError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Serialize for TapId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TapId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

/// Identifies one captured exchange across all taps.
///
/// A UUIDv7: unique, and ordered by creation time within the process, so it doubles as
/// a pagination cursor. Each exchange also has a per-tap sequence number for display.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(transparent))]
pub struct ExchangeId(#[cfg_attr(feature = "specta", specta(type = String))] Uuid);

impl ExchangeId {
    pub(crate) fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// The underlying UUID.
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl From<Uuid> for ExchangeId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl FromStr for ExchangeId {
    type Err = LensError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| LensError::InvalidConfig(format!("{s:?} isn't an exchange id")))
    }
}

impl fmt::Debug for ExchangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExchangeId({})", self.0)
    }
}

impl fmt::Display for ExchangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Identifies a listening socket.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ListenerId(pub(crate) u64);

impl fmt::Debug for ListenerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ListenerId({})", self.0)
    }
}

impl fmt::Display for ListenerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "listener-{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_ids_are_validated() {
        assert!(TapId::new("share:abc-1.2_3").is_ok());
        assert!(TapId::new("").is_err());
        assert!(TapId::new("has space").is_err());
        assert!(TapId::new(&"a".repeat(65)).is_err());
        let random = TapId::random();
        assert!(TapId::new(random.as_str()).is_ok());
        assert_eq!(random.as_str().len(), 16);
    }

    #[test]
    fn exchange_ids_are_time_ordered() {
        let a = ExchangeId::new();
        let b = ExchangeId::new();
        assert!(a < b);
        assert_eq!(a.to_string().parse::<ExchangeId>().ok(), Some(a));
    }

    #[test]
    fn tap_id_serde_validates() {
        let ok: Result<TapId, _> = serde_json::from_str("\"t1\"");
        assert!(ok.is_ok());
        let bad: Result<TapId, _> = serde_json::from_str("\"t 1\"");
        assert!(bad.is_err());
    }
}

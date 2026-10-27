use uuid::Uuid;

use crate::error::Error;

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(Uuid);

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

id_type!(PrincipalId, "A person, an agent or an integration.");

/// Service principal used when a migration or a startup upgrade writes.
///
/// The bytes are `md5("principal:system")` with the UUID version nibble set to 3
/// and the RFC 4122 variant. V23 stored the same value while it moved relations.
pub const SYSTEM_PRINCIPAL: PrincipalId =
    PrincipalId::from_uuid(Uuid::from_u128(0xc08766d6_3ee2_3ab0_8028_0738e61d0e4c));

id_type!(
    BatchId,
    "One logical write. A bulk batch can span several transactions."
);

/// Who a write is attributed to.
///
/// An agent principal is rejected unless `on_behalf_of` names the person the
/// agent acts for. The kind is read from `principals`, not from this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub actor: PrincipalId,
    pub on_behalf_of: Option<PrincipalId>,
    pub kind: BatchKind,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Interactive,
    Bulk,
}

impl BatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Bulk => "bulk",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    Human,
    Agent,
    Integration,
}

impl PrincipalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
            Self::Integration => "integration",
        }
    }

    pub fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "human" => Ok(Self::Human),
            "agent" => Ok(Self::Agent),
            "integration" => Ok(Self::Integration),
            other => Err(Error::Data(format!("unknown principal kind {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOp {
    Create,
    Update,
    Delete,
    Definitions,
}

impl ChangeOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Definitions => "definitions",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            "definitions" => Some(Self::Definitions),
            _ => None,
        }
    }
}

/// Rejects anything that is not a plain Postgres identifier we generated.
pub fn plain_name(name: &str) -> Result<&str, Error> {
    let ok = name.len() <= 63
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase());
    if ok {
        Ok(name)
    } else {
        Err(Error::Data(format!("refusing database name {name}")))
    }
}

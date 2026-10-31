//! Dev tokens from `BRAIAN_DEV_TOKENS`.
//!
//! Each entry is `handle:kind[@behalf]:token`, separated by `;`. An agent entry
//! always names the person it acts for. Tokens are compared without cutting the
//! check short, and they are not included in `Debug`.

use std::fmt;

use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use braian_store::{Actor, BatchKind, Error, PrincipalId, PrincipalKind, Store};
use sha2::{Digest, Sha256};

use crate::problem::Problem;
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "braian_session";

#[derive(Clone)]
pub struct Identity {
    token: String,
    pub handle: String,
    /// Login email when this identity is a person session or token owner.
    pub email: Option<String>,
    /// Shown name when known; UI falls back to handle.
    pub display_name: Option<String>,
    pub kind: PrincipalKind,
    pub behalf_handle: Option<String>,
    pub actor: Actor,
    pub role: Role,
    pub user_id: Option<uuid::Uuid>,
}

/// What a caller is allowed to do. Admin includes editor, editor includes viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Editor,
    Viewer,
}

impl Role {
    pub fn parse(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            "editor" => Self::Editor,
            _ => Self::Viewer,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Editor => "editor",
            Self::Viewer => "viewer",
        }
    }

    pub fn allows(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::Admin, _)
                | (Self::Editor, Self::Editor | Self::Viewer)
                | (Self::Viewer, Self::Viewer)
        )
    }
}

impl Identity {
    /// A catalog save is a person's act. Agents and imports file suggestions.
    pub fn may_commit(&self) -> bool {
        self.kind == PrincipalKind::Human
    }

    /// A caller whose kind is the row in `principals`, not a token spec.
    ///
    /// Used to check the migration's System principal. Building the identity
    /// from a dev token would upsert the kind and hide a `human` row.
    #[doc(hidden)]
    pub fn from_principal(record: &braian_store::PrincipalRecord) -> Self {
        Self {
            token: String::new(),
            handle: record.handle.clone().unwrap_or_else(|| record.name.clone()),
            email: None,
            display_name: Some(record.name.clone()),
            kind: record.kind,
            behalf_handle: None,
            actor: Actor {
                actor: record.id,
                on_behalf_of: None,
                kind: BatchKind::Interactive,
                source: "principal".into(),
            },
            role: Role::Admin,
            user_id: None,
        }
    }
}

/// `agent` for an agent or for Ctrl+K acting as the person. `import` for an integration.
pub fn suggestion_source(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Integration => "import",
        PrincipalKind::Agent | PrincipalKind::Human => "agent",
    }
}

pub fn delete_forbidden() -> Problem {
    Problem::new(
        axum::http::StatusCode::FORBIDDEN,
        "agent_cannot_delete",
        "An agent or MCP token cannot delete.",
    )
}

pub fn commit_forbidden() -> Problem {
    Problem::new(
        axum::http::StatusCode::FORBIDDEN,
        "needs_approval",
        "An agent or import files a suggestion. Only a person can save.",
    )
}

pub fn require_commit(identity: &Identity) -> Result<(), Problem> {
    if identity.may_commit() {
        Ok(())
    } else {
        Err(commit_forbidden())
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("handle", &self.handle)
            .field("kind", &self.kind)
            .field("behalf_handle", &self.behalf_handle)
            .field("actor", &self.actor.actor)
            .field("role", &self.role)
            .finish()
    }
}

pub fn parse_dev_tokens(raw: &str) -> Result<Vec<IdentitySpec>, String> {
    let mut specs = Vec::new();
    for entry in raw.split(';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        specs.push(parse_one(entry)?);
    }
    if specs.is_empty() {
        return Err("BRAIAN_DEV_TOKENS is empty".into());
    }
    Ok(specs)
}

/// A parsed token before principals exist.
pub struct IdentitySpec {
    pub handle: String,
    pub kind: PrincipalKind,
    pub behalf: Option<String>,
    pub token: String,
}

fn parse_one(entry: &str) -> Result<IdentitySpec, String> {
    let Some((handle, rest)) = entry.split_once(':') else {
        return Err("a token entry needs handle:kind:token".into());
    };
    let Some((kind_part, token)) = rest.split_once(':') else {
        return Err(format!("token entry for {handle} needs a token"));
    };
    let (kind_name, behalf) = match kind_part.split_once('@') {
        Some((kind, behalf)) => (kind, Some(behalf.to_owned())),
        None => (kind_part, None),
    };
    let kind = match kind_name {
        "human" => PrincipalKind::Human,
        "agent" => PrincipalKind::Agent,
        "integration" => PrincipalKind::Integration,
        other => return Err(format!("unknown principal kind {other}")),
    };
    if handle.is_empty() || token.is_empty() {
        return Err("token handle and token must not be empty".into());
    }
    if kind == PrincipalKind::Agent && behalf.is_none() {
        return Err(format!("agent {handle} requires @behalf"));
    }
    if kind != PrincipalKind::Agent && behalf.is_some() {
        return Err(format!("{handle} is not an agent and cannot name a behalf"));
    }
    Ok(IdentitySpec {
        handle: handle.to_owned(),
        kind,
        behalf,
        token: token.to_owned(),
    })
}

/// Upserts one principal per handle and returns identities the router can match.
pub async fn identities_from(store: &Store, raw: &str) -> Result<Vec<Identity>, Error> {
    let specs = parse_dev_tokens(raw).map_err(Error::Data)?;
    let mut ids: Vec<(String, PrincipalId)> = Vec::new();
    for spec in specs
        .iter()
        .filter(|spec| spec.kind != PrincipalKind::Agent)
    {
        let id = store
            .upsert_principal(&spec.handle, spec.kind, &spec.handle)
            .await?;
        ids.push((spec.handle.clone(), id));
    }
    for spec in specs
        .iter()
        .filter(|spec| spec.kind == PrincipalKind::Agent)
    {
        let id = store
            .upsert_principal(&spec.handle, spec.kind, &spec.handle)
            .await?;
        ids.push((spec.handle.clone(), id));
    }
    let mut identities = Vec::with_capacity(specs.len());
    for spec in specs {
        let behalf = match &spec.behalf {
            Some(handle) => Some(
                ids.iter()
                    .find(|(name, _)| name == handle)
                    .map(|(_, id)| *id)
                    .ok_or_else(|| Error::Data(format!("unknown behalf {handle}")))?,
            ),
            None => None,
        };
        let actor_id = ids
            .iter()
            .find(|(name, _)| name == &spec.handle)
            .map(|(_, id)| *id)
            .ok_or_else(|| Error::Data(format!("principal {} was not stored", spec.handle)))?;
        identities.push(Identity {
            token: spec.token,
            handle: spec.handle.clone(),
            email: None,
            display_name: Some(spec.handle),
            kind: spec.kind,
            behalf_handle: spec.behalf,
            actor: Actor {
                actor: actor_id,
                on_behalf_of: behalf,
                kind: BatchKind::Interactive,
                source: "api".into(),
            },
            role: if spec.kind == PrincipalKind::Human {
                Role::Admin
            } else {
                Role::Editor
            },
            user_id: None,
        });
    }
    Ok(identities)
}

pub struct Auth(pub Identity);

impl FromRequestParts<AppState> for Auth {
    type Rejection = Problem;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer(parts) {
            if let Some(identity) = state
                .identities
                .iter()
                .find(|identity| constant_time_eq(identity.token.as_bytes(), token.as_bytes()))
            {
                return Ok(Auth(identity.clone()));
            }
            if let Some(identity) = identity_for_api_token(state, token).await? {
                return Ok(Auth(identity));
            }
            return Err(Problem::unauthorized());
        }
        if let Some(token) = cookie(parts, SESSION_COOKIE)
            && let Some(identity) = identity_for_session(state, &token).await?
        {
            return Ok(Auth(identity));
        }
        Err(Problem::unauthorized())
    }
}

pub fn hash_password(password: &str) -> Result<String, String> {
    let mut salt_bytes = [0u8; 16];
    getrandom::fill(&mut salt_bytes).map_err(|error| error.to_string())?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|error| error.to_string())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| error.to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    if getrandom::fill(&mut bytes).is_err() {
        let first = uuid::Uuid::now_v7();
        let second = uuid::Uuid::now_v7();
        bytes[..16].copy_from_slice(first.as_bytes());
        bytes[16..].copy_from_slice(second.as_bytes());
    }
    hex::encode(bytes)
}

fn bearer(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

pub fn cookie(parts: &Parts, name: &str) -> Option<String> {
    let header = parts
        .headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?;
    for part in header.split(';') {
        let (key, value) = part.trim().split_once('=')?;
        if key == name {
            return Some(value.to_owned());
        }
    }
    None
}

fn identity_from_user(
    user: &braian_store::UserRecord,
    kind: PrincipalKind,
    actor: PrincipalId,
    behalf: Option<PrincipalId>,
    source: &str,
) -> Identity {
    Identity {
        token: String::new(),
        handle: user.email.clone(),
        email: Some(user.email.clone()),
        display_name: Some(user.display_name.clone()),
        kind,
        behalf_handle: None,
        actor: Actor {
            actor,
            on_behalf_of: behalf,
            kind: BatchKind::Interactive,
            source: source.to_owned(),
        },
        role: Role::parse(&user.role),
        user_id: Some(user.id),
    }
}

async fn identity_for_session(state: &AppState, token: &str) -> Result<Option<Identity>, Problem> {
    let user = state
        .store
        .user_for_session(&token_hash(token))
        .await
        .map_err(crate::problem::store_status)?;
    Ok(user
        .map(|user| identity_from_user(&user, PrincipalKind::Human, user.principal_id, None, "ui")))
}

async fn identity_for_api_token(
    state: &AppState,
    token: &str,
) -> Result<Option<Identity>, Problem> {
    let found = state
        .store
        .api_token(&token_hash(token))
        .await
        .map_err(crate::problem::store_status)?;
    let Some((principal, owner, kind)) = found else {
        return Ok(None);
    };
    let principal_kind = if kind == "agent" {
        PrincipalKind::Agent
    } else {
        PrincipalKind::Integration
    };
    let mut identity = identity_from_user(
        &owner,
        principal_kind,
        principal,
        Some(owner.principal_id),
        if kind == "agent" { "agent" } else { "api" },
    );
    identity.behalf_handle = Some(owner.email.clone());
    identity.handle = format!("{kind}:{principal}");
    Ok(Some(identity))
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}={token}; HttpOnly; Path=/; SameSite=Lax; Max-Age=1209600{secure}")
}

pub fn clear_session_cookie(secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0{secure}")
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    let len = left.len().max(right.len());
    for index in 0..len {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        diff |= (a ^ b) as usize;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{hash_password, parse_dev_tokens, verify_password};

    #[test]
    fn password_hash_stays_fast_when_the_debug_profile_optimizes_argon2() {
        let started = std::time::Instant::now();
        let hash = hash_password("correct horse").expect("hash");
        assert!(verify_password(&hash, "correct horse"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "hashing took {:?}; argon2 and blake2 need opt-level in the dev profile",
            started.elapsed()
        );
    }

    #[test]
    fn an_agent_token_must_name_a_person() {
        assert!(parse_dev_tokens("ada:agent:secret").is_err());
        assert!(parse_dev_tokens("ada:human@someone:secret").is_err());
        let specs = parse_dev_tokens("ada:human:secret;bot:agent@ada:other").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[1].behalf.as_deref(), Some("ada"));
    }
}

//! Dev tokens from `OPENPIM_DEV_TOKENS`.
//!
//! Each entry is `handle:kind[@behalf]:token`, separated by `;`. An agent entry
//! always names the person it acts for. Tokens are compared without cutting the
//! check short, and they are not included in `Debug`.

use std::fmt;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use openpim_store::{Actor, BatchKind, Error, PrincipalId, PrincipalKind, Store};

use crate::problem::Problem;
use crate::state::AppState;

#[derive(Clone)]
pub struct Identity {
    token: String,
    pub handle: String,
    pub kind: PrincipalKind,
    pub behalf_handle: Option<String>,
    pub actor: Actor,
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("handle", &self.handle)
            .field("kind", &self.kind)
            .field("behalf_handle", &self.behalf_handle)
            .field("actor", &self.actor.actor)
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
        return Err("OPENPIM_DEV_TOKENS is empty".into());
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
            handle: spec.handle,
            kind: spec.kind,
            behalf_handle: spec.behalf,
            actor: Actor {
                actor: actor_id,
                on_behalf_of: behalf,
                kind: BatchKind::Interactive,
                source: "api".into(),
            },
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
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        let Some(token) = header.and_then(|value| value.strip_prefix("Bearer ")) else {
            return Err(Problem::unauthorized());
        };
        let Some(identity) = state
            .identities
            .iter()
            .find(|identity| constant_time_eq(identity.token.as_bytes(), token.as_bytes()))
        else {
            return Err(Problem::unauthorized());
        };
        Ok(Auth(identity.clone()))
    }
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
    use super::parse_dev_tokens;

    #[test]
    fn an_agent_token_must_name_a_person() {
        assert!(parse_dev_tokens("ada:agent:secret").is_err());
        assert!(parse_dev_tokens("ada:human@someone:secret").is_err());
        let specs = parse_dev_tokens("ada:human:secret;bot:agent@ada:other").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[1].behalf.as_deref(), Some("ada"));
    }
}

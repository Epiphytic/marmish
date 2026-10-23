//! Additive executable-selection restrictions. These do not sandbox a program.
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub const MAX_KEY_ENTRIES: usize = 128;
pub const MAX_PATTERNS: usize = 256;
pub const MAX_PATTERN_BYTES: usize = 1024;
const MATCH_FUEL: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandPolicy {
    pub version: u32,
    #[serde(default)]
    pub defaults: Restrictions,
    #[serde(default)]
    pub keys: Vec<KeyRestrictions>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRestrictions {
    pub principal: PrincipalId,
    pub restrictions: Restrictions,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restrictions {
    #[serde(default)]
    pub allow_exec: Option<Vec<String>>,
    #[serde(default)]
    pub deny_exec: Vec<String>,
}

impl CommandPolicy {
    pub(crate) fn validate(&self, admitted: &[PrincipalId]) -> Result<()> {
        if self.version != 1
            || self.keys.len() > MAX_KEY_ENTRIES
            || self
                .keys
                .iter()
                .any(|entry| !admitted.contains(&entry.principal))
        {
            return Err(EngineError::InvalidPolicy);
        }
        let mut count = 0;
        for layer in
            std::iter::once(&self.defaults).chain(self.keys.iter().map(|k| &k.restrictions))
        {
            for pattern in layer.patterns() {
                count += 1;
                if count > MAX_PATTERNS {
                    return Err(EngineError::InvalidPolicy);
                }
                compile(pattern)?;
            }
        }
        Ok(())
    }

    fn layers(&self, principal: PrincipalId) -> impl Iterator<Item = &Restrictions> {
        std::iter::once(&self.defaults).chain(
            self.keys
                .iter()
                .filter(move |k| k.principal == principal)
                .map(|k| &k.restrictions),
        )
    }
}

impl Restrictions {
    fn patterns(&self) -> impl Iterator<Item = &String> {
        self.allow_exec.iter().flatten().chain(&self.deny_exec)
    }
    fn effective(&self) -> bool {
        self.allow_exec.is_some() || !self.deny_exec.is_empty()
    }
}

#[derive(Clone, Copy)]
enum Token {
    Literal(u8),
    Star,
    DoubleStar,
}

fn compile(pattern: &str) -> Result<Vec<Token>> {
    if pattern.len() > MAX_PATTERN_BYTES
        || !pattern.starts_with('/')
        || pattern.chars().any(char::is_control)
    {
        return Err(EngineError::InvalidPolicy);
    }
    let mut bytes = pattern.bytes().peekable();
    let mut tokens = Vec::with_capacity(pattern.len());
    while let Some(byte) = bytes.next() {
        tokens.push(match byte {
            b'\\' => match bytes.next() {
                Some(literal @ (b'*' | b'\\')) => Token::Literal(literal),
                _ => return Err(EngineError::InvalidPolicy),
            },
            b'*' => {
                if bytes.peek() == Some(&b'*') {
                    while bytes.peek() == Some(&b'*') {
                        bytes.next();
                    }
                    Token::DoubleStar
                } else {
                    Token::Star
                }
            }
            literal => Token::Literal(literal),
        });
    }
    Ok(tokens)
}

fn matches(tokens: &[Token], value: &[u8]) -> bool {
    let mut previous = vec![false; value.len() + 1];
    let mut current = previous.clone();
    previous[0] = true;
    for token in tokens {
        current.fill(false);
        current[0] = previous[0] && matches!(token, Token::Star | Token::DoubleStar);
        for (index, byte) in value.iter().enumerate() {
            current[index + 1] = match token {
                Token::Literal(literal) => previous[index] && literal == byte,
                Token::Star => previous[index + 1] || (*byte != b'/' && current[index]),
                Token::DoubleStar => previous[index + 1] || current[index],
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[value.len()]
}

/// Pure validation, suitable outside or inside admission's transaction boundary.
/// Returns whether this accepted job must enforce canonical spelling at launch.
pub(crate) fn check_command(
    policy: &EnginePolicy,
    principal: PrincipalId,
    request: &CommandRequest,
) -> Result<bool> {
    let Some(commands) = &policy.command_policy else {
        return Ok(false);
    };
    let layers: Vec<_> = commands
        .layers(principal)
        .filter(|l| l.effective())
        .collect();
    if layers.is_empty() {
        return Ok(false);
    }
    let CommandRequest::Exec { argv } = request else {
        return Err(EngineError::CommandDenied);
    };
    let executable = argv.first().ok_or(EngineError::CommandDenied)?;
    if executable.len() > 4095
        || executable.first() != Some(&b'/')
        || executable[1..]
            .split(|b| *b == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err(EngineError::CommandDenied);
    }
    // Preflight the entire applicable policy, even patterns that short-circuit.
    // This makes budget exhaustion independent of rule and key-entry ordering.
    let mut fuel = MATCH_FUEL;
    let mut compiled = Vec::new();
    for layer in &layers {
        for pattern in layer.patterns() {
            let tokens = compile(pattern).map_err(|_| EngineError::CommandDenied)?;
            let cost = (tokens.len() + 1)
                .checked_mul(executable.len() + 1)
                .ok_or(EngineError::CommandDenied)?;
            fuel = fuel.checked_sub(cost).ok_or(EngineError::CommandDenied)?;
            compiled.push(tokens);
        }
    }
    let mut patterns = compiled.iter();
    for layer in layers {
        let mut allowed = layer.allow_exec.is_none();
        for _ in layer.allow_exec.iter().flatten() {
            allowed |= matches(patterns.next().unwrap(), executable);
        }
        let mut denied = false;
        for _ in &layer.deny_exec {
            denied |= matches(patterns.next().unwrap(), executable);
        }
        if !allowed || denied {
            return Err(EngineError::CommandDenied);
        }
    }
    Ok(true)
}

pub(crate) fn require_canonical_exec(request: &CommandRequest) -> Result<()> {
    let CommandRequest::Exec { argv } = request else {
        return Err(EngineError::CommandDenied);
    };
    let executable = argv.first().ok_or(EngineError::CommandDenied)?;
    if executable.is_empty()
        || executable.len() > 4095
        || executable[0] != b'/'
        || executable.contains(&0)
    {
        return Err(EngineError::CommandDenied);
    }
    let resolved = std::fs::canonicalize(Path::new(std::ffi::OsStr::from_bytes(executable)))
        .map_err(|_| EngineError::CommandDenied)?;
    if resolved.as_os_str().as_bytes() != executable {
        return Err(EngineError::CommandDenied);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

//! A read-only view of the running server's configured clients.
use crate::command_config::{CommandConfig, Role, account};
use crate::command_wire::Account;
use crate::journal::{ClientInventoryAccess, Journal};
use marmish_engine::{CommandPolicy, CommandPolicyDescriptionV1, PrincipalId};
use nostr::key::PublicKey;
use nostr::nips::nip19::ToBech32;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::ops::Bound::{Excluded, Unbounded};

pub(crate) const INVENTORY_VERSION: u32 = 1;
const MAX_LIST_LIMIT: usize = 64;
const MAX_POLICY_PAGES: usize = 64;
const MAX_POLICY_PAGE_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "query", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InventoryRequest {
    List {
        after: Option<String>,
        limit: usize,
    },
    Show {
        npub: String,
        policy_page: usize,
        policy_generation: Option<String>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientInventoryReply {
    pub(crate) version: u32,
    pub(crate) result: InventoryResult,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InventoryResult {
    List {
        clients: Vec<ClientInventoryEntry>,
        next: Option<String>,
    },
    Show {
        client: ClientInventoryEntry,
        policy: Box<InventoryPolicyPage>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientInventoryEntry {
    pub(crate) npub: String,
    pub(crate) access: ClientInventoryAccess,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InventoryPolicyPage {
    version: u32,
    generation: String,
    page: usize,
    next_page: Option<usize>,
    semantics: PolicySemantics,
    rules: Vec<PolicyRule>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PolicySemantics {
    allow: String,
    deny: String,
    glob: String,
    resolution: String,
    caveats: String,
    shell_command_enabled: bool,
    interactive_shell_enabled: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "rule", rename_all = "snake_case", deny_unknown_fields)]
enum PolicyRule {
    Allow { layer: usize, pattern: String },
    AllowEmpty { layer: usize },
    Deny { layer: usize, pattern: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InventoryError {
    InvalidRequest,
    ServerUnavailable,
    ClientUnknown,
    PolicyUnavailable,
    PolicyStale,
    DescriptionTooLarge,
    ResponseTooLarge,
    ResponseEncoding,
    Storage,
    Busy,
    Deadline,
}

impl InventoryError {
    pub(crate) fn category(self) -> &'static str {
        match self {
            Self::InvalidRequest => "inventory_invalid_request",
            Self::ServerUnavailable => "inventory_server_unavailable",
            Self::ClientUnknown => "inventory_client_unknown",
            Self::PolicyUnavailable => "inventory_policy_unavailable",
            Self::PolicyStale => "inventory_policy_stale",
            Self::DescriptionTooLarge => "inventory_description_too_large",
            Self::ResponseTooLarge => "inventory_response_too_large",
            Self::ResponseEncoding => "inventory_response_encoding",
            Self::Storage => "inventory_storage",
            Self::Busy => "busy",
            Self::Deadline => "operation_uncertain",
        }
    }

    pub(crate) fn from_category(category: &str) -> Option<Self> {
        [
            Self::InvalidRequest,
            Self::ServerUnavailable,
            Self::ClientUnknown,
            Self::PolicyUnavailable,
            Self::PolicyStale,
            Self::DescriptionTooLarge,
            Self::ResponseTooLarge,
            Self::ResponseEncoding,
            Self::Storage,
            Self::Busy,
            Self::Deadline,
        ]
        .into_iter()
        .find(|error| error.category() == category)
    }
}

type Result<T> = std::result::Result<T, InventoryError>;

pub(crate) fn query(
    journal: &Journal,
    config: &CommandConfig,
    policy: Option<&CommandPolicy>,
    request: InventoryRequest,
) -> Result<ClientInventoryReply> {
    if !matches!(config.role(), Role::Server | Role::Both) {
        return Err(InventoryError::ServerUnavailable);
    }
    let keys = config.execution_clients();
    let result = match request {
        InventoryRequest::List { after, limit } => {
            if !(1..=MAX_LIST_LIMIT).contains(&limit) {
                return Err(InventoryError::InvalidRequest);
            }
            let lower = after
                .as_deref()
                .map(canonical_account)
                .transpose()?
                .map_or(Unbounded, Excluded);
            let mut remaining = keys.range((lower, Unbounded));
            let clients = remaining
                .by_ref()
                .take(limit)
                .map(|key| entry(journal, key))
                .collect::<Result<Vec<_>>>()?;
            let next = remaining
                .next()
                .and_then(|_| clients.last().map(|client| client.npub.clone()));
            InventoryResult::List { clients, next }
        }
        InventoryRequest::Show {
            npub,
            policy_page,
            policy_generation,
        } => {
            let key = canonical_account(&npub)?;
            if !keys.contains(&key) {
                return Err(InventoryError::ClientUnknown);
            }
            let policy = policy.ok_or(InventoryError::PolicyUnavailable)?;
            let policy = describe(policy, key, policy_page, policy_generation.as_deref())?;
            InventoryResult::Show {
                client: entry(journal, &key)?,
                policy: Box::new(policy),
            }
        }
    };
    Ok(ClientInventoryReply {
        version: INVENTORY_VERSION,
        result,
    })
}

fn npub(key: &Account) -> Result<String> {
    PublicKey::from_slice(key)
        .map_err(|_| InventoryError::InvalidRequest)?
        .to_bech32()
        .map_err(|_| InventoryError::InvalidRequest)
}

fn canonical_account(value: &str) -> Result<Account> {
    if value.len() != 63 || !value.starts_with("npub1") {
        return Err(InventoryError::InvalidRequest);
    }
    let key = account(value).map_err(|_| InventoryError::InvalidRequest)?;
    if npub(&key)? != value {
        return Err(InventoryError::InvalidRequest);
    }
    Ok(key)
}

fn entry(journal: &Journal, key: &Account) -> Result<ClientInventoryEntry> {
    let access = journal
        .client_inventory_access(key)
        .map_err(|_| InventoryError::Storage)?;
    Ok(ClientInventoryEntry {
        npub: npub(key)?,
        access,
    })
}

fn describe(
    policy: &CommandPolicy,
    key: Account,
    wanted: usize,
    expected_generation: Option<&str>,
) -> Result<InventoryPolicyPage> {
    if wanted >= MAX_POLICY_PAGES || (wanted > 0 && expected_generation.is_none()) {
        return Err(InventoryError::InvalidRequest);
    }
    if expected_generation.is_some_and(|value| !crate::foundation::valid_hex(value, 64)) {
        return Err(InventoryError::InvalidRequest);
    }
    // The service holds the engine-validated policy, including its existing
    // bounds on key entries, pattern count, and pattern size.
    let description = policy.describe_for_principal(PrincipalId(key));
    let generation = policy_generation(key, &description)?;
    if expected_generation.is_some_and(|expected| expected != generation) {
        return Err(InventoryError::PolicyStale);
    }

    let mut rules = Vec::new();
    for (layer, restrictions) in description.layers.into_iter().enumerate() {
        if let Some(allow) = restrictions.allow_exec {
            if allow.is_empty() {
                rules.push(PolicyRule::AllowEmpty { layer });
            }
            rules.extend(
                allow
                    .into_iter()
                    .map(|pattern| PolicyRule::Allow { layer, pattern }),
            );
        }
        rules.extend(
            restrictions
                .deny_exec
                .into_iter()
                .map(|pattern| PolicyRule::Deny { layer, pattern }),
        );
    }
    page_policy(description.version, generation, rules, wanted)
}

fn policy_generation(key: Account, description: &CommandPolicyDescriptionV1) -> Result<String> {
    let encoded = serde_json::to_vec(description).map_err(|_| InventoryError::PolicyUnavailable)?;
    let mut digest = Sha256::new();
    digest.update(b"marmish/client-inventory/effective-policy/v1\0");
    digest.update(key);
    digest.update(encoded);
    Ok(hex::encode(digest.finalize()))
}

fn page_policy(
    version: u32,
    generation: String,
    rules: Vec<PolicyRule>,
    wanted: usize,
) -> Result<InventoryPolicyPage> {
    let mut pending = rules
        .into_iter()
        .map(|rule| serde_json::to_vec(&rule).map(|bytes| (rule, bytes.len())))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| InventoryError::PolicyUnavailable)?
        .into_iter()
        .peekable();
    let mut selected = None;
    for page in 0..MAX_POLICY_PAGES {
        let mut reply = InventoryPolicyPage::empty(version, generation.clone(), page);
        let final_size = serde_json::to_vec(&reply)
            .map_err(|_| InventoryError::PolicyUnavailable)?
            .len();
        reply.next_page = Some(page + 1);
        let partial_size = serde_json::to_vec(&reply)
            .map_err(|_| InventoryError::PolicyUnavailable)?
            .len();
        let count = pending.len();
        let mut rules_size = 0;
        while let Some((_, size)) = pending.peek() {
            let size = *size + usize::from(!reply.rules.is_empty());
            let envelope = if reply.rules.len() + 1 == count {
                final_size
            } else {
                partial_size
            };
            if envelope + rules_size + size > MAX_POLICY_PAGE_BYTES {
                break;
            }
            let (rule, _) = pending.next().ok_or(InventoryError::PolicyUnavailable)?;
            reply.rules.push(rule);
            rules_size += size;
        }
        let done = pending.peek().is_none();
        if done {
            reply.next_page = None;
        } else if reply.rules.is_empty() {
            return Err(InventoryError::DescriptionTooLarge);
        }
        if page == wanted {
            selected = Some(reply);
        }
        if done {
            return selected.ok_or(InventoryError::InvalidRequest);
        }
    }
    Err(InventoryError::DescriptionTooLarge)
}

impl InventoryPolicyPage {
    fn empty(version: u32, generation: String, page: usize) -> Self {
        Self {
            version,
            generation,
            page,
            next_page: None,
            semantics: PolicySemantics {
                allow: "The canonical executable must match a rule in every present allow layer. An empty allow layer permits no executable.".into(),
                deny: "A match in any deny layer blocks the invoked path or canonical executable.".into(),
                glob: "* matches within a path segment. ** can cross /.".into(),
                resolution: "Unqualified executables are resolved using the server process PATH.".into(),
                caveats: "This is a policy snapshot. Admission also depends on paths, listener state, resource limits and the glob match budget.".into(),
                shell_command_enabled: false,
                interactive_shell_enabled: false,
            },
            rules: Vec::new(),
        }
    }
}

#[cfg(test)]
mod core_tests {
    use super::*;
    use marmish_engine::{KeyRestrictions, Restrictions};
    use serde_json::json;

    fn config(role: &str, keys: &[Account]) -> CommandConfig {
        CommandConfig::from_bytes(
            &serde_json::to_vec(&json!({
                "version": 1,
                "role": role,
                "relays": ["wss://127.0.0.1:9/"],
                "tor": "off",
                "trusted_remotes": [],
                "execution_clients": keys.iter().map(hex::encode).collect::<Vec<_>>(),
                "command_policy": {"version": 1},
                "cwd": "/",
                "limits": {},
                "aliases": {}
            }))
            .unwrap(),
        )
        .unwrap()
    }

    fn journal(config: &CommandConfig) -> (tempfile::TempDir, Journal) {
        let directory = crate::journal::private_test_directory();
        let mut journal = Journal::open(
            &directory.path().join("journal"),
            &[71; 32],
            &hex::encode([1; 32]),
        )
        .unwrap();
        journal
            .reconcile_client_inventory(config.execution_clients())
            .unwrap();
        (directory, journal)
    }

    fn show(value: String) -> InventoryRequest {
        InventoryRequest::Show {
            npub: value,
            policy_page: 0,
            policy_generation: None,
        }
    }

    #[test]
    fn list_has_exclusive_byte_ordered_cursor_and_never_recorded_state() {
        let low = [2; 32];
        let middle = [9; 32];
        let high = [255; 32];
        let config = config("both", &[high, low, middle]);
        let (_directory, journal) = journal(&config);
        let page = |after, limit| {
            let result = query(
                &journal,
                &config,
                None,
                InventoryRequest::List { after, limit },
            )
            .unwrap();
            let InventoryResult::List { clients, next } = result.result else {
                panic!("expected a list response");
            };
            (clients, next)
        };
        let (clients, next) = page(None, 2);
        assert_eq!(clients[0].npub, npub(&low).unwrap());
        assert_eq!(clients[1].npub, npub(&middle).unwrap());
        assert_eq!(next, Some(clients[1].npub.clone()));
        assert_eq!(
            serde_json::to_value(&clients[0].access).unwrap(),
            json!({
                "last_authenticated_submit_at": null,
                "last_engine_admitted_submit_at": null,
                "authenticated_submit_state": "never_recorded",
                "wall_clock_regressed": false
            })
        );
        let (clients, next) = page(next, 64);
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].npub, npub(&high).unwrap());
        assert!(next.is_none());
        assert!(page(Some(npub(&high).unwrap()), 64).0.is_empty());
        assert_eq!(page(Some(npub(&[3; 32]).unwrap()), 64).0.len(), 2);
        for limit in [0, 65] {
            assert_eq!(
                query(
                    &journal,
                    &config,
                    None,
                    InventoryRequest::List { after: None, limit },
                )
                .unwrap_err(),
                InventoryError::InvalidRequest
            );
        }
    }

    #[test]
    fn show_requires_canonical_npub_an_active_configured_key_and_policy() {
        let key = [2; 32];
        let configured = config("server", &[key]);
        let (directory, journal) = journal(&configured);
        let require_error = |config: &CommandConfig, input, error| {
            assert_eq!(
                query(&journal, config, None, show(input)).unwrap_err(),
                error
            );
        };
        let name = npub(&key).unwrap();
        for malformed in [
            name.to_ascii_uppercase(),
            hex::encode(key),
            format!(" {name}"),
        ] {
            require_error(&configured, malformed, InventoryError::InvalidRequest);
        }
        require_error(&configured, name.clone(), InventoryError::PolicyUnavailable);
        require_error(
            &configured,
            npub(&[3; 32]).unwrap(),
            InventoryError::ClientUnknown,
        );
        require_error(
            &config("client", &[key]),
            name.clone(),
            InventoryError::ServerUnavailable,
        );
        let no_clients = config("server", &[]);
        require_error(&no_clients, name.clone(), InventoryError::ClientUnknown);
        let empty = query(
            &journal,
            &no_clients,
            None,
            InventoryRequest::List {
                after: None,
                limit: 32,
            },
        )
        .unwrap();
        assert!(
            matches!(empty.result, InventoryResult::List { clients, next: None } if clients.is_empty())
        );
        let engine_policy = configured
            .engine_policy(
                directory.path().join("engine"),
                "/bin/true".into(),
                [1; 32],
                &[],
            )
            .unwrap()
            .unwrap();
        let result = query(
            &journal,
            &configured,
            engine_policy.command_policy.as_ref(),
            show(name),
        )
        .unwrap();
        let InventoryResult::Show { policy, .. } = result.result else {
            panic!("expected a show response");
        };
        assert_eq!(
            policy.rules,
            [PolicyRule::Allow {
                layer: 0,
                pattern: "/**".into()
            }]
        );
        assert!(!policy.semantics.shell_command_enabled);
        assert!(!policy.semantics.interactive_shell_enabled);
    }

    fn policy() -> CommandPolicy {
        CommandPolicy {
            version: 1,
            defaults: Restrictions {
                allow_exec: Some(vec!["/**".into()]),
                deny_exec: Vec::new(),
            },
            keys: vec![
                KeyRestrictions {
                    principal: PrincipalId([2; 32]),
                    restrictions: Restrictions {
                        allow_exec: Some(Vec::new()),
                        deny_exec: (0..12)
                            .map(|i| format!("/{i}/{}\"", "x".repeat(980)))
                            .collect(),
                    },
                },
                KeyRestrictions {
                    principal: PrincipalId([3; 32]),
                    restrictions: Restrictions {
                        allow_exec: None,
                        deny_exec: vec!["/other-clients-only".into()],
                    },
                },
            ],
        }
    }

    #[test]
    fn scoped_policy_pages_are_lossless_and_generation_uses_only_selected_layers() {
        let key = [2; 32];
        let mut policy = policy();
        policy
            .validate_for_principals(&[PrincipalId(key), PrincipalId([3; 32])])
            .unwrap();
        let first = describe(&policy, key, 0, None).unwrap();
        assert_eq!(first.next_page, Some(1));
        assert_eq!(first.rules[1], PolicyRule::AllowEmpty { layer: 1 });
        let generation = first.generation.clone();
        let mut rules = Vec::new();
        let mut page = first;
        loop {
            let encoded = serde_json::to_vec(&page).unwrap();
            assert!(encoded.len() <= MAX_POLICY_PAGE_BYTES);
            assert!(
                !String::from_utf8(encoded)
                    .unwrap()
                    .contains("other-clients-only")
            );
            rules.extend(page.rules);
            let Some(next) = page.next_page else {
                break;
            };
            page = describe(&policy, key, next, Some(&generation)).unwrap();
        }
        let expected = policy.keys[0]
            .restrictions
            .deny_exec
            .iter()
            .map(|pattern| PolicyRule::Deny {
                layer: 1,
                pattern: pattern.clone(),
            })
            .collect::<Vec<_>>();
        assert_eq!(&rules[2..], expected);
        assert_eq!(
            describe(&policy, key, 1, None).unwrap_err(),
            InventoryError::InvalidRequest
        );
        assert_eq!(
            describe(&policy, key, 63, Some(&generation)).unwrap_err(),
            InventoryError::InvalidRequest
        );
        policy.keys[1]
            .restrictions
            .deny_exec
            .push("/private-new-rule".into());
        assert_eq!(
            describe(&policy, key, 0, None).unwrap().generation,
            generation
        );
        policy.keys[0]
            .restrictions
            .deny_exec
            .push("/new-rule".into());
        assert_eq!(
            describe(&policy, key, 1, Some(&generation)).unwrap_err(),
            InventoryError::PolicyStale
        );
        assert_ne!(
            describe(&policy, [3; 32], 0, None).unwrap().generation,
            generation
        );
    }

    #[test]
    fn accepts_64_policy_pages_and_rejects_overflow_from_the_first_page() {
        let key = [2; 32];
        let pattern = format!("/{}", "\\".repeat(1022));
        let mut policy = CommandPolicy {
            version: 1,
            defaults: Restrictions {
                allow_exec: None,
                deny_exec: vec![pattern.clone(); MAX_POLICY_PAGES * 3],
            },
            keys: Vec::new(),
        };
        policy.validate_for_principals(&[PrincipalId(key)]).unwrap();
        let generation = describe(&policy, key, 0, None).unwrap().generation;
        let last = describe(&policy, key, MAX_POLICY_PAGES - 1, Some(&generation)).unwrap();
        assert_eq!(last.rules.len(), 3);
        assert!(last.next_page.is_none());
        assert!(serde_json::to_vec(&last).unwrap().len() <= MAX_POLICY_PAGE_BYTES);
        policy.defaults.deny_exec.push(pattern);
        assert_eq!(
            describe(&policy, key, 0, None).unwrap_err(),
            InventoryError::DescriptionTooLarge
        );
    }

    #[test]
    fn ipc_vocabulary_rejects_unknown_fields_and_error_categories() {
        assert_eq!(
            InventoryError::from_category("inventory_policy_stale"),
            Some(InventoryError::PolicyStale)
        );
        assert_eq!(InventoryError::from_category("some_internal_error"), None);
        assert!(
            serde_json::from_value::<InventoryRequest>(
                json!({"query":"list","limit":1,"private":"leak"})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<ClientInventoryReply>(json!({"version":1,"result":{"kind":"list","clients":[],"next":null,"private":"leak"}})).is_err());
    }
}

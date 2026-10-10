//! Instance-name claims shared by every hosted provider.
//!
//! One instance name maps to one `meta:<name>` service on a device, so a name
//! can serve only one provider at a time. Every provider claims its name in
//! this one table before starting anything, so two providers can never both
//! pass the check and start under the same name:
//!
//! - Codex and OpenCode hold a [`Claim`] for the duration of their start; after
//!   it their own host table proves ownership (checked by [`reserve`]).
//!   Concurrent starts of the *same* provider share the claim, keeping their
//!   existing reuse behaviour.
//! - ACP holds an exclusive [`Reservation`] from startup until its stop has
//!   finished; a failed or abandoned startup releases it on drop.
//! - A Codex or OpenCode stop holds a claim ([`retiring`]) from before the host
//!   leaves its table until its relay keys and listeners are released.
//!
//! Lock order: the claim table may be held while a provider's host table is
//! read (in [`reserve`]), never the other way round.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use anyhow::{bail, Result};
use once_cell::sync::OnceCell;

/// The provider holding a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Native Codex app-server hosting.
    Codex,
    /// The attached OpenCode HTTP gateway.
    OpenCode,
    /// An owned ACP agent.
    Acp,
}

impl Provider {
    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Acp => "an ACP agent",
        }
    }
}

/// Name → (provider, number of live claims).
fn claims() -> MutexGuard<'static, HashMap<String, (Provider, usize)>> {
    static CLAIMS: OnceCell<Mutex<HashMap<String, (Provider, usize)>>> = OnceCell::new();
    CLAIMS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn taken(name: &str, by: Provider) -> anyhow::Error {
    anyhow::anyhow!(
        "`{name}` is already hosting {} on this device; choose another name",
        by.label()
    )
}

fn unclaim(name: &str) {
    let mut held = claims();
    if let Some((_, count)) = held.get_mut(name) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            held.remove(name);
        }
    }
}

/// A Codex or OpenCode startup claim; released on drop.
#[must_use = "dropping a claim releases the name"]
pub struct Claim {
    name: String,
}

impl Drop for Claim {
    fn drop(&mut self) {
        unclaim(&self.name);
    }
}

/// Claim `name` for a Codex or OpenCode startup. Fails while another
/// provider claims it; the caller still checks the other providers' running
/// hosts as before.
pub fn claim(name: &str, provider: Provider) -> Result<Claim> {
    if provider == Provider::Acp {
        bail!("ACP hosting reserves its name with `reserve`");
    }
    let mut held = claims();
    match held.get_mut(name) {
        Some((owner, _)) if *owner != provider => return Err(taken(name, *owner)),
        Some((_, count)) => *count += 1,
        None => {
            held.insert(name.to_string(), (provider, 1));
        },
    }
    Ok(Claim {
        name: name.to_string(),
    })
}

/// Hold `name` for `provider` while one of its hosts is being retired: a
/// stop removes the host from its table first, but its relay keys and
/// listeners are released only afterwards, and no other provider may claim
/// the name in between. `None` when another provider holds the name (then
/// `provider` was not hosting it). Take it **before** removing the host and
/// never while holding a provider's host table.
pub fn retire(name: &str, provider: Provider) -> Option<Claim> {
    claim(name, provider).ok()
}

/// Run `cleanup` (removal and teardown of `provider`'s host `name`) with the
/// retirement claim held.
pub fn retiring<T>(name: &str, provider: Provider, cleanup: impl FnOnce() -> T) -> T {
    let _claim = retire(name, provider);
    cleanup()
}

/// An ACP name; released on drop unless [`Reservation::commit`]ted.
#[must_use = "dropping a reservation releases the name"]
pub struct Reservation {
    name: String,
    committed: bool,
}

impl Reservation {
    /// Keep the name reserved after this value is dropped; [`release`] frees
    /// it when the host stops.
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.committed {
            unclaim(&self.name);
        }
    }
}

/// Reserve `name` for an ACP host unless any provider claims or hosts it.
/// The running-host checks happen under the claim lock, so a Codex or
/// OpenCode start that already finished is seen, and one still starting
/// holds a claim.
pub fn reserve(name: &str) -> Result<Reservation> {
    let mut held = claims();
    if let Some((owner, _)) = held.get(name) {
        return Err(taken(name, *owner));
    }
    if super::serve::is_hosting_codex(name) {
        return Err(taken(name, Provider::Codex));
    }
    if super::serve_opencode::is_hosting(name) {
        return Err(taken(name, Provider::OpenCode));
    }
    held.insert(name.to_string(), (Provider::Acp, 1));
    Ok(Reservation {
        name: name.to_string(),
        committed: false,
    })
}

/// Free a committed ACP name when its host stops.
pub fn release(name: &str) {
    let mut held = claims();
    if held
        .get(name)
        .is_some_and(|(owner, _)| *owner == Provider::Acp)
    {
        held.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_reserved(name: &str) -> bool {
        claims()
            .get(name)
            .is_some_and(|(owner, _)| *owner == Provider::Acp)
    }

    #[test]
    fn a_reservation_is_exclusive_and_released_unless_committed() {
        let name = "hosting-test-unique-name";
        let first = reserve(name).expect("free");
        assert!(reserve(name).is_err());
        drop(first);
        assert!(!is_reserved(name));
        reserve(name).expect("free again").commit();
        assert!(is_reserved(name));
        assert!(claim(name, Provider::Codex).is_err(), "a committed ACP name stays reserved");
        release(name);
        assert!(!is_reserved(name));
    }

    #[test]
    fn providers_exclude_each_other_but_share_their_own_claims() {
        let name = "hosting-test-cross-provider";
        let codex = claim(name, Provider::Codex).expect("free");
        let again = claim(name, Provider::Codex).expect("same provider shares the claim");
        assert!(claim(name, Provider::OpenCode).is_err());
        assert!(reserve(name).is_err(), "an ACP host cannot start under a starting Codex host");
        drop(codex);
        assert!(reserve(name).is_err(), "the claim lives until the last starter is done");
        drop(again);
        let acp = reserve(name).expect("free");
        assert!(claim(name, Provider::Codex).is_err());
        assert!(claim(name, Provider::OpenCode).is_err());
        drop(acp);
        assert!(claim(name, Provider::OpenCode).is_ok());
    }

    #[test]
    fn a_retiring_host_keeps_its_name_until_cleanup_finishes() {
        use std::sync::{Arc, Barrier};

        let name = "hosting-test-retiring";
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let stopper = std::thread::spawn({
            let (entered, release) = (entered.clone(), release.clone());
            move || {
                retiring(name, Provider::Codex, || {
                    // Relay withdrawal and listener cleanup are in progress.
                    entered.wait();
                    release.wait();
                });
            }
        });
        entered.wait();
        assert!(reserve(name).is_err(), "an ACP host cannot take the name mid-retirement");
        assert!(claim(name, Provider::OpenCode).is_err(), "nor can OpenCode");
        let same = claim(name, Provider::Codex);
        assert!(same.is_ok(), "a Codex re-host still reuses the name");
        drop(same);
        release.wait();
        stopper.join().expect("stopper");
        let acp = reserve(name).expect("free once cleanup finished");
        drop(acp);
    }
}

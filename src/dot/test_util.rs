//! Test utilities shared across `dot` test modules.
//!
//! Only compiled under `#[cfg(test)]`. Keeps helpers out of the production
//! build while letting any test in the `dot` tree reuse them.

#![cfg(test)]

use std::ffi::{OsStr, OsString};

/// RAII guard that overrides a process-wide environment variable for the
/// lifetime of the guard and restores the previous value on `Drop` — even
/// across panics.
///
/// Tests using this helper MUST be marked `#[serial]` because they mutate
/// process-global state. The guard removes the variable if it was previously
/// unset, or restores its old value otherwise.
///
/// This replaces the manual `match prev { Some(v) => set_var, None => remove_var }`
/// pattern, which leaks state into later tests if anything panics between
/// the `set_var` and the manual restore.
pub struct EnvGuard {
    key: OsString,
    prev: Option<OsString>,
}

impl EnvGuard {
    /// Set `key` to `value` for the lifetime of the returned guard.
    pub fn set<K, V>(key: K, value: V) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let key_os = key.as_ref().to_os_string();
        let prev = std::env::var_os(&key_os);
        // SAFETY: setting environment variables is process-global. Tests using
        // this helper are required to be `#[serial]`.
        unsafe {
            std::env::set_var(&key_os, value.as_ref());
        }
        EnvGuard { key: key_os, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: see `EnvGuard::set`.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}

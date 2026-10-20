//! Passwords in the system keychain: the macOS Keychain, or on Linux the
//! Secret Service (GNOME Keyring, KWallet) through `secret-tool`.
//! Each password is stored under an account name, e.g. a bottle id.

use crate::{Error, Result};

/// Keychain service the passwords are filed under.
const SERVICE: &str = "com.praevit.vinter.domain";

/// Stores (or replaces) the password for `account`.
pub fn set(account: &str, label: &str, password: &str) -> Result<()> {
    imp::set(account, label, password)
}

/// The password for `account`, if one is stored.
pub fn get(account: &str) -> Result<Option<String>> {
    imp::get(account)
}

/// Removes the password for `account`. Removing a missing one is fine.
pub fn delete(account: &str) -> Result<()> {
    imp::delete(account)
}

#[cfg(target_os = "macos")]
mod imp {
    use security_framework::passwords;

    use super::*;

    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    pub fn set(account: &str, _label: &str, password: &str) -> Result<()> {
        passwords::set_generic_password(SERVICE, account, password.as_bytes())
            .map_err(|e| Error::Keychain(e.to_string()))
    }

    pub fn get(account: &str) -> Result<Option<String>> {
        match passwords::get_generic_password(SERVICE, account) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(Error::Keychain(e.to_string())),
        }
    }

    pub fn delete(account: &str) -> Result<()> {
        match passwords::delete_generic_password(SERVICE, account) {
            Err(e) if e.code() != NOT_FOUND => Err(Error::Keychain(e.to_string())),
            _ => Ok(()),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::*;

    fn secret_tool(args: &[&str], input: Option<&str>) -> Result<std::process::Output> {
        let mut child = Command::new("secret-tool")
            .args(args)
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::Keychain(format!(
                    "could not run secret-tool ({e}); install libsecret-tools or your distribution's equivalent"
                ))
            })?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| Error::Keychain(e.to_string()))?;
        }
        child
            .wait_with_output()
            .map_err(|e| Error::Keychain(e.to_string()))
    }

    pub fn set(account: &str, label: &str, password: &str) -> Result<()> {
        let label = format!("--label={label}");
        let out = secret_tool(
            &["store", &label, "service", SERVICE, "account", account],
            Some(password),
        )?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Error::Keychain(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }

    pub fn get(account: &str) -> Result<Option<String>> {
        let out = secret_tool(&["lookup", "service", SERVICE, "account", account], None)?;
        // `lookup` exits with 1 and prints nothing when there is no such password.
        Ok(out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    pub fn delete(account: &str) -> Result<()> {
        secret_tool(&["clear", "service", SERVICE, "account", account], None)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// Uses the real keychain, so it only runs on request:
    /// `cargo test -p vinter-core secret -- --ignored`.
    #[test]
    #[ignore]
    fn round_trip() {
        let account = format!("vinter-test-{}", uuid::Uuid::new_v4());
        assert_eq!(super::get(&account).unwrap(), None);
        super::set(&account, "Vinter test", "pa55 wörd").unwrap();
        super::set(&account, "Vinter test", "changed").unwrap();
        assert_eq!(super::get(&account).unwrap().as_deref(), Some("changed"));
        super::delete(&account).unwrap();
        assert_eq!(super::get(&account).unwrap(), None);
        super::delete(&account).unwrap();
    }
}

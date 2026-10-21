//! Domain accounts: running a bottle's programs as a Windows domain user.
//!
//! Wine cannot join a domain (no machine account, Group Policy or logon
//! scripts). What a bottle gets instead:
//! - Identity: every Wine takes the Windows user name from `USER`. Vinter
//!   Wine's domain patch also reports the domain (and, if set, a computer
//!   name) from `WINEDOMAIN`, `WINEDNSDOMAIN` and `WINECOMPUTERNAME`, so
//!   programs see a workstation joined to it.
//! - Sign-in: with a runtime built with Kerberos, Vinter gets a ticket with
//!   `kinit` before programs start, into a ticket cache only this bottle
//!   uses. Wine's Kerberos and Negotiate support sign in to domain services
//!   (SQL Server, IIS, file servers) with it.
//!
//! The password lives in the system keychain under the bottle's id.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::{Bottle, DomainAccount, PREFIX_DIR, dir, get};
use crate::error::IoResultExt;
use crate::process::{self, logs};
use crate::runtime::Runtime;
use crate::{AppPaths, Error, Result, secret};

/// Folder in a bottle with its `krb5.conf` and ticket cache.
pub const KERBEROS_DIR: &str = "kerberos";
const CONFIG_FILE: &str = "krb5.conf";
const CACHE_FILE: &str = "ccache";
/// Active Directory tickets last 10 hours by default; get a new one well before.
const TICKET_REFRESH: Duration = Duration::from_secs(8 * 60 * 60);
const KINIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Checks an account and returns it in its usual form: domain and computer
/// name in upper case, DNS names in lower case, surrounding spaces removed.
pub fn normalize(account: &DomainAccount) -> Result<DomainAccount> {
    let invalid = |what: &str, rule: &str| Err(Error::InvalidSetting(format!("{what}: {rule}")));

    let user = account.user.trim();
    // Windows' rules for user names (at most 20 characters, none of these);
    // it also names a folder and a Unix environment variable.
    let user_ok = !user.is_empty()
        && user.chars().count() <= 20
        && user != "."
        && user != ".."
        && !user
            .chars()
            .any(|c| c.is_control() || r#""/\[]:;|=,+*?<>@"#.contains(c));
    if !user_ok {
        return invalid(
            "user name",
            r#"use 1 to 20 characters, without " / \ [ ] : ; | = , + * ? < > @"#,
        );
    }

    let netbios_ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 15
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    };
    let domain = account.domain.trim().to_ascii_uppercase();
    if !netbios_ok(&domain) {
        return invalid(
            "domain",
            "use its short (NetBIOS) name, e.g. CORP: up to 15 letters, digits, - or _",
        );
    }
    let computer_name = account.computer_name.trim().to_ascii_uppercase();
    if !computer_name.is_empty() && !netbios_ok(&computer_name) {
        return invalid("computer name", "use up to 15 letters, digits, - or _");
    }

    let dns_domain = account
        .dns_domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !is_dns_name(&dns_domain) || !dns_domain.contains('.') {
        return invalid("DNS domain", "use the full name, e.g. corp.example.com");
    }

    let kdc = account.kdc.trim().to_ascii_lowercase();
    if !kdc.is_empty() {
        let (host, port) = kdc.split_once(':').unwrap_or((&kdc, "1"));
        if !is_dns_name(host) || port.parse::<u16>().map_or(true, |p| p == 0) {
            return invalid(
                "domain controller",
                "use a host name or address, optionally with :port",
            );
        }
    }

    Ok(DomainAccount {
        user: user.to_string(),
        domain,
        dns_domain,
        computer_name,
        kdc,
    })
}

/// Host names and IPv4 addresses: dot-separated labels of letters, digits and `-`.
fn is_dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// The Kerberos realm: the DNS domain in upper case.
pub fn realm(account: &DomainAccount) -> String {
    account.dns_domain.to_ascii_uppercase()
}

/// `user@REALM`.
pub fn principal(account: &DomainAccount) -> String {
    format!("{}@{}", account.user, realm(account))
}

/// Environment for Wine processes in a bottle (in folder `dir`) with this account.
pub fn env(dir: &Path, account: &DomainAccount) -> Vec<(String, String)> {
    let kerberos = dir.join(KERBEROS_DIR);
    let mut env = vec![
        ("USER".to_string(), account.user.clone()),
        ("WINEDOMAIN".to_string(), account.domain.clone()),
        ("WINEDNSDOMAIN".to_string(), account.dns_domain.clone()),
        (
            "KRB5CCNAME".to_string(),
            format!("FILE:{}", kerberos.join(CACHE_FILE).display()),
        ),
        // The system's settings (if any) after the bottle's, which win.
        (
            "KRB5_CONFIG".to_string(),
            format!("{}:/etc/krb5.conf", kerberos.join(CONFIG_FILE).display()),
        ),
    ];
    if !account.computer_name.is_empty() {
        env.push(("WINECOMPUTERNAME".into(), account.computer_name.clone()));
    }
    env
}

/// The bottle's `krb5.conf` (in folder `kerberos`). Without a domain
/// controller set, Kerberos finds one through the domain's DNS records, as
/// Windows does. Plugins are looked for in an empty folder: MIT Kerberos
/// would otherwise load optional ones (smart cards, SPAKE) from where it was
/// built, which on a Mac with Homebrew's krb5 brings in a second copy of it.
pub fn krb5_conf(account: &DomainAccount, kerberos: &Path) -> String {
    let realm = realm(account);
    let plugins = kerberos.join("plugins");
    let plugins = plugins.display();
    let dns = &account.dns_domain;
    let kdc = if account.kdc.is_empty() {
        String::new()
    } else {
        format!(
            "        kdc = {kdc}\n        admin_server = {host}\n",
            kdc = account.kdc,
            host = account.kdc.split(':').next().unwrap_or(&account.kdc)
        )
    };
    format!(
        "# Written by Vinter for this bottle's domain account.\n\
         [libdefaults]\n\
         \x20   default_realm = {realm}\n\
         \x20   dns_lookup_kdc = true\n\
         \x20   dns_lookup_realm = false\n\
         \x20   rdns = false\n\
         \x20   forwardable = true\n\
         \x20   plugin_base_dir = {plugins}\n\
         \n\
         [realms]\n\
         \x20   {realm} = {{\n\
         {kdc}\
         \x20   }}\n\
         \n\
         [domain_realm]\n\
         \x20   .{dns} = {realm}\n\
         \x20   {dns} = {realm}\n"
    )
}

/// The Windows user name programs in the bottle see.
pub fn windows_user(account: Option<&DomainAccount>) -> String {
    match account {
        Some(a) => a.user.clone(),
        None => host_user(),
    }
}

/// The user name Wine uses without an account: `$USER`, else the login name.
fn host_user() -> String {
    let name = std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .or_else(login_name)
        .unwrap_or_else(|| "wine".into());
    // Wine keeps only the part after the last slash or backslash.
    name.rsplit(['/', '\\']).next().unwrap_or(&name).to_string()
}

#[cfg(unix)]
fn login_name() -> Option<String> {
    // SAFETY: getpwuid returns NULL or a pointer to a static entry, read at once.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_name.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*pw).pw_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(not(unix))]
fn login_name() -> Option<String> {
    None
}

/// Moves the Windows profile (`C:\users\<from>`) to the new user name, so
/// installed programs keep their settings, and leaves a link at the old name
/// for paths programs saved. Leaves both alone if the new one already exists.
pub fn move_profile(prefix: &Path, from: &str, to: &str) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let users = prefix.join("drive_c").join("users");
    let old = users.join(from);
    let new = users.join(to);
    // Not a real folder (missing, or the link from an earlier move): nothing to move.
    if !std::fs::symlink_metadata(&old).is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    match std::fs::symlink_metadata(&new) {
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(&new).at(&new)?,
        Ok(_) => return Ok(()),
        Err(_) => {}
    }
    std::fs::rename(&old, &new).at(&old)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(to, &old).at(&old)?;
    Ok(())
}

/// Sets, changes or (with `None`) removes a bottle's domain account.
/// `password`: `Some` stores a new one in the keychain, `None` keeps the
/// stored one. Close the bottle's programs first: the profile folder moves
/// when the user name changes, and programs see the change when they start.
pub fn set(
    paths: &AppPaths,
    id: &str,
    account: Option<DomainAccount>,
    password: Option<String>,
) -> Result<Bottle> {
    let mut bottle = get(paths, id)?;
    let dir = dir(paths, id)?;
    let account = account.as_ref().map(normalize).transpose()?;
    if password.as_deref().is_some_and(|p| p.contains('\0')) {
        return Err(Error::InvalidSetting(
            "the password contains a NUL byte".into(),
        ));
    }

    move_profile(
        &dir.join(PREFIX_DIR),
        &windows_user(bottle.domain.as_ref()),
        &windows_user(account.as_ref()),
    )?;

    let kerberos = dir.join(KERBEROS_DIR);
    match &account {
        None => {
            if bottle.domain.is_some() {
                secret::delete(id)?;
            }
            if kerberos.exists() {
                std::fs::remove_dir_all(&kerberos).at(&kerberos)?;
            }
        }
        Some(account) => {
            std::fs::create_dir_all(&kerberos).at(&kerberos)?;
            let conf = kerberos.join(CONFIG_FILE);
            std::fs::write(&conf, krb5_conf(account, &kerberos)).at(&conf)?;
            if let Some(password) = password.as_deref().filter(|p| !p.is_empty()) {
                let label = format!("Vinter: {} ({})", bottle.name, principal(account));
                secret::set(id, &label, password)?;
            }
            // A ticket for other credentials must not be used again.
            if bottle.domain.as_ref() != Some(account) || password.is_some() {
                let _ = std::fs::remove_file(kerberos.join(CACHE_FILE));
            }
        }
    }

    bottle.domain = account;
    bottle.save(&dir)?;
    Ok(bottle)
}

/// True if a password for the bottle's account is in the keychain.
pub fn has_password(id: &str) -> Result<bool> {
    Ok(secret::get(id)?.is_some())
}

/// Copies the stored password to a duplicated bottle, if there is one.
pub fn copy_password(from_id: &str, to_id: &str, label: &str) -> Result<()> {
    if let Some(password) = secret::get(from_id)? {
        secret::set(to_id, label, &password)?;
    }
    Ok(())
}

/// Signs in now: gets a new Kerberos ticket with the stored password.
pub fn sign_in(paths: &AppPaths, id: &str) -> Result<()> {
    let bottle = get(paths, id)?;
    let account = bottle
        .domain
        .as_ref()
        .ok_or_else(|| Error::DomainSignIn("this bottle has no domain account".into()))?;
    let rt = super::runtime_for(paths, &bottle)?;
    let dir = dir(paths, id)?;
    let password = secret::get(id)?
        .ok_or_else(|| Error::DomainSignIn("no password is saved for this account".into()))?;
    let (log, _) = logs::create(&dir, "kerberos")?;
    kinit(&rt, &dir, account, &password, &log)
}

/// Before a program starts: gets a ticket if the bottle has an account with
/// a password, the runtime supports Kerberos and the last ticket is getting
/// old. Problems are written to `log` and do not stop the program, which
/// may still work without one (or ask for a password itself).
pub fn refresh_ticket(rt: &Runtime, dir: &Path, bottle: &Bottle, log: &Path) {
    let Some(account) = &bottle.domain else {
        return;
    };
    if !rt.kerberos || !ticket_is_old(dir) {
        return;
    }
    let result = secret::get(&bottle.id).and_then(|password| match password {
        Some(password) => kinit(rt, dir, account, &password, log),
        None => Ok(()),
    });
    if let Err(e) = result {
        note(log, &format!("Vinter: {e}"));
    }
}

fn ticket_is_old(dir: &Path) -> bool {
    std::fs::metadata(dir.join(KERBEROS_DIR).join(CACHE_FILE))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age > TICKET_REFRESH)
}

fn note(log: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::File::options().create(true).append(true).open(log) {
        let _ = writeln!(f, "{line}");
    }
}

/// Runs `kinit` for the account, with the password on its standard input.
fn kinit(
    rt: &Runtime,
    dir: &Path,
    account: &DomainAccount,
    password: &str,
    log: &Path,
) -> Result<()> {
    let kerberos = dir.join(KERBEROS_DIR);
    std::fs::create_dir_all(&kerberos).at(&kerberos)?;
    let conf = kerberos.join(CONFIG_FILE);
    std::fs::write(&conf, krb5_conf(account, &kerberos)).at(&conf)?;

    let (program, heimdal) = kinit_program(rt);
    let mut cmd = Command::new(&program);
    cmd.envs(
        env(dir, account)
            .into_iter()
            .filter(|(k, _)| k.starts_with("KRB5")),
    );
    if heimdal {
        // macOS's own kinit only reads a password from a terminal unless told.
        cmd.arg("--password-file=STDIN");
    }
    cmd.arg(principal(account));
    note(
        log,
        &format!("Vinter: signing in as {}", principal(account)),
    );
    process::run_with_input(
        cmd,
        format!("{password}\n").as_bytes(),
        log,
        "kinit",
        KINIT_TIMEOUT,
    )
    .map_err(|e| match e {
        Error::ProcessFailed { message, .. } => Error::DomainSignIn(sign_in_hint(&message)),
        other => other,
    })
}

/// Vinter Wine bundles MIT Kerberos' kinit. Other runtimes use the system's:
/// macOS has Heimdal's, Linux distributions usually MIT's.
fn kinit_program(rt: &Runtime) -> (PathBuf, bool) {
    let bundled = rt.bin_dir().join("kinit");
    if bundled.is_file() {
        (bundled, false)
    } else if cfg!(target_os = "macos") {
        (PathBuf::from("/usr/bin/kinit"), true)
    } else {
        (PathBuf::from("kinit"), false)
    }
}

/// Turns kinit's output into something a person can act on, keeping it below.
fn sign_in_hint(output: &str) -> String {
    let lower = output.to_ascii_lowercase();
    let hint = if lower.contains("password incorrect") || lower.contains("preauthentication failed")
    {
        "the password is wrong"
    } else if lower.contains("not found in kerberos database") || lower.contains("client not found")
    {
        "the domain does not know this user name"
    } else if lower.contains("cannot find kdc")
        || lower.contains("cannot contact any kdc")
        || lower.contains("unable to reach")
    {
        "no domain controller could be reached; check the network or VPN, or set the domain controller"
    } else if lower.contains("password has expired") || lower.contains("password expired") {
        "the password has expired; change it on a Windows computer first"
    } else if lower.contains("revoked") || lower.contains("locked") {
        "the account is disabled or locked"
    } else if lower.contains("no such file") || lower.contains("could not run") {
        "kinit was not found; install Kerberos (krb5) or use a Vinter Wine runtime"
    } else {
        return output.trim().to_string();
    };
    format!("{hint}\n{}", output.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> DomainAccount {
        DomainAccount {
            user: " jdoe ".into(),
            domain: "corp".into(),
            dns_domain: "Corp.Example.com.".into(),
            computer_name: "".into(),
            kdc: "".into(),
        }
    }

    #[test]
    fn normalizes_names() {
        let a = normalize(&account()).unwrap();
        assert_eq!(a.user, "jdoe");
        assert_eq!(a.domain, "CORP");
        assert_eq!(a.dns_domain, "corp.example.com");
        assert_eq!(principal(&a), "jdoe@CORP.EXAMPLE.COM");

        let mut b = account();
        b.computer_name = "ws-042".into();
        b.kdc = "DC01.corp.example.com:88".into();
        let b = normalize(&b).unwrap();
        assert_eq!(b.computer_name, "WS-042");
        assert_eq!(b.kdc, "dc01.corp.example.com:88");
    }

    #[test]
    fn rejects_bad_names() {
        let with = |f: fn(&mut DomainAccount)| {
            let mut a = account();
            f(&mut a);
            normalize(&a)
        };
        assert!(with(|a| a.user = "".into()).is_err());
        assert!(with(|a| a.user = "j/doe".into()).is_err());
        assert!(with(|a| a.user = "jdoe@corp".into()).is_err());
        assert!(with(|a| a.user = "..".into()).is_err());
        assert!(with(|a| a.user = "a".repeat(21)).is_err());
        assert!(with(|a| a.domain = "corp.example.com".into()).is_err());
        assert!(with(|a| a.domain = "A".repeat(16)).is_err());
        assert!(with(|a| a.dns_domain = "corp".into()).is_err());
        assert!(with(|a| a.dns_domain = "corp..com".into()).is_err());
        assert!(with(|a| a.computer_name = "my pc".into()).is_err());
        assert!(with(|a| a.kdc = "dc01:0".into()).is_err());
        assert!(with(|a| a.kdc = "dc 01".into()).is_err());
    }

    #[test]
    fn environment() {
        let a = normalize(&account()).unwrap();
        let env = env(Path::new("/b"), &a);
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("USER"), Some("jdoe"));
        assert_eq!(get("WINEDOMAIN"), Some("CORP"));
        assert_eq!(get("WINEDNSDOMAIN"), Some("corp.example.com"));
        assert_eq!(get("KRB5CCNAME"), Some("FILE:/b/kerberos/ccache"));
        assert_eq!(
            get("KRB5_CONFIG"),
            Some("/b/kerberos/krb5.conf:/etc/krb5.conf")
        );
        assert_eq!(get("WINECOMPUTERNAME"), None);
    }

    #[test]
    fn krb5_conf_names_the_realm() {
        let mut a = normalize(&account()).unwrap();
        let conf = krb5_conf(&a, Path::new("/b/kerberos"));
        assert!(
            conf.contains("default_realm = CORP.EXAMPLE.COM\n"),
            "{conf}"
        );
        assert!(
            conf.contains("    .corp.example.com = CORP.EXAMPLE.COM\n"),
            "{conf}"
        );
        assert!(!conf.contains(" kdc ="), "{conf}");
        assert!(
            conf.contains("plugin_base_dir = /b/kerberos/plugins\n"),
            "{conf}"
        );
        a.kdc = "dc01.corp.example.com:88".into();
        let conf = krb5_conf(&a, Path::new("/b/kerberos"));
        assert!(
            conf.contains("        kdc = dc01.corp.example.com:88\n"),
            "{conf}"
        );
        assert!(
            conf.contains("        admin_server = dc01.corp.example.com\n"),
            "{conf}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn profile_moves_and_moves_back() {
        let tmp = tempfile::tempdir().unwrap();
        let users = tmp.path().join("drive_c/users");
        std::fs::create_dir_all(users.join("me/AppData")).unwrap();

        move_profile(tmp.path(), "me", "jdoe").unwrap();
        assert!(users.join("jdoe/AppData").is_dir());
        assert_eq!(
            std::fs::read_link(users.join("me")).unwrap(),
            Path::new("jdoe")
        );
        // Saved paths through the old name still work.
        assert!(users.join("me/AppData").is_dir());

        move_profile(tmp.path(), "jdoe", "me").unwrap();
        assert!(
            std::fs::symlink_metadata(users.join("me"))
                .unwrap()
                .is_dir()
        );
        assert_eq!(
            std::fs::read_link(users.join("jdoe")).unwrap(),
            Path::new("me")
        );

        // An existing profile under the new name is kept as it is.
        std::fs::create_dir_all(users.join("other")).unwrap();
        move_profile(tmp.path(), "me", "other").unwrap();
        assert!(
            std::fs::symlink_metadata(users.join("me"))
                .unwrap()
                .is_dir()
        );
        // Nothing to move is fine.
        move_profile(tmp.path(), "nobody", "someone").unwrap();
    }

    #[test]
    fn hints_for_common_failures() {
        assert!(
            sign_in_hint("kinit: Password incorrect while getting initial credentials")
                .starts_with("the password is wrong\n")
        );
        assert!(
            sign_in_hint("kinit: Cannot find KDC for realm \"CORP.EXAMPLE.COM\"")
                .starts_with("no domain controller")
        );
        assert_eq!(sign_in_hint("  something else \n"), "something else");
    }
}

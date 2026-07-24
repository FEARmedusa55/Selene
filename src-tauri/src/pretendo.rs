//! Pretendo Network support for Cemu.
//!
//! This app does **not** reimplement Pretendo — Cemu has built-in support. What
//! lives here is configuration and, more importantly, honest reporting of what
//! is missing, because the failure mode otherwise is a game that connects to
//! nothing with no explanation.
//!
//! Online play needs files dumped from a Wii U the user owns. Nothing here
//! downloads, bundles, or generates them:
//!
//! ```text
//! <Cemu>/otp.bin
//! <Cemu>/seeprom.bin
//! <Cemu>/mlc01/usr/save/system/act/<8-hex-id>/account.dat
//! <Cemu>/mlc01/sys/title/0005001b/10054000/content/ccerts/*.cert
//! <Cemu>/mlc01/sys/title/0005001b/10054000/content/scerts/*.cert
//! ```
//!
//! Cemu records the choice of network in `settings.xml`:
//!
//! ```xml
//! <Account>
//!     <PersistentId>2147483651</PersistentId>
//!     <OnlineEnabled>false</OnlineEnabled>
//!     <ActiveService>0</ActiveService>
//! </Account>
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Which network Cemu talks to. Values are Cemu's own encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkService {
    Nintendo,
    Pretendo,
    Custom,
}

impl NetworkService {
    pub fn from_code(v: u32) -> Self {
        match v {
            1 => Self::Pretendo,
            2 => Self::Custom,
            _ => Self::Nintendo,
        }
    }
    pub fn code(self) -> u32 {
        match self {
            Self::Nintendo => 0,
            Self::Pretendo => 1,
            Self::Custom => 2,
        }
    }
}

/// One file or directory Cemu needs for online play.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredFile {
    pub label: String,
    /// Path relative to the Cemu data directory, for showing the user where it
    /// belongs when it is missing.
    pub relative_path: String,
    pub present: bool,
    /// For directories: how many files were found.
    pub count: usize,
    pub detail: String,
}

/// A Cemu account, as read from `account.dat`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Directory name, e.g. "80000003".
    pub persistent_id: String,
    pub mii_name: String,
    /// The NNID/PNID username. Empty when the account is local-only.
    pub account_id: String,
    /// Cemu's numeric network id; 0 when unlinked.
    pub principal_id: u64,
    pub is_active: bool,
}

impl Account {
    /// An account is only usable online once it carries a network id.
    pub fn is_linked(&self) -> bool {
        !self.account_id.trim().is_empty() && self.principal_id != 0
    }
}

/// Everything the Pretendo tab needs to render.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub cemu_configured: bool,
    pub data_dir: String,
    pub service: NetworkService,
    pub online_enabled: bool,
    pub accounts: Vec<Account>,
    pub files: Vec<RequiredFile>,
    /// True once every required file is present.
    pub files_complete: bool,
    /// Cemu holds settings.xml open and rewrites it on exit.
    pub cemu_running: bool,
}

fn utf16_ish_to_string(raw: &str) -> String {
    raw.trim().to_string()
}

/// Parse Cemu's `account.dat`, a plain `key=value` file.
pub fn parse_account(text: &str, persistent_id: &str) -> Account {
    let mut mii_name = String::new();
    let mut account_id = String::new();
    let mut principal_id = 0u64;

    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "MiiName" => mii_name = utf16_ish_to_string(v),
            "AccountId" => account_id = utf16_ish_to_string(v),
            // Cemu stores PrincipalId as hex ("578bc6af"), not decimal. Parsing
            // it as decimal fails and falls back to 0, which would make a real
            // linked account read as unlinked. Only the actual dump revealed
            // this -- the synthesised test value happened to be all digits.
            "PrincipalId" => {
                let raw = v.trim().trim_start_matches("0x");
                principal_id = u64::from_str_radix(raw, 16).unwrap_or(0);
            }
            _ => {}
        }
    }

    Account {
        persistent_id: persistent_id.to_string(),
        mii_name,
        account_id,
        principal_id,
        is_active: false,
    }
}

/// `MiiName` is stored as hex-encoded UTF-16. Decode it for display, falling
/// back to the raw value when it does not look like hex.
fn decode_mii_name(raw: &str) -> String {
    let raw = raw.trim();
    if raw.len() < 4 || raw.len() % 4 != 0 || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return raw.to_string();
    }
    let units: Vec<u16> = raw
        .as_bytes()
        .chunks(4)
        .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16(&units).unwrap_or_else(|_| raw.to_string())
}

pub fn list_accounts(cemu_data_dir: &Path) -> Vec<Account> {
    let act = cemu_data_dir
        .join("mlc01")
        .join("usr")
        .join("save")
        .join("system")
        .join("act");

    let Ok(entries) = std::fs::read_dir(&act) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        // Account directories are 8 hex digits starting 8.
        if id.len() != 8 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path.join("account.dat")) else {
            continue;
        };
        let mut acc = parse_account(&text, &id);
        acc.mii_name = decode_mii_name(&acc.mii_name);
        out.push(acc);
    }
    out.sort_by(|a, b| a.persistent_id.cmp(&b.persistent_id));
    out
}

/// Count certificate files in `dir`.
///
/// Real dumps store these as `.der` (X.509 certs) and `.aes` (encrypted keys),
/// e.g. `WIIU_ACCOUNT_1_CERT.der`, `WIIU_ACCOUNT_1_RSA_KEY.aes` — *not* `.cert`,
/// which an earlier version wrongly required and which would have reported an
/// installed set as empty. The ccerts/scerts directories hold only certificate
/// material, so any regular file counts.
fn count_certs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

/// Check every file Cemu needs for online play.
pub fn check_files(cemu_data_dir: &Path) -> Vec<RequiredFile> {
    let mut out = Vec::new();

    for (label, rel) in [
        ("Console OTP dump", "otp.bin"),
        ("SEEPROM dump", "seeprom.bin"),
    ] {
        let p = cemu_data_dir.join(rel);
        let size = p.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(RequiredFile {
            label: label.into(),
            relative_path: rel.into(),
            present: p.is_file(),
            count: 0,
            detail: if p.is_file() {
                format!("{size} bytes")
            } else {
                "Dump from your console".into()
            },
        });
    }

    let accounts = list_accounts(cemu_data_dir);
    let linked = accounts.iter().filter(|a| a.is_linked()).count();
    out.push(RequiredFile {
        label: "Account linked to a network ID".into(),
        relative_path: "mlc01/usr/save/system/act/<id>/account.dat".into(),
        present: linked > 0,
        count: accounts.len(),
        detail: if accounts.is_empty() {
            "No accounts found".into()
        } else if linked > 0 {
            format!("{linked} of {} linked", accounts.len())
        } else {
            format!("{} account(s), none linked to a PNID", accounts.len())
        },
    });

    // Certificates live under the account-service title.
    let certs_base = cemu_data_dir
        .join("mlc01/sys/title/0005001b/10054000/content");
    for (label, sub) in [("Client certificates", "ccerts"), ("Server certificates", "scerts")] {
        let dir = certs_base.join(sub);
        let n = count_certs(&dir);
        out.push(RequiredFile {
            label: label.into(),
            relative_path: format!("mlc01/sys/title/0005001b/10054000/content/{sub}/"),
            present: n > 0,
            count: n,
            detail: if n > 0 {
                format!("{n} certificate(s)")
            } else {
                "Dump from your console".into()
            },
        });
    }

    out
}

// --- settings.xml -----------------------------------------------------------

fn element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let a = xml.find(&open)? + open.len();
    let b = xml[a..].find(&close)? + a;
    Some(&xml[a..b])
}

/// Replace the text of `tag` inside `scope`, returning the new `scope`.
fn set_element(scope: &str, tag: &str, value: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    match (scope.find(&open), scope.find(&close)) {
        (Some(a), Some(b)) if b > a => {
            let start = a + open.len();
            format!("{}{}{}", &scope[..start], value, &scope[b..])
        }
        // Absent: append inside the scope, matching Cemu's indentation.
        _ => format!("{}\n        {open}{value}{close}", scope.trim_end()),
    }
}

pub fn read_service(xml: &str) -> (NetworkService, bool) {
    let account = element(xml, "Account").unwrap_or("");
    let service = element(account, "ActiveService")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map(NetworkService::from_code)
        .unwrap_or(NetworkService::Nintendo);
    let online = element(account, "OnlineEnabled")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    (service, online)
}

/// Rewrite the `<Account>` element only.
///
/// settings.xml is the user's whole Cemu configuration — window geometry, game
/// paths, graphic packs — so everything outside `<Account>` is passed through
/// untouched.
pub fn apply_service(xml: &str, service: NetworkService, online: bool) -> String {
    let Some(account) = element(xml, "Account") else {
        // No account element at all; leave the file alone rather than guess at
        // a structure Cemu may not accept.
        return xml.to_string();
    };
    let updated = set_element(account, "ActiveService", &service.code().to_string());
    let updated = set_element(&updated, "OnlineEnabled", if online { "true" } else { "false" });
    xml.replacen(account, &updated, 1)
}

pub fn settings_path(cemu_data_dir: &Path) -> PathBuf {
    cemu_data_dir.join("settings.xml")
}

/// Persist the network choice.
///
/// Refuses while Cemu is running, for the same reason as graphic packs: Cemu
/// rewrites settings.xml wholesale on exit and would discard the change.
pub fn set_service(cemu_data_dir: &Path, service: NetworkService, online: bool) -> Result<()> {
    if crate::config::cemu::graphic_packs::cemu_is_running() {
        anyhow::bail!(
            "Cemu is running. It rewrites settings.xml when it closes, which would \
             discard this change — close Cemu and try again."
        );
    }
    let path = settings_path(cemu_data_dir);
    let xml = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let updated = apply_service(&xml, service, online);
    std::fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Set an account's network ID (its PNID username).
///
/// Only `AccountId` is written. The password is deliberately not handled here:
/// Cemu stores a derived `AccountPasswordCache`, and reimplementing that
/// derivation would risk writing a value Cemu cannot use. Cemu's own account
/// settings screen does it correctly.
pub fn set_account_id(cemu_data_dir: &Path, persistent_id: &str, pnid: &str) -> Result<()> {
    if crate::config::cemu::graphic_packs::cemu_is_running() {
        anyhow::bail!("Cemu is running — close it before editing accounts.");
    }
    let path = cemu_data_dir
        .join("mlc01/usr/save/system/act")
        .join(persistent_id)
        .join("account.dat");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;

    let pnid = pnid.trim();
    let mut wrote = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.split('=').next().map(str::trim) == Some("AccountId") {
            out.push(format!("AccountId={pnid}"));
            wrote = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !wrote {
        out.push(format!("AccountId={pnid}"));
    }

    let mut joined = out.join("\n");
    joined.push('\n');
    std::fs::write(&path, joined).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

pub fn status(cemu_data_dir: Option<&Path>) -> Status {
    let Some(dir) = cemu_data_dir else {
        return Status {
            cemu_configured: false,
            data_dir: String::new(),
            service: NetworkService::Nintendo,
            online_enabled: false,
            accounts: Vec::new(),
            files: Vec::new(),
            files_complete: false,
            cemu_running: false,
        };
    };

    let xml = std::fs::read_to_string(settings_path(dir)).unwrap_or_default();
    let (service, online_enabled) = read_service(&xml);
    let files = check_files(dir);
    let files_complete = files.iter().all(|f| f.present);

    let active_id = element(&xml, "Account")
        .and_then(|a| element(a, "PersistentId"))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map(|v| format!("{v:08x}"));

    let mut accounts = list_accounts(dir);
    if let Some(active) = active_id {
        for a in &mut accounts {
            a.is_active = a.persistent_id.eq_ignore_ascii_case(&active);
        }
    }

    Status {
        cemu_configured: true,
        data_dir: dir.to_string_lossy().into_owned(),
        service,
        online_enabled,
        accounts,
        files,
        files_complete,
        cemu_running: crate::config::cemu::graphic_packs::cemu_is_running(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape taken from this machine's settings.xml.
    const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<content>
    <language>0</language>
    <Account>
        <PersistentId>2147483651</PersistentId>
        <OnlineEnabled>false</OnlineEnabled>
        <ActiveService>0</ActiveService>
    </Account>
    <GraphicPack>
        <Entry filename="packs/x/rules.txt"/>
    </GraphicPack>
</content>"#;

    #[test]
    fn maps_cemus_service_codes() {
        assert_eq!(NetworkService::from_code(0), NetworkService::Nintendo);
        assert_eq!(NetworkService::from_code(1), NetworkService::Pretendo);
        assert_eq!(NetworkService::from_code(2), NetworkService::Custom);
        // Anything unexpected must fall back to the safe default.
        assert_eq!(NetworkService::from_code(99), NetworkService::Nintendo);
        assert_eq!(NetworkService::Pretendo.code(), 1);
    }

    #[test]
    fn reads_the_current_service_and_online_flag() {
        let (svc, online) = read_service(SETTINGS);
        assert_eq!(svc, NetworkService::Nintendo);
        assert!(!online);
    }

    #[test]
    fn switching_to_pretendo_touches_nothing_else() {
        let out = apply_service(SETTINGS, NetworkService::Pretendo, true);
        let (svc, online) = read_service(&out);
        assert_eq!(svc, NetworkService::Pretendo);
        assert!(online);

        // The rest of the user's configuration must survive verbatim.
        assert!(out.contains("<language>0</language>"));
        assert!(out.contains("<PersistentId>2147483651</PersistentId>"));
        assert!(out.contains(r#"<Entry filename="packs/x/rules.txt"/>"#));
        assert!(out.trim_start().starts_with("<?xml"));
    }

    #[test]
    fn switching_back_restores_the_original() {
        let on = apply_service(SETTINGS, NetworkService::Pretendo, true);
        let off = apply_service(&on, NetworkService::Nintendo, false);
        assert_eq!(off.trim(), SETTINGS.trim(), "round trip was not clean");
    }

    #[test]
    fn missing_account_element_is_left_alone() {
        let bare = "<?xml version=\"1.0\"?>\n<content>\n  <language>0</language>\n</content>";
        assert_eq!(apply_service(bare, NetworkService::Pretendo, true), bare);
    }

    #[test]
    fn parses_an_unlinked_local_account() {
        // The real account on this machine: no PNID yet.
        let dat = "[AccountInstance_20120705]\nPersistentId=80000003\nMiiName=0055007300650072\nAccountId=\nPrincipalId=0\n";
        let a = parse_account(dat, "80000003");
        assert_eq!(a.persistent_id, "80000003");
        assert!(a.account_id.is_empty());
        assert_eq!(a.principal_id, 0);
        assert!(!a.is_linked(), "an account with no network id is not usable online");
    }

    #[test]
    fn parses_a_linked_account() {
        // PrincipalId is hex, exactly as the real dumped account stores it.
        let dat = "PersistentId=80000001\nAccountId=SomePnid\nPrincipalId=578bc6af\n";
        let a = parse_account(dat, "80000001");
        assert_eq!(a.account_id, "SomePnid");
        assert_eq!(a.principal_id, 0x578b_c6af);
        assert!(a.is_linked());
    }

    #[test]
    fn an_account_id_without_a_principal_id_is_not_linked() {
        // Half-configured accounts are a real state and must not read as ready.
        let a = parse_account("AccountId=Someone\nPrincipalId=0\n", "80000001");
        assert!(!a.is_linked());
    }

    #[test]
    fn decodes_hex_utf16_mii_names() {
        // "User" as UTF-16BE hex.
        assert_eq!(decode_mii_name("0055007300650072"), "User");
        // Non-hex values pass through unchanged.
        assert_eq!(decode_mii_name("Plain"), "Plain");
        assert_eq!(decode_mii_name(""), "");
    }

    #[test]
    fn setting_account_id_preserves_other_fields() {
        let dir = std::env::temp_dir().join(format!("pretendo-acc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let acc = dir.join("mlc01/usr/save/system/act/80000003");
        std::fs::create_dir_all(&acc).unwrap();
        std::fs::write(
            acc.join("account.dat"),
            "[AccountInstance_20120705]\nPersistentId=80000003\nMiiName=0055007300650072\nAccountId=\nPrincipalId=0\nGender=0\n",
        )
        .unwrap();

        // Guarded on Cemu not running; in tests it is not.
        set_account_id(&dir, "80000003", "MyPnid").unwrap();

        let out = std::fs::read_to_string(acc.join("account.dat")).unwrap();
        assert!(out.contains("AccountId=MyPnid"), "{out}");
        assert!(out.contains("PersistentId=80000003"), "lost a field:\n{out}");
        assert!(out.contains("MiiName=0055007300650072"));
        assert!(out.contains("Gender=0"));
        assert!(out.contains("[AccountInstance_20120705]"), "lost the header");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn counts_real_cert_files_by_presence_not_extension() {
        let dir = std::env::temp_dir().join(format!("pretendo-certs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ccerts = dir.join("mlc01/sys/title/0005001b/10054000/content/ccerts");
        std::fs::create_dir_all(&ccerts).unwrap();
        // Exactly the shapes Dumpling produces -- .der and .aes, never .cert.
        std::fs::write(ccerts.join("WIIU_ACCOUNT_1_CERT.der"), b"x").unwrap();
        std::fs::write(ccerts.join("WIIU_ACCOUNT_1_RSA_KEY.aes"), b"x").unwrap();
        std::fs::write(ccerts.join("WIIU_WAGONU_HMAC_KEY.aes"), b"x").unwrap();

        let files = check_files(&dir);
        let client = files.iter().find(|f| f.label.contains("Client")).unwrap();
        assert!(client.present, "real .der/.aes certs must count as present");
        assert_eq!(client.count, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_missing_online_files_individually() {
        let dir = std::env::temp_dir().join(format!("pretendo-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let files = check_files(&dir);
        assert_eq!(files.len(), 5, "otp, seeprom, account, ccerts, scerts");
        assert!(files.iter().all(|f| !f.present));
        // Each entry must say where the file belongs, not just that it is absent.
        assert!(files.iter().all(|f| !f.relative_path.is_empty()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

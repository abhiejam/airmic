//! Pairing (docs/protocol.md §2.3, §4): the 4 digit code and the paired phones in `paired.json`.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "the control channel uses it from the next PR")
)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::info;

const CODE_LIFETIME: Duration = Duration::from_secs(120);
const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub phone_id: String,
    pub phone_name: String,
    pub token: String,
    /// Unix seconds.
    pub paired_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairOutcome {
    Paired { token: String },
    BadCode,
    Locked,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    devices: Vec<PairedDevice>,
}

enum Code {
    None,
    Active {
        digits: String,
        issued: Instant,
        failures: u32,
    },
    Locked,
}

pub struct Pairing {
    path: PathBuf,
    devices: Vec<PairedDevice>,
    code: Code,
}

impl Pairing {
    /// Loads the paired phones from `path`, or starts empty when the file does not exist.
    pub fn load(path: PathBuf) -> anyhow::Result<Pairing> {
        let store: Store = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Pairing {
            path,
            devices: store.devices,
            code: Code::None,
        })
    }

    /// Returns the pairing code while it is valid: issued less than 2 minutes ago and not locked.
    pub fn current_code(&self, now: Instant) -> Option<&str> {
        match &self.code {
            Code::Active { digits, issued, .. }
                if now.saturating_duration_since(*issued) < CODE_LIFETIME =>
            {
                Some(digits)
            }
            _ => None,
        }
    }

    /// Returns true after 5 wrong codes, until the user shows a new code.
    pub fn is_locked(&self) -> bool {
        matches!(self.code, Code::Locked)
    }

    /// Issues a fresh code, which also lifts a lockout.
    pub fn regenerate_code(&mut self, now: Instant) -> String {
        let digits = format!("{:04}", rand::random_range(0..10_000u16));
        // Until the desktop app shows the code over IPC (D3.7), the log is the only place to read it.
        info!("pairing code {digits}");
        self.code = Code::Active {
            digits: digits.clone(),
            issued: now,
            failures: 0,
        };
        digits
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "for the IPC server (D3.7)"))]
    pub fn devices(&self) -> &[PairedDevice] {
        &self.devices
    }

    pub fn is_known(&self, phone_id: &str) -> bool {
        self.devices.iter().any(|d| d.phone_id == phone_id)
    }

    pub fn is_token_valid(&self, phone_id: &str, token: &str) -> bool {
        self.devices
            .iter()
            .any(|d| d.phone_id == phone_id && d.token == token)
    }

    /// Checks `code` and, when it matches, stores the phone with a new token and retires the code.
    pub fn pair(
        &mut self,
        code: &str,
        phone_id: &str,
        phone_name: &str,
        now: Instant,
    ) -> anyhow::Result<PairOutcome> {
        let valid = self.current_code(now).is_some();
        let (digits, failures) = match &mut self.code {
            Code::Locked => return Ok(PairOutcome::Locked),
            Code::Active {
                digits, failures, ..
            } if valid => (digits, failures),
            _ => return Ok(PairOutcome::BadCode),
        };
        if code != digits.as_str() {
            *failures += 1;
            if *failures < MAX_ATTEMPTS {
                return Ok(PairOutcome::BadCode);
            }
            self.code = Code::Locked;
            return Ok(PairOutcome::Locked);
        }
        self.code = Code::None;
        let token = format!("{:032x}", rand::random::<u128>());
        self.devices.retain(|d| d.phone_id != phone_id);
        self.devices.push(PairedDevice {
            phone_id: phone_id.to_string(),
            phone_name: phone_name.to_string(),
            token: token.clone(),
            paired_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        });
        self.save()?;
        info!(phone_id, phone_name, "phone paired");
        Ok(PairOutcome::Paired { token })
    }

    /// Removes a paired phone. Returns false when it was not paired.
    #[cfg_attr(not(test), expect(dead_code, reason = "for the IPC server (D3.7)"))]
    pub fn forget(&mut self, phone_id: &str) -> anyhow::Result<bool> {
        let before = self.devices.len();
        self.devices.retain(|d| d.phone_id != phone_id);
        if self.devices.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Writes `paired.json` through a temp file and a rename, so a crash never leaves it half written.
    fn save(&self) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let store = Store {
            devices: self.devices.clone(),
        };
        let write = || -> std::io::Result<()> {
            let mut file = fs::File::create(&tmp)?;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.write_all(serde_json::to_string_pretty(&store)?.as_bytes())?;
            file.sync_all()?;
            fs::rename(&tmp, &self.path)
        };
        write().with_context(|| format!("writing {}", self.path.display()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Returns a `paired.json` path in a fresh temp directory.
    pub(crate) fn temp_store() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("airmic-test-{:016x}", rand::random::<u64>()));
        dir.join("paired.json")
    }

    fn pair(p: &mut Pairing, code: &str, now: Instant) -> PairOutcome {
        p.pair(code, "p1", "iPhone", now).unwrap()
    }

    fn wrong(code: &str) -> String {
        format!("{:04}", (code.parse::<u16>().unwrap() + 1) % 10_000)
    }

    #[test]
    fn right_code_pairs_and_persists_the_phone() {
        let path = temp_store();
        let (mut p, t) = (Pairing::load(path.clone()).unwrap(), Instant::now());
        let code = p.regenerate_code(t);
        assert!(code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit()));
        let PairOutcome::Paired { token } = pair(&mut p, &code, t) else {
            panic!("expected paired");
        };
        assert!(
            token.len() == 32
                && token
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert_eq!(p.current_code(t), None, "a code pairs one phone");

        let reloaded = Pairing::load(path.clone()).unwrap();
        assert!(reloaded.is_token_valid("p1", &token));
        assert!(!reloaded.is_token_valid("p1", &"0".repeat(32)));
        assert!(!reloaded.is_token_valid("p2", &token));
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn code_expires_after_2_minutes() {
        let (mut p, t) = (Pairing::load(temp_store()).unwrap(), Instant::now());
        let code = p.regenerate_code(t);
        let later = t + Duration::from_secs(119);
        assert_eq!(p.current_code(later), Some(code.as_str()));
        let expired = t + CODE_LIFETIME;
        assert_eq!(p.current_code(expired), None);
        assert_eq!(pair(&mut p, &code, expired), PairOutcome::BadCode);
    }

    #[test]
    fn fifth_wrong_code_locks_until_a_new_code() {
        let (mut p, t) = (Pairing::load(temp_store()).unwrap(), Instant::now());
        let code = p.regenerate_code(t);
        for _ in 0..4 {
            assert_eq!(pair(&mut p, &wrong(&code), t), PairOutcome::BadCode);
        }
        assert_eq!(pair(&mut p, &wrong(&code), t), PairOutcome::Locked);
        assert!(p.is_locked());
        assert_eq!(
            pair(&mut p, &code, t),
            PairOutcome::Locked,
            "old code is dead"
        );
        let code = p.regenerate_code(t);
        assert!(matches!(pair(&mut p, &code, t), PairOutcome::Paired { .. }));
    }

    #[test]
    fn forget_removes_the_phone_from_disk() {
        let path = temp_store();
        let (mut p, t) = (Pairing::load(path.clone()).unwrap(), Instant::now());
        let code = p.regenerate_code(t);
        pair(&mut p, &code, t);
        assert_eq!(p.devices().len(), 1);
        assert!(p.forget("p1").unwrap());
        assert!(!p.forget("p1").unwrap());
        assert!(!Pairing::load(path).unwrap().is_known("p1"));
    }
}

//! What a timing depends on besides the code: the machine, the operating
//! system, the compilers and the build profile. Absolute times are only
//! comparable between runs with the same fingerprint.

//! What a timing depends on besides the code: the machine, the operating
//! system, the compilers and the build profile. Absolute times are only
//! comparable between runs with the same fingerprint.

use std::collections::BTreeMap;

/// The things a timing depends on, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// `cpu`, `os`, `rustc`, `profile` and `c`.
    pub fields: BTreeMap<String, String>,
}

/// Run a command and return its trimmed output, or `unknown`.
fn command(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn cpu() -> String {
    if cfg!(target_os = "macos") {
        return command("sysctl", &["-n", "machdep.cpu.brand_string"]);
    }
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into())
}

impl Fingerprint {
    /// This machine and build; `c` is the C engine's build information, if
    /// C is part of the run.
    pub fn current(c: Option<String>) -> Fingerprint {
        let mut fields = BTreeMap::new();
        fields.insert("cpu".into(), cpu());
        fields.insert("os".into(), command("uname", &["-sr"]));
        fields.insert("rustc".into(), env!("AST_BENCH_RUSTC").to_string());
        fields.insert(
            "profile".into(),
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .into(),
        );
        fields.insert("c".into(), c.unwrap_or_else(|| "none".into()));
        Fingerprint { fields }
    }

    /// `key=value` lines in key order.
    pub fn to_text(&self) -> String {
        self.fields
            .iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect()
    }

    /// The inverse of [`to_text`](Self::to_text).
    pub fn parse(text: &str) -> Fingerprint {
        Fingerprint {
            fields: text
                .lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// A short identifier: the FNV-1a hash of [`to_text`](Self::to_text), so
    /// it is stable across Rust releases.
    pub fn id(&self) -> String {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in self.to_text().bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
        format!("{h:016x}")
    }

    /// The names of the fields that differ from `other`'s.
    pub fn diff(&self, other: &Fingerprint) -> Vec<String> {
        let mut keys: Vec<&String> = self.fields.keys().chain(other.fields.keys()).collect();
        keys.sort();
        keys.dedup();
        keys.into_iter()
            .filter(|k| self.fields.get(*k) != other.fields.get(*k))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_stable_and_field_sensitive() {
        let mut a = Fingerprint {
            fields: BTreeMap::new(),
        };
        a.fields.insert("cpu".into(), "M4".into());
        let mut b = a.clone();
        assert_eq!(a.id(), b.id());
        b.fields.insert("cpu".into(), "M3".into());
        assert_ne!(a.id(), b.id());
        assert_eq!(a.diff(&b), vec!["cpu".to_string()]);
    }

    #[test]
    fn text_round_trips() {
        let f = Fingerprint::current(Some("variant=bench".into()));
        assert_eq!(Fingerprint::parse(&f.to_text()), f);
    }
}

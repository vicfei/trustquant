//! Manifest store (ADR-0004 §2): one file per calculation, atomic write via
//! temp-file + rename, lookup by filename == calculation_id.

use florin_core::manifest::CalculationManifest;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum LoadError {
    NotFound,
}

pub struct ManifestStore {
    base: PathBuf,
}

impl ManifestStore {
    /// Resolution order: $FLORIN_DATA_DIR → $XDG_DATA_HOME/florin
    /// → ~/.local/share/florin (ADR-0004 §2).
    pub fn default_store() -> Self {
        let base = std::env::var_os("FLORIN_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .map(|p| p.join("florin"))
            })
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
                PathBuf::from(home).join(".local/share/florin")
            });
        ManifestStore { base }
    }

    pub fn at(base: impl AsRef<Path>) -> Self {
        ManifestStore {
            base: base.as_ref().to_path_buf(),
        }
    }

    pub fn calculations_dir(&self) -> PathBuf {
        self.base.join("calculations")
    }

    fn manifest_path(&self, calculation_id: &str) -> PathBuf {
        // calculation_id is minted by us (hex uuid form); refuse separators
        // so a caller cannot walk the filesystem with an arbitrary id.
        let safe: String = calculation_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.calculations_dir().join(format!("{safe}.json"))
    }

    /// Atomic write: serialize, write to `<id>.json.tmp`, fsync, rename.
    pub fn save(&self, m: &CalculationManifest) -> std::io::Result<()> {
        let dir = self.calculations_dir();
        std::fs::create_dir_all(&dir)?;
        let final_path = self.manifest_path(&m.calculation_id);
        let tmp_path = final_path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(m)?; // pretty for humans; hash covers canonical form, not file bytes
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp_path)?;
            f.write_all(&body)?;
            f.write_all(b"\n")?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    pub fn load(&self, calculation_id: &str) -> Result<CalculationManifest, LoadError> {
        let path = self.manifest_path(calculation_id);
        let mut body = String::new();
        match std::fs::File::open(&path) {
            Ok(mut f) => {
                f.read_to_string(&mut body)
                    .map_err(|_| LoadError::NotFound)?;
            }
            Err(_) => return Err(LoadError::NotFound),
        }
        serde_json::from_str(&body).map_err(|_| LoadError::NotFound)
    }
}

/// Mint an execution identity: UUIDv4-format from OS entropy. Execution
/// identity, not content (ADR-0001 §3): identical computations get distinct
/// ids; replay compares digests, never ids.
pub fn new_calculation_id() -> String {
    let mut bytes = [0u8; 16];
    if read_urandom(&mut bytes) {
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &h[0..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..32]
        )
    } else {
        // Offline fallback with reduced uniqueness; recorded honestly.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id() as u128;
        let v = t ^ (pid << 64) ^ 0x9e3779b97f4a7c15u128;
        let h: String = v.to_le_bytes().iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}-{}-{}-{}-offline",
            &h[0..8],
            &h[8..12],
            &h[12..16],
            &h[16..20]
        )
    }
}

fn read_urandom(buf: &mut [u8; 16]) -> bool {
    use std::io::Read;
    match std::fs::File::open("/dev/urandom") {
        Ok(mut f) => f.read_exact(buf).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use florin_core::contract::ToolOutput;
    use florin_core::manifest::{self, ManifestContext};
    use serde_json::json;

    fn temp_store() -> (ManifestStore, tempdir::Guard) {
        let dir = std::env::temp_dir().join(format!("finstr-test-{}", new_calculation_id()));
        std::fs::create_dir_all(&dir).unwrap();
        (ManifestStore::at(&dir), tempdir::Guard(dir))
    }

    // tiny RAII guard so tests don't leave litter
    mod tempdir {
        pub struct Guard(pub std::path::PathBuf);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn sample_manifest(id: &str) -> CalculationManifest {
        let req = json!({"schema_version": "0.1", "tool": "price_european_option", "params": {"spot": 100}});
        let out = ToolOutput {
            engine_id: "bs_analytic.v1",
            provider_id: "builtin-rust",
            result: json!({"npv": 6.98}),
            warnings: vec![],
        };
        manifest::build(
            &ManifestContext {
                calculation_id: id,
                runtime_version: "test",
                provider_version: "test",
            },
            &req,
            "price_european_option",
            &out,
        )
        .unwrap()
    }

    #[test]
    fn atomic_roundtrip_and_no_path_traversal() {
        let (store, _guard) = temp_store();
        let m = sample_manifest("aaaaaaaa-0000-4000-8000-000000000001");
        store.save(&m).unwrap();
        let loaded = store.load("aaaaaaaa-0000-4000-8000-000000000001").unwrap();
        assert_eq!(loaded.manifest_hash, m.manifest_hash);
        assert!(matches!(store.load("no-such-id"), Err(LoadError::NotFound)));
        // traversal attempt is neutralized to a Not Found, never a file read
        assert!(matches!(
            store.load("../../etc/passwd"),
            Err(LoadError::NotFound)
        ));
    }

    #[test]
    fn ids_are_execution_identity_not_content() {
        let a = new_calculation_id();
        let b = new_calculation_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
    }
}

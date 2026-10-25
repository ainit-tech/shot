//! Updates, apart from Windows: the program learns of a new version from the description of the
//! latest release on the program's site, `latest.json`, and takes only a version newer than its
//! own, from a file of the size and SHA-256 the description gives. How the description is
//! fetched, checked and the new version installed is in `win::update`.

use std::collections::HashMap;
use std::fmt;

use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The description of the latest release.
pub const MANIFEST: &str = "https://shot.ainit.tech/download/latest.json";
/// The kind of file this program installs itself from.
pub const PLATFORM: &str = "windows-x64";
/// No release of the program comes near this; a bigger file is a mistake or worse.
const LARGEST: u64 = 200 * 1024 * 1024;

/// A version of the program: three numbers, as in `Cargo.toml` and in the tags of releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let mut numbers = text.split('.').map(|part| {
            let plain = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) && (part == "0" || !part.starts_with('0'));
            plain.then(|| part.parse::<u64>().ok()).flatten()
        });
        let version = Version(numbers.next()??, numbers.next()??, numbers.next()??);
        numbers.next().is_none().then_some(version)
    }

    /// The version of this program.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("the version in Cargo.toml is X.Y.Z")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A file of a release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct File {
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// A release newer than the program, with the file this program installs it from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub file: File,
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    version: String,
    files: HashMap<String, File>,
}

/// What the description of the latest release offers a program of the version `current`: a newer
/// release, or nothing when the program is as new or newer. The program never goes back to an
/// older version.
pub fn offer(manifest: &[u8], current: Version) -> Result<Option<Release>, String> {
    let manifest: Manifest = serde_json::from_slice(manifest).map_err(|e| format!("latest.json: {e}"))?;
    if manifest.name != "ainitShot" {
        return Err(format!("latest.json describes {}", manifest.name));
    }
    let version = Version::parse(&manifest.version).ok_or_else(|| format!("latest.json: version {}", manifest.version))?;
    if version <= current {
        return Ok(None);
    }
    let file = manifest.files.get(PLATFORM).ok_or_else(|| format!("latest.json has no file for {PLATFORM}"))?;
    if !file.url.starts_with("https://") {
        return Err(format!("latest.json: {} isn't https", file.url));
    }
    if file.size == 0 || file.size > LARGEST {
        return Err(format!("latest.json: size {}", file.size));
    }
    if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("latest.json: sha256 {}", file.sha256));
    }
    Ok(Some(Release { version, file: File { sha256: file.sha256.to_ascii_lowercase(), ..file.clone() } }))
}

/// Whether `manifest` is signed with the secret key whose public half is `key`: the signature is
/// minisign's, as `minisign -S` writes it.
pub fn signed(manifest: &[u8], signature: &str, key: &str) -> bool {
    let (Ok(key), Ok(signature)) = (PublicKey::from_base64(key), Signature::decode(signature)) else { return false };
    key.verify(manifest, &signature, false).is_ok()
}

/// SHA-256 of a file as it arrives, for comparing with the description.
#[derive(Default)]
pub struct Hash(Sha256);

impl Hash {
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The hash in lowercase hex, as the description gives it.
    pub fn hex(self) -> String {
        self.0.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_three_numbers_and_compare_as_numbers() {
        assert_eq!(Version::parse("0.1.0"), Some(Version(0, 1, 0)));
        assert_eq!(Version::parse("10.20.30"), Some(Version(10, 20, 30)));
        for wrong in ["", "1", "1.2", "1.2.3.4", "v1.2.3", "1.2.x", "01.2.3", "1.2.3-beta", " 1.2.3", "1..3"] {
            assert_eq!(Version::parse(wrong), None, "{wrong}");
        }
        assert!(Version::parse("0.10.0") > Version::parse("0.9.9"));
        assert_eq!(Version::parse("1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    fn manifest(version: &str) -> String {
        format!(
            r#"{{"name": "ainitShot", "version": "{version}", "date": "2026-11-01", "files": {{
                "windows-x64": {{"url": "https://shot.ainit.tech/download/{version}/ainitShot-{version}-windows-x64-setup.exe",
                                 "size": 4567890, "sha256": "{}"}},
                "macos": {{"url": "https://shot.ainit.tech/download/{version}/ainitShot-{version}-macos.dmg", "size": 1, "sha256": "x"}}
            }}}}"#,
            "AB".repeat(32)
        )
    }

    #[test]
    fn only_a_newer_version_is_offered() {
        let current = Version(0, 2, 0);
        let newer = offer(manifest("0.3.0").as_bytes(), current).unwrap().unwrap();
        assert_eq!(newer.version, Version(0, 3, 0));
        assert_eq!(newer.file.size, 4567890);
        assert_eq!(newer.file.sha256, "ab".repeat(32));
        assert!(newer.file.url.ends_with("ainitShot-0.3.0-windows-x64-setup.exe"));
        assert_eq!(offer(manifest("0.2.0").as_bytes(), current), Ok(None));
        assert_eq!(offer(manifest("0.1.9").as_bytes(), current), Ok(None));
    }

    #[test]
    fn a_wrong_description_offers_nothing() {
        let current = Version(0, 1, 0);
        let wrong = [
            manifest("0.2"),
            manifest("0.2.0").replace("ainitShot\"", "other\""),
            manifest("0.2.0").replace("windows-x64", "windows-arm64"),
            manifest("0.2.0").replace("https://shot.ainit.tech/download/0.2.0/ainitShot", "http://shot.ainit.tech/download/0.2.0/ainitShot"),
            manifest("0.2.0").replace("4567890", "0"),
            manifest("0.2.0").replace(&"AB".repeat(32), "abc"),
            "not json".into(),
        ];
        for text in wrong {
            assert!(offer(text.as_bytes(), current).is_err(), "{text}");
        }
    }

    /// The key and the signature of "test" from minisign-verify's documentation.
    const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIGNATURE: &str = "untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1633700835\tfile:test\tprehashed
wLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";

    #[test]
    fn only_what_the_key_signed_passes() {
        assert!(signed(b"test", SIGNATURE, KEY));
        assert!(!signed(b"Test", SIGNATURE, KEY));
        assert!(!signed(b"test", &SIGNATURE.replace("wLMD", "xLMD"), KEY));
        assert!(!signed(b"test", SIGNATURE, "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO4"));
        assert!(!signed(b"test", "", KEY));
        assert!(!signed(b"test", SIGNATURE, ""));
    }

    #[test]
    fn the_hash_is_sha256_in_hex() {
        let mut hash = Hash::default();
        hash.update(b"te");
        hash.update(b"st");
        assert_eq!(hash.hex(), "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08");
    }
}

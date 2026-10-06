// SPDX-License-Identifier: MIT

use flight_trust::{Fingerprint, Identity, Role, TrustStore};
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flight-trust-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn identities_are_distinct_and_the_fingerprint_is_the_id() {
    let a = Identity::generate().expect("a");
    let b = Identity::generate().expect("b");
    assert_ne!(a.fingerprint(), b.fingerprint());
    let text = a.fingerprint().as_str();
    assert!(text.starts_with("sha256:") && text.len() == "sha256:".len() + 64);
    assert_eq!(a.fingerprint().host_id().as_str(), text);
    assert_eq!(&Fingerprint::parse(text).expect("parses"), a.fingerprint());
}

#[test]
fn an_identity_survives_save_and_load_and_its_key_is_private() {
    let dir = tmp("identity");
    let made = Identity::load_or_create(&dir).expect("create");
    let again = Identity::load_or_create(&dir).expect("load");
    assert_eq!(made.fingerprint(), again.fingerprint());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("key.pem"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "private key must be owner-only");
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn a_changed_key_is_a_different_identity_whatever_it_is_called() {
    let before = Identity::generate().expect("before");
    let after = Identity::generate().expect("after");
    let mut store = TrustStore::empty();
    store.authorize(before.fingerprint(), "mini-1", Role::Node);
    assert!(store.is_authorized(before.fingerprint(), Role::Node));
    assert!(
        !store.is_authorized(after.fingerprint(), Role::Node),
        "same name, new key: not trusted"
    );
}

#[test]
fn fingerprints_reject_malformed_text() {
    for bad in [
        "",
        "sha256:",
        "sha256:XYZ",
        "md5:00",
        &format!("sha256:{}", "A".repeat(64)),
        &format!("sha256:{}", "a".repeat(63)),
    ] {
        assert!(Fingerprint::parse(bad).is_err(), "{bad}");
    }
}

fn id(n: char) -> Fingerprint {
    Fingerprint::parse(&format!("sha256:{}", n.to_string().repeat(64))).expect("fp")
}

#[test]
fn the_documented_trust_file_parses_and_round_trips() {
    let text = format!(
        r#"
version = 1

[orchestrator]
fingerprint = "{o}"
display_name = "flight-home"

[[nodes]]
id = "{a}"
display_name = "mini-1"
enabled = true

[[nodes]]
id = "{b}"
display_name = "mini-2"
enabled = false

[[nodes]]
id = "{u}"
display_name = "laptop"
role = "ui"
"#,
        o = id('0'),
        a = id('a'),
        b = id('b'),
        u = id('c')
    );
    let store = TrustStore::parse(&text).expect("parse");
    assert!(store.is_authorized(&id('a'), Role::Node));
    assert!(!store.is_authorized(&id('b'), Role::Node), "disabled");
    assert!(
        store.is_authorized(&id('c'), Role::Ui),
        "enabled by default"
    );
    assert!(
        !store.is_authorized(&id('c'), Role::Node),
        "a UI is not a node"
    );
    assert!(!store.is_authorized(&id('d'), Role::Node), "unknown");
    assert_eq!(
        store.orchestrator().map(|o| o.display_name.as_str()),
        Some("flight-home")
    );
    let again = TrustStore::parse(&store.to_toml().expect("write")).expect("reparse");
    assert_eq!(again, store);
}

#[test]
fn revocation_by_disable_or_removal() {
    let mut s = TrustStore::empty();
    s.authorize(&id('a'), "mini-1", Role::Node);
    assert!(s.disable(&id('a')));
    assert!(!s.is_authorized(&id('a'), Role::Node));
    s.authorize(&id('a'), "mini-1", Role::Node);
    assert!(
        s.is_authorized(&id('a'), Role::Node),
        "re-authorizing re-enables"
    );
    assert!(s.remove(&id('a')));
    assert!(!s.is_authorized(&id('a'), Role::Node));
    assert!(!s.disable(&id('a')));
}

#[test]
fn bad_trust_files_are_refused() {
    for text in [
        "version = 2",
        "nodes = []",
        "version = 1\n[[nodes]]\nid = \"nope\"\ndisplay_name = \"x\"",
        &format!(
            "version = 1\n[[nodes]]\nid = \"{a}\"\ndisplay_name = \"x\"\n[[nodes]]\nid = \"{a}\"\ndisplay_name = \"y\"",
            a = id('a')
        ),
    ] {
        assert!(TrustStore::parse(text).is_err(), "{text}");
    }
}

#[test]
fn a_missing_file_trusts_nothing_and_saving_is_atomic_and_public_only() {
    let dir = tmp("trust");
    let path = dir.join("trust.toml");
    assert_eq!(
        TrustStore::load(&path).expect("missing"),
        TrustStore::empty()
    );
    let mut s = TrustStore::empty();
    s.authorize(&id('a'), "mini-1", Role::Node);
    s.save(&path).expect("save");
    assert_eq!(TrustStore::load(&path).expect("load"), s);
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(!text.contains("PRIVATE"), "{text}");
    assert!(!dir.join("trust.toml.tmp").exists());
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

// SPDX-License-Identifier: MIT

use flight_trust::{EnrollError, EnrollmentTokens};

#[test]
fn a_token_works_once() {
    let mut t = EnrollmentTokens::new();
    let issued = t.issue(1_000, 600).expect("issue");
    assert_eq!(t.redeem(&issued.secret, 1_001), Ok(()));
    assert_eq!(
        t.redeem(&issued.secret, 1_002),
        Err(EnrollError::Invalid),
        "second use"
    );
}

#[test]
fn an_expired_token_is_refused_and_gone() {
    let mut t = EnrollmentTokens::new();
    let issued = t.issue(1_000, 600).expect("issue");
    assert_eq!(issued.expires_at, 1_600);
    assert_eq!(t.redeem(&issued.secret, 1_600), Err(EnrollError::Expired));
    assert_eq!(
        t.redeem(&issued.secret, 1_000),
        Err(EnrollError::Invalid),
        "it was consumed"
    );
}

#[test]
fn a_token_just_inside_its_lifetime_works() {
    let mut t = EnrollmentTokens::new();
    let issued = t.issue(1_000, 600).expect("issue");
    assert_eq!(t.redeem(&issued.secret, 1_599), Ok(()));
}

#[test]
fn guesses_fail_and_do_not_consume_real_tokens() {
    let mut t = EnrollmentTokens::new();
    let issued = t.issue(1_000, 600).expect("issue");
    assert_eq!(t.redeem("guess", 1_001), Err(EnrollError::Invalid));
    assert_eq!(t.redeem("", 1_001), Err(EnrollError::Invalid));
    assert_eq!(t.outstanding(), 1);
    assert_eq!(t.redeem(&issued.secret, 1_002), Ok(()));
}

#[test]
fn tokens_are_high_entropy_distinct_and_never_shown_by_debug() {
    let mut t = EnrollmentTokens::new();
    let a = t.issue(1, 600).expect("a");
    let b = t.issue(1, 600).expect("b");
    assert_ne!(a.secret, b.secret);
    assert_eq!(
        a.secret.len(),
        43,
        "256 random bits, base64url without padding"
    );
    assert!(a
        .secret
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
    assert!(!format!("{a:?}").contains(&a.secret));
}

#[test]
fn expired_tokens_are_swept_when_new_ones_are_issued() {
    let mut t = EnrollmentTokens::new();
    t.issue(1_000, 10).expect("old");
    t.issue(2_000, 10).expect("new");
    assert_eq!(t.outstanding(), 1);
}

mod bundle {
    use flight_trust::{EnrollmentBundle, Fingerprint};

    fn fp() -> Fingerprint {
        Fingerprint::parse(&format!("sha256:{}", "ab".repeat(32))).expect("fp")
    }

    #[test]
    fn a_bundle_round_trips_and_keeps_identity_and_token_separate() {
        let b = EnrollmentBundle {
            orchestrator: fp(),
            address: "192.168.1.20:7777".into(),
            token: "tok_en-123".into(),
            expires_at: 1_700_000_600,
        };
        let text = b.to_string();
        assert_eq!(EnrollmentBundle::parse(&text).expect("parse"), b);
        assert!(!format!("{b:?}").contains("tok_en-123"));
    }

    #[test]
    fn malformed_bundles_are_refused() {
        let ok = format!("orchestrator={} address=h:1 token=t expires=5", fp());
        assert!(EnrollmentBundle::parse(&ok).is_ok());
        for bad in [
            "",
            "address=h:1 token=t expires=5",
            &format!("orchestrator={} address=h:1 token=t", fp()),
            &format!("{ok} extra=1"),
            &format!("{ok} token=again"),
            "orchestrator=sha256:nope address=h:1 token=t expires=5",
            &format!("orchestrator={} address=h:1 token=t expires=soon", fp()),
        ] {
            assert!(EnrollmentBundle::parse(bad).is_err(), "{bad}");
        }
    }
}

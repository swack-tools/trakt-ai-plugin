//! Key sealing, rotation, and tamper rejection for client-held key encryption.
use trakt_mcp::seal::{self, Keys, RETIRED_KEY_SECONDS};

const NOW: u64 = 2_000_000_000;
const ID: &str = "0123456789abcdef0123456789abcdef";

fn hex(byte: &str) -> String {
    byte.repeat(32)
}

fn keys() -> Keys {
    Keys::parse(&format!(
        r#"{{"index":"{}","keys":[{{"kid":"new","key":"{}"}}]}}"#,
        hex("11"),
        hex("22")
    ))
    .unwrap()
}

fn rotated(retired_at: u64) -> Keys {
    Keys::parse(&format!(
        r#"{{"index":"{}","keys":[{{"kid":"old","key":"{}","retired_at":{retired_at}}},{{"kid":"newer","key":"{}"}}]}}"#,
        hex("11"),
        hex("22"),
        hex("33")
    ))
    .unwrap()
}

fn old_only() -> Keys {
    Keys::parse(&format!(
        r#"{{"index":"{}","keys":[{{"kid":"old","key":"{}"}}]}}"#,
        hex("11"),
        hex("22")
    ))
    .unwrap()
}

#[test]
fn a_sealed_token_opens_only_as_its_own_kind_and_before_expiry() {
    let (keys, k) = (keys(), seal::random_key());
    let token = keys.seal_token("access", ID, &k, NOW + 60).unwrap();
    assert!(token.starts_with(&format!("{ID}.new.")));
    assert!(
        !token.contains(&seal::to_hex(&k)),
        "K is sealed, not carried"
    );
    assert_eq!(keys.open_token("access", &token, NOW), Some(k));
    assert_eq!(keys.open_token("refresh", &token, NOW), None);
    assert_eq!(keys.open_token("access", &token, NOW + 60), None);
    assert_ne!(token, keys.seal_token("access", ID, &k, NOW + 60).unwrap());
    trakt_mcp::security::token_id(&token).unwrap();
}

#[test]
fn tampering_with_any_part_of_a_token_is_rejected() {
    let (keys, k) = (keys(), seal::random_key());
    let token = keys.seal_token("access", ID, &k, NOW + 60).unwrap();
    let other_id = token.replacen(ID, "f123456789abcdef0123456789abcdef", 1);
    assert_eq!(keys.open_token("access", &other_id, NOW), None);
    let mut flipped = token.clone().into_bytes();
    let last = flipped.len() - 1;
    flipped[last] = if flipped[last] == b'0' { b'1' } else { b'0' };
    assert_eq!(
        keys.open_token("access", &String::from_utf8(flipped).unwrap(), NOW),
        None
    );
    for bad in [
        "",
        "a.b",
        "a.b.c.d",
        &format!("{ID}.new.zz"),
        &format!("{ID}.nope.00"),
    ] {
        assert_eq!(keys.open_token("access", bad, NOW), None, "{bad}");
    }
}

#[test]
fn rotation_seals_with_the_new_key_and_retires_the_old_after_thirty_days() {
    let k = seal::random_key();
    let old_token = old_only()
        .seal_token("refresh", ID, &k, NOW + 90 * 86400)
        .unwrap();
    let retired_at = NOW;
    let keys = rotated(retired_at);
    assert_eq!(keys.open_token("refresh", &old_token, NOW + 1), Some(k));
    assert_eq!(
        keys.open_token("refresh", &old_token, retired_at + RETIRED_KEY_SECONDS),
        Some(k)
    );
    assert_eq!(
        keys.open_token("refresh", &old_token, retired_at + RETIRED_KEY_SECONDS + 1),
        None
    );
    let fresh = keys.seal_token("access", ID, &k, NOW + 60).unwrap();
    assert!(fresh.starts_with(&format!("{ID}.newer.")));
    assert_eq!(keys.open_token("access", &fresh, NOW), Some(k));
    assert_eq!(old_only().open_token("access", &fresh, NOW), None);
}

#[test]
fn invalid_key_configuration_fails_closed() {
    for text in [
        "",
        "{}",
        r#"{"index":"11","keys":[]}"#,
        &format!(r#"{{"index":"{}","keys":[]}}"#, hex("11")),
        &format!(
            r#"{{"index":"{}","keys":[{{"kid":"a","key":"22"}}]}}"#,
            hex("11")
        ),
        &format!(
            r#"{{"index":"{}","keys":[{{"kid":"a.b","key":"{}"}}]}}"#,
            hex("11"),
            hex("22")
        ),
        &format!(
            r#"{{"index":"{}","keys":[{{"kid":"a","key":"{}","retired_at":1}}]}}"#,
            hex("11"),
            hex("22")
        ),
    ] {
        assert!(Keys::parse(text).is_err(), "{text}");
    }
}

#[test]
fn a_grant_opens_only_with_its_key_and_user() {
    let (k, other) = (seal::random_key(), seal::random_key());
    assert_ne!(k, other);
    let sealed = seal::seal_grant(&k, "user-a", r#"{"refresh_token":"secret"}"#).unwrap();
    assert!(!sealed.contains("secret"));
    assert!(seal::open_grant(&k, "user-a", &sealed).is_some());
    assert!(seal::open_grant(&other, "user-a", &sealed).is_none());
    assert!(seal::open_grant(&k, "user-b", &sealed).is_none());
}

#[test]
fn identifiers_are_keyed_and_connection_keys_round_trip() {
    let keys = keys();
    let id = keys.grant_id("42").unwrap();
    assert_eq!(id.len(), 64);
    assert_ne!(id, trakt_mcp::security::hash("grant:42"));
    assert_ne!(id, trakt_mcp::security::hash("42"));
    assert_ne!(id, rotated(NOW).grant_id("43").unwrap());
    assert_eq!(
        id,
        rotated(NOW).grant_id("42").unwrap(),
        "rotation keeps grants findable"
    );
    assert_ne!(
        keys.ip_id("203.0.113.9").unwrap(),
        trakt_mcp::security::hash("203.0.113.9")
    );
    let k = seal::random_key();
    let text = seal::connection_key(&id, &k);
    assert_eq!(seal::parse_connection_key(&text), Some((id.clone(), k)));
    assert_eq!(
        seal::parse_connection_key(&format!(" {text}\n")),
        Some((id, k))
    );
    for bad in ["", "tck", "tck_aa_bb", "other_aa_bb", &format!("{text}_x")] {
        assert_eq!(seal::parse_connection_key(bad), None, "{bad}");
    }
}

#[test]
fn a_parked_login_is_bound_to_its_connection() {
    let keys = keys();
    let sealed = keys
        .seal_pending(ID, r#"{"refresh_token":"secret"}"#)
        .unwrap();
    assert!(!sealed.contains("secret"));
    assert!(keys.open_pending(ID, &sealed, NOW).is_some());
    assert!(keys.open_pending("other", &sealed, NOW).is_none());
}

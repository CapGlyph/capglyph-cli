//! C2PA integration tests — require the `c2pa` cargo feature.
#![cfg(feature = "c2pa")]

use sigil::c2pa::WatermarkClaim;

#[test]
fn watermark_claim_serde_roundtrip() {
    let claim = WatermarkClaim {
        mode: "dct".to_string(),
        recipient_id: Some("alice01".to_string()),
        keyed: true,
    };
    let json = serde_json::to_vec(&claim).unwrap();
    let back: WatermarkClaim = serde_json::from_slice(&json).unwrap();
    assert_eq!(back.mode, "dct");
    assert_eq!(back.recipient_id.as_deref(), Some("alice01"));
    assert!(back.keyed);
}

#[test]
fn watermark_claim_none_fields() {
    let claim = WatermarkClaim {
        mode: "dwt".to_string(),
        recipient_id: None,
        keyed: false,
    };
    let json = serde_json::to_vec(&claim).unwrap();
    let back: WatermarkClaim = serde_json::from_slice(&json).unwrap();
    assert_eq!(back.mode, "dwt");
    assert_eq!(back.recipient_id, None);
    assert!(!back.keyed);
}

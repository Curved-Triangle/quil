use std::{fs, path::Path};

#[test]
fn runtime_entitlements_have_no_network_access() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/Quil.entitlements");
    let entitlements = fs::read_to_string(path).expect("read entitlements");
    assert!(entitlements.contains("com.apple.security.app-sandbox"));
    assert!(entitlements.contains("com.apple.security.device.audio-input"));
    assert!(!entitlements.contains("com.apple.security.network.client"));
    assert!(!entitlements.contains("com.apple.security.network.server"));
}

#![cfg(windows)]

use control_center_core::windows::{
    ServiceRuntimeState, ServiceSource, WindowsServiceSource,
};

#[test]
fn native_service_source_enumerates_windows_services() {
    let source = WindowsServiceSource;
    let services = source
        .read_services()
        .expect("native Windows service enumeration should succeed");

    assert!(
        !services.is_empty(),
        "a normal Windows installation should expose at least one service"
    );
    assert!(
        services
            .iter()
            .all(|service| !service.service_name.trim().is_empty()),
        "native service records must have nonblank service names"
    );
    assert!(
        services
            .iter()
            .any(|service| service.runtime_state == ServiceRuntimeState::Running),
        "a normal Windows installation should expose at least one running service"
    );
}

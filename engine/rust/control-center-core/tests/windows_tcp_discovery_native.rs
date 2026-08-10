#![cfg(windows)]

use std::{
    net::{Ipv4Addr, TcpListener},
    process,
};

use control_center_core::windows::{TcpEndpointSource, WindowsTcpEndpointSource};

#[test]
fn native_tcp_endpoint_source_includes_current_process_ipv4_listener() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("test should be able to create a local TCP listener");
    let local_addr = listener
        .local_addr()
        .expect("test listener should have a local address");

    let endpoints = WindowsTcpEndpointSource
        .read_listening_endpoints()
        .expect("native Windows TCP endpoint enumeration should succeed");

    assert!(
        endpoints.iter().any(|endpoint| {
            endpoint.local_address == local_addr.ip().to_string()
                && endpoint.local_port == local_addr.port()
                && endpoint.owning_pid == Some(process::id())
        }),
        "native TCP endpoint source should include the listener created by this test"
    );
}

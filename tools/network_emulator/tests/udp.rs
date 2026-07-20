use std::{net::UdpSocket, thread, time::Duration};

use network_emulator::{Config, DirectionConfig, run};

#[test]
fn forwards_udp_through_both_directions() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server_addr = server.local_addr().unwrap();
    thread::spawn(move || {
        let mut buf = [0; 64];
        let (len, peer) = server.recv_from(&mut buf).unwrap();
        server.send_to(&buf[..len], peer).unwrap();
    });

    let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
    let listen_addr = listener.local_addr().unwrap();
    drop(listener);
    let delay = Duration::from_millis(5);
    thread::spawn(move || {
        run(Config {
            listen_addr,
            server_addr,
            server_bind_addr: None,
            buf_size: 1024,
            seed: 1,
            uplink: DirectionConfig {
                min_delay: delay,
                max_delay: delay,
                ..Default::default()
            },
            downlink: DirectionConfig {
                min_delay: delay,
                max_delay: delay,
                ..Default::default()
            },
        })
        .unwrap();
    });
    thread::sleep(Duration::from_millis(20));

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    client.send_to(b"ping", listen_addr).unwrap();
    let mut buf = [0; 64];
    let (len, _) = client.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..len], b"ping");
}

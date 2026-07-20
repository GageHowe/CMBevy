use std::{
    collections::HashMap,
    io,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct DirectionConfig {
    pub loss: f64,
    pub duplicate: f64,
    pub corrupt: f64,
    pub reorder: f64,
    pub min_delay: Duration,
    pub max_delay: Duration,
    pub reorder_window: Duration,
}

impl Default for DirectionConfig {
    fn default() -> Self {
        Self {
            loss: 0.0,
            duplicate: 0.0,
            corrupt: 0.0,
            reorder: 0.0,
            min_delay: Duration::from_millis(0),
            max_delay: Duration::from_millis(0),
            reorder_window: Duration::from_millis(25),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub server_addr: SocketAddr,
    pub server_bind_addr: Option<SocketAddr>,
    pub buf_size: usize,
    pub seed: u64,
    pub uplink: DirectionConfig,
    pub downlink: DirectionConfig,
}

struct LinkState {
    rng: fastrand::Rng,
    config: DirectionConfig,
}

impl LinkState {
    fn new(seed: u64, config: DirectionConfig) -> Self {
        Self {
            rng: fastrand::Rng::with_seed(seed),
            config,
        }
    }

    fn schedule(
        &mut self,
        data: &[u8],
        now: Instant,
        target: PacketTarget,
        tx: &mpsc::Sender<ScheduledPacket>,
    ) {
        if self.chance(self.config.loss) {
            return;
        }

        let mut payload = data.to_vec();
        if self.chance(self.config.corrupt) && !payload.is_empty() {
            let index = self.rng.usize(..payload.len());
            let bit = 1u8 << self.rng.usize(..8);
            payload[index] ^= bit;
        }

        let extra_reorder = if self.chance(self.config.reorder) {
            self.config.reorder_window
        } else {
            Duration::ZERO
        };

        let due = now + self.sample_delay() + extra_reorder;
        let _ = tx.send(ScheduledPacket {
            due,
            payload: payload.clone(),
            target: target.clone(),
        });

        if self.chance(self.config.duplicate) {
            let duplicate_due = due + self.sample_delay().min(Duration::from_millis(5));
            let _ = tx.send(ScheduledPacket {
                due: duplicate_due,
                payload,
                target,
            });
        }
    }

    fn sample_delay(&mut self) -> Duration {
        if self.config.max_delay <= self.config.min_delay {
            return self.config.min_delay;
        }
        let span = self.config.max_delay - self.config.min_delay;
        self.config.min_delay + Duration::from_nanos(self.rng.u64(..span.as_nanos() as u64 + 1))
    }

    fn chance(&mut self, probability: f64) -> bool {
        probability > 0.0 && (probability >= 1.0 || self.rng.f64() < probability)
    }
}

#[derive(Clone)]
enum PacketTarget {
    Connected(Arc<UdpSocket>),
    ToClient(Arc<UdpSocket>, SocketAddr),
}

struct ScheduledPacket {
    due: Instant,
    payload: Vec<u8>,
    target: PacketTarget,
}

struct ClientLink {
    server_socket: Arc<UdpSocket>,
    uplink: LinkState,
}

pub fn run(config: Config) -> io::Result<()> {
    let listener = Arc::new(UdpSocket::bind(config.listen_addr)?);
    listener.set_nonblocking(false)?;

    let (tx, rx) = mpsc::channel::<ScheduledPacket>();

    start_scheduler(rx);

    let mut clients = HashMap::new();
    let mut client_seed = config.seed.wrapping_add(1);

    println!(
        "network_emulator listening on {} and forwarding to {}",
        config.listen_addr, config.server_addr
    );
    println!(
        "uplink: loss={:.1}% delay={}..{}ms dup={:.1}% reorder={:.1}% corrupt={:.1}%",
        config.uplink.loss * 100.0,
        config.uplink.min_delay.as_millis(),
        config.uplink.max_delay.as_millis(),
        config.uplink.duplicate * 100.0,
        config.uplink.reorder * 100.0,
        config.uplink.corrupt * 100.0,
    );
    println!(
        "downlink: loss={:.1}% delay={}..{}ms dup={:.1}% reorder={:.1}% corrupt={:.1}%",
        config.downlink.loss * 100.0,
        config.downlink.min_delay.as_millis(),
        config.downlink.max_delay.as_millis(),
        config.downlink.duplicate * 100.0,
        config.downlink.reorder * 100.0,
        config.downlink.corrupt * 100.0,
    );

    let mut buf = vec![0u8; config.buf_size];
    loop {
        let (count, client_addr) = listener.recv_from(&mut buf)?;
        let client = get_or_create_client(
            client_addr,
            &mut clients,
            &listener,
            &tx,
            &config,
            &mut client_seed,
        )?;

        let now = Instant::now();
        client.uplink.schedule(
            &buf[..count],
            now,
            PacketTarget::Connected(Arc::clone(&client.server_socket)),
            &tx,
        );
    }
}

fn get_or_create_client<'a>(
    client_addr: SocketAddr,
    clients: &'a mut HashMap<SocketAddr, ClientLink>,
    listener: &Arc<UdpSocket>,
    tx: &mpsc::Sender<ScheduledPacket>,
    config: &Config,
    client_seed: &mut u64,
) -> io::Result<&'a mut ClientLink> {
    if clients.contains_key(&client_addr) {
        return Ok(clients.get_mut(&client_addr).unwrap());
    }

    let server_socket = Arc::new(UdpSocket::bind(
        config
            .server_bind_addr
            .unwrap_or_else(|| wildcard_bind_addr(config.server_addr)),
    )?);
    server_socket.connect(config.server_addr)?;
    let seed = *client_seed;
    *client_seed = client_seed.wrapping_add(2);
    let client = ClientLink {
        server_socket: Arc::clone(&server_socket),
        uplink: LinkState::new(seed, config.uplink.clone()),
    };
    spawn_downlink_thread(
        server_socket,
        Arc::clone(listener),
        client_addr,
        tx.clone(),
        config.downlink.clone(),
        seed ^ 0x9E37_79B9_7F4A_7C15,
        config.buf_size,
    );
    println!("new emulated client: {}", client_addr);
    Ok(clients.entry(client_addr).or_insert(client))
}

fn wildcard_bind_addr(server_addr: SocketAddr) -> SocketAddr {
    match server_addr {
        SocketAddr::V4(_) => SocketAddr::from(([0, 0, 0, 0], 0)),
        SocketAddr::V6(_) => SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 0)),
    }
}

fn spawn_downlink_thread(
    server_socket: Arc<UdpSocket>,
    listener: Arc<UdpSocket>,
    client_addr: SocketAddr,
    tx: mpsc::Sender<ScheduledPacket>,
    config: DirectionConfig,
    seed: u64,
    buf_size: usize,
) {
    thread::spawn(move || {
        let mut buf = vec![0u8; buf_size];
        let mut downlink = LinkState::new(seed, config);
        loop {
            let count = match server_socket.recv(&mut buf) {
                Ok(count) => count,
                Err(err) => {
                    eprintln!("downlink recv error for {}: {}", client_addr, err);
                    return;
                }
            };
            downlink.schedule(
                &buf[..count],
                Instant::now(),
                PacketTarget::ToClient(Arc::clone(&listener), client_addr),
                &tx,
            );
        }
    });
}

fn start_scheduler(rx: mpsc::Receiver<ScheduledPacket>) {
    thread::spawn(move || {
        let mut packets = Vec::new();

        loop {
            packets.sort_by_key(|packet: &ScheduledPacket| packet.due);
            while packets
                .first()
                .is_some_and(|packet| packet.due <= Instant::now())
            {
                let packet = packets.remove(0);
                if let Err(err) = match packet.target {
                    PacketTarget::Connected(socket) => socket.send(&packet.payload).map(|_| ()),
                    PacketTarget::ToClient(socket, addr) => {
                        socket.send_to(&packet.payload, addr).map(|_| ())
                    }
                } {
                    eprintln!("scheduled send error: {}", err);
                }
            }

            if let Some(packet) = packets.first() {
                let timeout = packet.due.saturating_duration_since(Instant::now());
                match rx.recv_timeout(timeout) {
                    Ok(packet) => packets.push(packet),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                let Ok(packet) = rx.recv() else { return };
                packets.push(packet);
            }
        }
    });
}

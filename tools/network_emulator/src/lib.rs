use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    io,
    net::{SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering as AtomicOrdering},
        mpsc,
    },
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
    pub buf_size: usize,
    pub seed: u64,
    pub stats_interval: Duration,
    pub uplink: DirectionConfig,
    pub downlink: DirectionConfig,
}

#[derive(Default)]
struct Stats {
    client_packets: AtomicU64,
    server_packets: AtomicU64,
    client_bytes: AtomicU64,
    server_bytes: AtomicU64,
    sent_to_server: AtomicU64,
    sent_to_client: AtomicU64,
    dropped_up: AtomicU64,
    dropped_down: AtomicU64,
    duplicated_up: AtomicU64,
    duplicated_down: AtomicU64,
    corrupted_up: AtomicU64,
    corrupted_down: AtomicU64,
    reordered_up: AtomicU64,
    reordered_down: AtomicU64,
}

#[derive(Clone, Copy)]
enum Direction {
    Uplink,
    Downlink,
}

struct LinkState {
    rng: Rng64,
    config: DirectionConfig,
}

impl LinkState {
    fn new(seed: u64, config: DirectionConfig) -> Self {
        Self {
            rng: Rng64::new(seed),
            config,
        }
    }

    fn schedule(
        &mut self,
        data: &[u8],
        now: Instant,
        direction: Direction,
        target: PacketTarget,
        tx: &mpsc::Sender<ScheduledPacket>,
        stats: &Stats,
    ) {
        if self.rng.chance(self.config.loss) {
            match direction {
                Direction::Uplink => {
                    stats.dropped_up.fetch_add(1, AtomicOrdering::Relaxed);
                }
                Direction::Downlink => {
                    stats.dropped_down.fetch_add(1, AtomicOrdering::Relaxed);
                }
            }
            return;
        }

        let mut payload = data.to_vec();
        if self.rng.chance(self.config.corrupt) && !payload.is_empty() {
            let index = self.rng.index(payload.len());
            let bit = 1u8 << self.rng.index(8);
            payload[index] ^= bit;
            match direction {
                Direction::Uplink => {
                    stats.corrupted_up.fetch_add(1, AtomicOrdering::Relaxed);
                }
                Direction::Downlink => {
                    stats.corrupted_down.fetch_add(1, AtomicOrdering::Relaxed);
                }
            }
        }

        let extra_reorder = if self.rng.chance(self.config.reorder) {
            match direction {
                Direction::Uplink => {
                    stats.reordered_up.fetch_add(1, AtomicOrdering::Relaxed);
                }
                Direction::Downlink => {
                    stats.reordered_down.fetch_add(1, AtomicOrdering::Relaxed);
                }
            }
            self.config.reorder_window
        } else {
            Duration::ZERO
        };

        let due = now + self.sample_delay() + extra_reorder;
        let _ = tx.send(ScheduledPacket {
            due,
            sequence: 0,
            payload: payload.clone(),
            target: target.clone(),
            direction,
        });

        if self.rng.chance(self.config.duplicate) {
            let duplicate_due = due + self.sample_delay().min(Duration::from_millis(5));
            let _ = tx.send(ScheduledPacket {
                due: duplicate_due,
                sequence: 0,
                payload,
                target,
                direction,
            });
            match direction {
                Direction::Uplink => {
                    stats.duplicated_up.fetch_add(1, AtomicOrdering::Relaxed);
                }
                Direction::Downlink => {
                    stats.duplicated_down.fetch_add(1, AtomicOrdering::Relaxed);
                }
            }
        }
    }

    fn sample_delay(&mut self) -> Duration {
        if self.config.max_delay <= self.config.min_delay {
            return self.config.min_delay;
        }
        let span = self.config.max_delay - self.config.min_delay;
        self.config.min_delay + Duration::from_nanos(self.rng.range_u64(span.as_nanos() as u64 + 1))
    }
}

#[derive(Clone)]
enum PacketTarget {
    Connected(Arc<UdpSocket>),
    ToClient(Arc<UdpSocket>, SocketAddr),
}

struct ScheduledPacket {
    due: Instant,
    sequence: u64,
    payload: Vec<u8>,
    target: PacketTarget,
    direction: Direction,
}

impl PartialEq for ScheduledPacket {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.sequence == other.sequence
    }
}

impl Eq for ScheduledPacket {}

impl PartialOrd for ScheduledPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .due
            .cmp(&self.due)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

struct ClientLink {
    server_socket: Arc<UdpSocket>,
    uplink: Mutex<LinkState>,
}

pub fn run(config: Config) -> io::Result<()> {
    let listener = Arc::new(UdpSocket::bind(config.listen_addr)?);
    listener.set_nonblocking(false)?;

    let stats = Arc::new(Stats::default());
    let (tx, rx) = mpsc::channel::<ScheduledPacket>();

    start_scheduler(rx, Arc::clone(&stats));
    start_stats_logger(Arc::clone(&stats), config.stats_interval);

    let clients: Arc<Mutex<HashMap<SocketAddr, Arc<ClientLink>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let client_seed = AtomicU64::new(config.seed.wrapping_add(1));

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
        stats.client_packets.fetch_add(1, AtomicOrdering::Relaxed);
        stats
            .client_bytes
            .fetch_add(count as u64, AtomicOrdering::Relaxed);

        let client = get_or_create_client(
            client_addr,
            &clients,
            &listener,
            &tx,
            &stats,
            &config,
            &client_seed,
        )?;

        let now = Instant::now();
        let mut uplink = client
            .uplink
            .lock()
            .map_err(|_| io::Error::other("uplink mutex poisoned"))?;
        uplink.schedule(
            &buf[..count],
            now,
            Direction::Uplink,
            PacketTarget::Connected(Arc::clone(&client.server_socket)),
            &tx,
            &stats,
        );
    }
}

fn get_or_create_client(
    client_addr: SocketAddr,
    clients: &Arc<Mutex<HashMap<SocketAddr, Arc<ClientLink>>>>,
    listener: &Arc<UdpSocket>,
    tx: &mpsc::Sender<ScheduledPacket>,
    stats: &Arc<Stats>,
    config: &Config,
    client_seed: &AtomicU64,
) -> io::Result<Arc<ClientLink>> {
    if let Some(existing) = clients
        .lock()
        .map_err(|_| io::Error::other("clients mutex poisoned"))?
        .get(&client_addr)
        .cloned()
    {
        return Ok(existing);
    }

    let server_socket = Arc::new(UdpSocket::bind(local_bind_addr(config.server_addr))?);
    server_socket.connect(config.server_addr)?;
    let seed = client_seed.fetch_add(2, AtomicOrdering::Relaxed);
    let client = Arc::new(ClientLink {
        server_socket: Arc::clone(&server_socket),
        uplink: Mutex::new(LinkState::new(seed, config.uplink.clone())),
    });

    let mut guard = clients
        .lock()
        .map_err(|_| io::Error::other("clients mutex poisoned"))?;
    let entry = guard.entry(client_addr).or_insert_with(|| {
        spawn_downlink_thread(
            Arc::clone(&server_socket),
            Arc::clone(listener),
            client_addr,
            tx.clone(),
            Arc::clone(stats),
            config.downlink.clone(),
            seed ^ 0x9E37_79B9_7F4A_7C15,
            config.buf_size,
        );
        println!("new emulated client: {}", client_addr);
        Arc::clone(&client)
    });
    Ok(Arc::clone(entry))
}

fn local_bind_addr(server_addr: SocketAddr) -> SocketAddr {
    match server_addr {
        SocketAddr::V4(_) => SocketAddr::from(([127, 0, 0, 1], 0)),
        SocketAddr::V6(_) => SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 0)),
    }
}

fn spawn_downlink_thread(
    server_socket: Arc<UdpSocket>,
    listener: Arc<UdpSocket>,
    client_addr: SocketAddr,
    tx: mpsc::Sender<ScheduledPacket>,
    stats: Arc<Stats>,
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
            stats.server_packets.fetch_add(1, AtomicOrdering::Relaxed);
            stats
                .server_bytes
                .fetch_add(count as u64, AtomicOrdering::Relaxed);
            downlink.schedule(
                &buf[..count],
                Instant::now(),
                Direction::Downlink,
                PacketTarget::ToClient(Arc::clone(&listener), client_addr),
                &tx,
                &stats,
            );
        }
    });
}

fn start_scheduler(rx: mpsc::Receiver<ScheduledPacket>, stats: Arc<Stats>) {
    thread::spawn(move || {
        let mut heap: BinaryHeap<ScheduledPacket> = BinaryHeap::new();
        let mut sequence = 0u64;
        let mut disconnected = false;

        loop {
            while let Some(packet) = heap.peek() {
                let now = Instant::now();
                if packet.due > now {
                    break;
                }

                let Some(packet) = heap.pop() else {
                    break;
                };
                let result = match packet.target {
                    PacketTarget::Connected(socket) => socket.send(&packet.payload).map(|_| ()),
                    PacketTarget::ToClient(socket, addr) => {
                        socket.send_to(&packet.payload, addr).map(|_| ())
                    }
                };
                if let Err(err) = result {
                    eprintln!("scheduled send error: {}", err);
                } else {
                    match packet.direction {
                        Direction::Uplink => {
                            stats.sent_to_server.fetch_add(1, AtomicOrdering::Relaxed);
                        }
                        Direction::Downlink => {
                            stats.sent_to_client.fetch_add(1, AtomicOrdering::Relaxed);
                        }
                    }
                }
            }

            if disconnected && heap.is_empty() {
                return;
            }

            match heap.peek() {
                Some(packet) => {
                    let timeout = packet.due.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(timeout) {
                        Ok(mut packet) => {
                            packet.sequence = sequence;
                            sequence = sequence.wrapping_add(1);
                            heap.push(packet);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
                    }
                }
                None => match rx.recv() {
                    Ok(mut packet) => {
                        packet.sequence = sequence;
                        sequence = sequence.wrapping_add(1);
                        heap.push(packet);
                    }
                    Err(_) => return,
                },
            }
        }
    });
}

fn start_stats_logger(stats: Arc<Stats>, interval: Duration) {
    if interval.is_zero() {
        return;
    }

    thread::spawn(move || {
        loop {
            thread::sleep(interval);
            println!(
                "stats: client_rx={} server_rx={} sent_up={} sent_down={} drop_up={} drop_down={} dup_up={} dup_down={} corrupt_up={} corrupt_down={} reorder_up={} reorder_down={}",
                stats.client_packets.load(AtomicOrdering::Relaxed),
                stats.server_packets.load(AtomicOrdering::Relaxed),
                stats.sent_to_server.load(AtomicOrdering::Relaxed),
                stats.sent_to_client.load(AtomicOrdering::Relaxed),
                stats.dropped_up.load(AtomicOrdering::Relaxed),
                stats.dropped_down.load(AtomicOrdering::Relaxed),
                stats.duplicated_up.load(AtomicOrdering::Relaxed),
                stats.duplicated_down.load(AtomicOrdering::Relaxed),
                stats.corrupted_up.load(AtomicOrdering::Relaxed),
                stats.corrupted_down.load(AtomicOrdering::Relaxed),
                stats.reordered_up.load(AtomicOrdering::Relaxed),
                stats.reordered_down.load(AtomicOrdering::Relaxed),
            );
        }
    });
}

#[derive(Clone, Debug)]
struct Rng64 {
    state: u64,
}

impl Rng64 {
    fn new(seed: u64) -> Self {
        let mut state = seed;
        if state == 0 {
            state = 0xA076_1D64_78BD_642F;
        }
        Self { state }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_f64(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        bits as f64 / ((1u64 << 53) as f64)
    }

    fn chance(&mut self, probability: f64) -> bool {
        if probability <= 0.0 {
            return false;
        }
        if probability >= 1.0 {
            return true;
        }
        self.next_f64() < probability
    }

    fn range_u64(&mut self, upper_exclusive: u64) -> u64 {
        if upper_exclusive <= 1 {
            return 0;
        }
        self.next_u64() % upper_exclusive
    }

    fn index(&mut self, upper_exclusive: usize) -> usize {
        self.range_u64(upper_exclusive as u64) as usize
    }
}

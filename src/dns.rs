//! Reverse DNS of the addresses in the table (#5): IP → host name, like "Resolve addresses" of
//! TCPView.
//!
//! * [`DnsCache`] – pure: positive and negative results with a time to live, and which addresses
//!   are being looked up (tested without threads or network).
//! * [`Resolver`] – the cache plus a small pool of worker threads that run the lookups, so the UI
//!   thread never waits for DNS. At most [`WORKERS`] lookups run at the same time; the UI asks
//!   with [`Resolver::name`] (which queues a missing address) and takes the results with
//!   [`Resolver::receive`] once per frame.
//! * [`system_lookup`] – `GetNameInfoW` (Windows) / `getnameinfo` (Linux) with `NI_NAMEREQD`, so
//!   an address without a name is a negative result, not its own text.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Condvar, Mutex, mpsc};

/// Parallel lookups. An address without a PTR record can take seconds (the system resolver
/// tries every DNS server and suffix), so several run at once.
pub const WORKERS: usize = 8;
/// How long a found name is kept (ms).
pub const TTL_MS: u64 = 10 * 60 * 1000;
/// How long "no name" is kept before the address is asked again (ms).
pub const NEGATIVE_TTL_MS: u64 = 5 * 60 * 1000;
/// Above this many entries the expired ones are dropped.
const MAX_ENTRIES: usize = 20_000;

#[derive(Clone, Debug, Default)]
struct Entry {
    /// The last result; `None` = no name (or not known yet).
    name: Option<Arc<str>>,
    /// When the last result came (ms); `None` = never.
    resolved_at: Option<u64>,
    /// A lookup is queued or running.
    pending: bool,
}

/// Results of reverse lookups with their age.
#[derive(Default)]
pub struct DnsCache {
    map: HashMap<IpAddr, Entry>,
}

impl DnsCache {
    /// The known name of `ip` (an expired one is still returned while it is looked up again) and
    /// whether a lookup must be started now (the address is then marked pending).
    pub fn get(&mut self, ip: IpAddr, now: u64) -> (Option<Arc<str>>, bool) {
        if !resolvable(ip) {
            return (None, false);
        }
        let e = self.map.entry(ip).or_default();
        let fresh = e.resolved_at.is_some_and(|at| {
            let ttl = if e.name.is_some() {
                TTL_MS
            } else {
                NEGATIVE_TTL_MS
            };
            now.saturating_sub(at) < ttl
        });
        let start = !fresh && !e.pending;
        if start {
            e.pending = true;
        }
        (e.name.clone(), start)
    }

    /// Stores the result of a lookup.
    pub fn insert(&mut self, ip: IpAddr, name: Option<String>, now: u64) {
        if self.map.len() >= MAX_ENTRIES {
            self.map.retain(|_, e| {
                e.pending
                    || e.resolved_at
                        .is_some_and(|at| now.saturating_sub(at) < TTL_MS)
            });
        }
        let e = self.map.entry(ip).or_default();
        e.name = name.filter(|n| !n.is_empty()).map(Arc::from);
        e.resolved_at = Some(now);
        e.pending = false;
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// Addresses worth asking about: not 0.0.0.0 / `::`.
pub fn resolvable(ip: IpAddr) -> bool {
    !ip.is_unspecified()
}

/// A function that resolves one address (the system resolver, or a fake one in tests).
pub type LookupFn = Arc<dyn Fn(IpAddr) -> Option<String> + Send + Sync>;

type Queue = Arc<(Mutex<VecDeque<IpAddr>>, Condvar)>;
type ResultTx = mpsc::Sender<(IpAddr, Option<String>)>;
type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// The cache plus the worker threads.
pub struct Resolver {
    cache: DnsCache,
    queue: Queue,
    results: mpsc::Receiver<(IpAddr, Option<String>)>,
    /// Kept to start the workers lazily (no threads until the first lookup).
    start: Option<(LookupFn, ResultTx, WakeFn)>,
}

impl Resolver {
    /// `wake` is called after every finished lookup (the UI's repaint request).
    pub fn new(lookup: LookupFn, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cache: DnsCache::default(),
            queue: Arc::new((Mutex::new(VecDeque::new()), Condvar::new())),
            results: rx,
            start: Some((lookup, tx, wake)),
        }
    }

    /// The name of `ip`, if known; queues a lookup when there is no fresh result.
    pub fn name(&mut self, ip: IpAddr, now: u64) -> Option<Arc<str>> {
        let (name, start) = self.cache.get(ip, now);
        if start {
            self.spawn_workers();
            let (q, cv) = &*self.queue;
            q.lock().unwrap_or_else(|e| e.into_inner()).push_back(ip);
            cv.notify_one();
        }
        name
    }

    /// Takes the finished lookups into the cache; true when there were any (the view changes).
    pub fn receive(&mut self, now: u64) -> bool {
        let mut any = false;
        while let Ok((ip, name)) = self.results.try_recv() {
            self.cache.insert(ip, name, now);
            any = true;
        }
        any
    }

    /// Lookups queued or running.
    pub fn pending(&self) -> usize {
        self.cache.map.values().filter(|e| e.pending).count()
    }

    fn spawn_workers(&mut self) {
        let Some((lookup, tx, wake)) = self.start.take() else {
            return;
        };
        for n in 0..WORKERS {
            let (lookup, tx, wake, queue) =
                (lookup.clone(), tx.clone(), wake.clone(), self.queue.clone());
            let _ = std::thread::Builder::new()
                .name(format!("dns-{n}"))
                .spawn(move || worker(&queue, &*lookup, &tx, &*wake));
        }
    }
}

fn worker(
    queue: &Queue,
    lookup: &(dyn Fn(IpAddr) -> Option<String> + Send + Sync),
    tx: &mpsc::Sender<(IpAddr, Option<String>)>,
    wake: &(dyn Fn() + Send + Sync),
) {
    loop {
        let ip = {
            let (q, cv) = &**queue;
            let mut q = q.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(ip) = q.pop_front() {
                    break ip;
                }
                q = cv.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let name = lookup(ip);
        if tx.send((ip, name)).is_err() {
            return; // the UI is gone
        }
        wake();
    }
}

/// The resolver of the operating system: the host name of `ip`, `None` when it has none.
pub fn system_lookup(ip: IpAddr) -> Option<String> {
    let (buf, len) = sockaddr(ip);
    let name = platform::name_info(&buf[..len])?;
    // A resolver that returns the address itself has no name for it.
    (name.parse::<IpAddr>().is_err()).then_some(name)
}

/// `sockaddr_in` / `sockaddr_in6` bytes (the same layout on Windows and Linux; only the value of
/// AF_INET6 differs). The port is 0 – only the host part is asked.
fn sockaddr(ip: IpAddr) -> ([u8; 28], usize) {
    let mut b = [0u8; 28];
    match ip {
        IpAddr::V4(v4) => {
            b[0..2].copy_from_slice(&platform::AF_INET.to_ne_bytes());
            b[4..8].copy_from_slice(&v4.octets());
            (b, 16)
        }
        IpAddr::V6(v6) => {
            b[0..2].copy_from_slice(&platform::AF_INET6.to_ne_bytes());
            b[8..24].copy_from_slice(&v6.octets());
            (b, 28)
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::sync::Once;

    pub const AF_INET: u16 = 2;
    pub const AF_INET6: u16 = 23;
    const NI_NAMEREQD: i32 = 0x04;
    const NI_MAXHOST: usize = 1025;

    #[link(name = "ws2_32")]
    unsafe extern "system" {
        fn WSAStartup(version: u16, data: *mut u8) -> i32;
        fn GetNameInfoW(
            sa: *const u8,
            sa_len: i32,
            host: *mut u16,
            host_len: u32,
            serv: *mut u16,
            serv_len: u32,
            flags: i32,
        ) -> i32;
    }

    static INIT: Once = Once::new();

    pub fn name_info(sa: &[u8]) -> Option<String> {
        INIT.call_once(|| {
            // WSADATA is 408 bytes on x64; Winsock stays initialized for the whole program.
            let mut data = [0u8; 512];
            // SAFETY: `data` is larger than WSADATA; WSAStartup only writes into it.
            unsafe {
                WSAStartup(0x0202, data.as_mut_ptr());
            }
        });
        let mut host = vec![0u16; NI_MAXHOST];
        // SAFETY: `sa` holds a complete sockaddr_in / sockaddr_in6 of `sa.len()` bytes, `host`
        // is valid for NI_MAXHOST UTF-16 units, no service buffer is requested.
        let rc = unsafe {
            GetNameInfoW(
                sa.as_ptr(),
                sa.len() as i32,
                host.as_mut_ptr(),
                host.len() as u32,
                std::ptr::null_mut(),
                0,
                NI_NAMEREQD,
            )
        };
        if rc != 0 {
            return None;
        }
        let end = host.iter().position(|&c| c == 0).unwrap_or(host.len());
        Some(String::from_utf16_lossy(&host[..end]))
    }
}

#[cfg(not(windows))]
mod platform {
    pub const AF_INET: u16 = 2;
    pub const AF_INET6: u16 = 10;
    const NI_NAMEREQD: i32 = 8;
    const NI_MAXHOST: usize = 1025;

    unsafe extern "C" {
        fn getnameinfo(
            sa: *const u8,
            sa_len: u32,
            host: *mut u8,
            host_len: u32,
            serv: *mut u8,
            serv_len: u32,
            flags: i32,
        ) -> i32;
    }

    pub fn name_info(sa: &[u8]) -> Option<String> {
        let mut host = vec![0u8; NI_MAXHOST];
        // SAFETY: `sa` holds a complete sockaddr_in / sockaddr_in6 of `sa.len()` bytes, `host`
        // is valid for NI_MAXHOST bytes, no service buffer is requested.
        let rc = unsafe {
            getnameinfo(
                sa.as_ptr(),
                sa.len() as u32,
                host.as_mut_ptr(),
                host.len() as u32,
                std::ptr::null_mut(),
                0,
                NI_NAMEREQD,
            )
        };
        if rc != 0 {
            return None;
        }
        let end = host.iter().position(|&c| c == 0).unwrap_or(host.len());
        Some(String::from_utf8_lossy(&host[..end]).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn cache_ttl_and_pending() {
        let mut c = DnsCache::default();
        let a = ip("10.0.0.1");
        assert_eq!(c.get(a, 0), (None, true));
        // Asked again while pending: no second lookup.
        assert_eq!(c.get(a, 10), (None, false));
        c.insert(a, Some("host.example".into()), 100);
        assert_eq!(c.get(a, 200).0.as_deref(), Some("host.example"));
        assert!(!c.get(a, 200).1);
        // Expired: the old name stays while the address is asked again.
        let (name, start) = c.get(a, 100 + TTL_MS);
        assert_eq!((name.as_deref(), start), (Some("host.example"), true));
    }

    #[test]
    fn negative_results_expire_sooner() {
        let mut c = DnsCache::default();
        let a = ip("192.0.2.7");
        c.get(a, 0);
        c.insert(a, None, 0);
        assert_eq!(c.get(a, NEGATIVE_TTL_MS - 1), (None, false));
        assert_eq!(c.get(a, NEGATIVE_TTL_MS), (None, true));
        // An empty answer counts as no name.
        c.insert(a, Some(String::new()), 0);
        assert_eq!(c.get(a, 1).0, None);
    }

    #[test]
    fn unspecified_is_never_asked() {
        let mut c = DnsCache::default();
        assert_eq!(c.get(ip("0.0.0.0"), 0), (None, false));
        assert_eq!(c.get(ip("::"), 0), (None, false));
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn sockaddr_layout() {
        let (b, n) = sockaddr(ip("192.168.1.2"));
        assert_eq!(n, 16);
        assert_eq!(u16::from_ne_bytes([b[0], b[1]]), platform::AF_INET);
        assert_eq!(&b[4..8], &[192, 168, 1, 2]);
        let (b, n) = sockaddr(ip("2001:db8::1"));
        assert_eq!(n, 28);
        assert_eq!(u16::from_ne_bytes([b[0], b[1]]), platform::AF_INET6);
        assert_eq!(b[8], 0x20);
        assert_eq!(b[23], 1);
    }

    /// Hundreds of addresses through the worker pool with a slow fake resolver: every address
    /// is asked once, never more than WORKERS at a time, and the caller never blocks.
    #[test]
    fn pool_resolves_many_addresses_in_parallel() {
        let calls = Arc::new(AtomicUsize::new(0));
        let running = Arc::new(AtomicUsize::new(0));
        let max_running = Arc::new(AtomicUsize::new(0));
        let (c, r, m) = (calls.clone(), running.clone(), max_running.clone());
        let lookup: LookupFn = Arc::new(move |ip| {
            c.fetch_add(1, Ordering::SeqCst);
            let now = r.fetch_add(1, Ordering::SeqCst) + 1;
            m.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(2));
            r.fetch_sub(1, Ordering::SeqCst);
            match ip {
                IpAddr::V4(v4) if v4.octets()[3] % 2 == 0 => Some(format!("h{}", v4.octets()[3])),
                _ => None,
            }
        });
        let mut res = Resolver::new(lookup, Arc::new(|| {}));
        let addrs: Vec<IpAddr> = (0..300u32)
            .map(|i| IpAddr::V4((0x0A00_0000 + i).into()))
            .collect();
        let t = Instant::now();
        for &a in &addrs {
            assert_eq!(res.name(a, 0), None);
        }
        // Queuing 300 addresses does not wait for any lookup.
        assert!(
            t.elapsed() < Duration::from_millis(200),
            "{:?}",
            t.elapsed()
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        while res.pending() > 0 && Instant::now() < deadline {
            res.receive(1);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(res.pending(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 300);
        assert!(max_running.load(Ordering::SeqCst) <= WORKERS);
        assert_eq!(res.name(ip("10.0.0.2"), 2).as_deref(), Some("h2"));
        assert_eq!(res.name(ip("10.0.0.3"), 2), None);
        // Cached: no new lookups.
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::SeqCst), 300);
    }

    /// The system resolver answers for the loopback address (hosts file / built-in name).
    #[test]
    fn system_resolver_knows_loopback() {
        let name = system_lookup(ip("127.0.0.1"));
        assert!(name.as_deref().is_some_and(|n| !n.is_empty()), "{name:?}");
    }
}

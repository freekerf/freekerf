//! The machine driver: one I/O thread owns the protocol [`Host`], the open link
//! and the clock (ADR 0001: threads and `crossbeam-channel`, no async runtime).
//!
//! * [`Machine::call`] runs a closure on the host, on the I/O thread, and returns
//!   its result: every operation is serialized with the traffic.
//! * [`Machine::stop`] goes through a priority channel checked before anything
//!   else, so STOP (soft reset) is never stuck behind other requests.
//! * [`Machine::subscribe`] returns a channel with every [`MachineEvent`].
//!
//! A reader thread per link turns blocking reads into messages; the I/O thread
//! waits on commands, received bytes and the next protocol deadline.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded, select, unbounded};
use freekerf_protocol::{Clock, Event, Host, HostSnapshot, Output};
use freekerf_transport::{LinkWriter, Transport, TransportError};

/// The real monotonic clock.
#[derive(Debug, Clone)]
pub struct SystemClock(Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// What the driver reports.
#[derive(Debug, Clone, PartialEq)]
pub enum MachineEvent {
    /// A protocol event.
    Host(Event),
    /// The link opened.
    Connected {
        /// Transport description.
        transport: String,
    },
    /// The link closed.
    Disconnected {
        /// Closed by the user (else: lost or failed).
        user: bool,
    },
}

/// Errors of the driver.
#[derive(Debug, thiserror::Error)]
pub enum MachineError {
    /// The transport failed to open.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// The I/O thread is gone.
    #[error("the machine driver stopped")]
    Stopped,
    /// Already connected.
    #[error("already connected")]
    AlreadyConnected,
}

type Job = Box<dyn FnOnce(&mut Host, u64) + Send>;

enum Cmd {
    Connect(Box<dyn Transport>, Sender<Result<(), MachineError>>),
    Disconnect(Sender<()>),
    Call(Job),
    Shutdown,
}

enum Rx {
    Bytes(u64, Vec<u8>),
    Closed(u64),
}

/// Handle to the driver; cheap to clone, usable from any thread.
#[derive(Clone)]
pub struct Machine {
    inner: Arc<Inner>,
}

struct Inner {
    cmd: Sender<Cmd>,
    stop: Sender<()>,
    snapshot: Arc<Mutex<HostSnapshot>>,
    subscribers: Arc<Mutex<Vec<Sender<MachineEvent>>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for Machine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Machine")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Shutdown);
        if let Some(t) = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take() {
            let _ = t.join();
        }
    }
}

impl Machine {
    /// Starts the driver around `host`, with the given clock.
    #[must_use]
    pub fn start(host: Host, clock: Arc<dyn Clock>) -> Self {
        let (cmd_tx, cmd_rx) = unbounded();
        let (stop_tx, stop_rx) = unbounded();
        let snapshot = Arc::new(Mutex::new(host.snapshot()));
        let subscribers: Arc<Mutex<Vec<Sender<MachineEvent>>>> = Arc::new(Mutex::new(Vec::new()));
        let mut io = Io {
            host,
            clock,
            writer: None,
            rx: None,
            reader_stop: None,
            snapshot: Arc::clone(&snapshot),
            subscribers: Arc::clone(&subscribers),
            link_id: 0,
        };
        let thread = thread::Builder::new()
            .name("freekerf-machine".into())
            .spawn(move || io.run(&cmd_rx, &stop_rx))
            .ok();
        Self {
            inner: Arc::new(Inner {
                cmd: cmd_tx,
                stop: stop_tx,
                snapshot,
                subscribers,
                thread: Mutex::new(thread),
            }),
        }
    }

    /// Opens a link and starts talking to the machine.
    ///
    /// # Errors
    /// The transport failed to open, or a link is already open.
    pub fn connect(&self, transport: Box<dyn Transport>) -> Result<(), MachineError> {
        let (tx, rx) = bounded(1);
        self.inner
            .cmd
            .send(Cmd::Connect(transport, tx))
            .map_err(|_| MachineError::Stopped)?;
        rx.recv().map_err(|_| MachineError::Stopped)?
    }

    /// Closes the link (returns once it is closed).
    ///
    /// # Errors
    /// The driver stopped.
    pub fn disconnect(&self) -> Result<(), MachineError> {
        let (tx, rx) = bounded(1);
        self.inner
            .cmd
            .send(Cmd::Disconnect(tx))
            .map_err(|_| MachineError::Stopped)?;
        rx.recv().map_err(|_| MachineError::Stopped)
    }

    /// Runs `f` on the host, on the I/O thread, and returns its result.
    ///
    /// # Errors
    /// The driver stopped.
    pub fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Host, u64) -> R + Send + 'static,
    ) -> Result<R, MachineError> {
        let (tx, rx) = bounded(1);
        let job: Job = Box::new(move |h, now| {
            let _ = tx.send(f(h, now));
        });
        self.inner
            .cmd
            .send(Cmd::Call(job))
            .map_err(|_| MachineError::Stopped)?;
        rx.recv().map_err(|_| MachineError::Stopped)
    }

    /// STOP: soft reset through the priority channel. Never waits behind other
    /// requests; does nothing when not connected.
    pub fn stop(&self) {
        let _ = self.inner.stop.send(());
    }

    /// Latest state (updated after every protocol step).
    #[must_use]
    pub fn snapshot(&self) -> HostSnapshot {
        self.inner
            .snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Receives every event from now on.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<MachineEvent> {
        let (tx, rx) = unbounded();
        self.inner
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        rx
    }
}

struct Io {
    host: Host,
    clock: Arc<dyn Clock>,
    writer: Option<Box<dyn LinkWriter>>,
    rx: Option<Receiver<Rx>>,
    reader_stop: Option<Arc<AtomicBool>>,
    snapshot: Arc<Mutex<HostSnapshot>>,
    subscribers: Arc<Mutex<Vec<Sender<MachineEvent>>>>,
    link_id: u64,
}

impl Io {
    fn now(&self) -> u64 {
        self.clock.now_ms()
    }

    fn run(&mut self, cmd_rx: &Receiver<Cmd>, stop_rx: &Receiver<()>) {
        loop {
            // STOP first, always
            while stop_rx.try_recv().is_ok() {
                let now = self.now();
                let _ = self.host.soft_reset(now);
                self.flush();
            }
            let now = self.now();
            let wait = self
                .host
                .next_deadline()
                .map_or(50, |d| d.saturating_sub(now).clamp(1, 50));
            let never = crossbeam_channel::never();
            let rx = self.rx.clone().unwrap_or(never);
            select! {
                recv(stop_rx) -> _ => {
                    let now = self.now();
                    let _ = self.host.soft_reset(now);
                }
                recv(cmd_rx) -> cmd => match cmd {
                    Ok(Cmd::Connect(t, reply)) => {
                        let r = self.connect(t.as_ref());
                        let _ = reply.send(r);
                    }
                    Ok(Cmd::Disconnect(reply)) => {
                        self.close_link(true);
                        let _ = reply.send(());
                    }
                    Ok(Cmd::Call(job)) => {
                        let now = self.now();
                        job(&mut self.host, now);
                    }
                    Ok(Cmd::Shutdown) | Err(_) => {
                        self.close_link(true);
                        return;
                    }
                },
                recv(rx) -> msg => match msg {
                    Ok(Rx::Bytes(id, data)) if id == self.link_id => {
                        let now = self.now();
                        self.host.receive(&data, now);
                    }
                    Ok(Rx::Closed(id)) if id == self.link_id => self.close_link(false),
                    Ok(_) => {}
                    Err(_) => self.close_link(false),
                },
                default(Duration::from_millis(wait)) => {}
            }
            let now = self.now();
            self.host.tick(now);
            self.flush();
        }
    }

    fn connect(&mut self, t: &dyn Transport) -> Result<(), MachineError> {
        if self.writer.is_some() {
            return Err(MachineError::AlreadyConnected);
        }
        let now = self.now();
        let link = match t.open() {
            Ok(l) => l,
            Err(e) => {
                self.host.open_failed(now);
                self.flush();
                return Err(e.into());
            }
        };
        self.link_id += 1;
        let id = self.link_id;
        let (tx, rx) = unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let mut reader = link.reader;
        let spawned = thread::Builder::new()
            .name("freekerf-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while !flag.load(Ordering::SeqCst) {
                    match reader.read(&mut buf, Duration::from_millis(20)) {
                        Ok(0) => {}
                        Ok(n) => {
                            if tx.send(Rx::Bytes(id, buf[..n].to_vec())).is_err() {
                                return;
                            }
                        }
                        Err(_) => {
                            let _ = tx.send(Rx::Closed(id));
                            return;
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            self.host.open_failed(now);
            self.flush();
            return Err(MachineError::Transport(TransportError::Io(e.to_string())));
        }
        self.writer = Some(link.writer);
        self.rx = Some(rx);
        self.reader_stop = Some(stop);
        self.publish(&MachineEvent::Connected {
            transport: t.describe(),
        });
        self.host.open(now);
        self.flush();
        Ok(())
    }

    fn close_link(&mut self, user: bool) {
        if let Some(stop) = self.reader_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        let was_open = self.writer.take().is_some();
        self.rx = None;
        if was_open {
            let now = self.now();
            self.host.close(user, now);
            self.flush();
            self.publish(&MachineEvent::Disconnected { user });
        }
    }

    /// Writes pending output, publishes events, updates the snapshot.
    fn flush(&mut self) {
        while let Some(out) = self.host.poll_output() {
            match out {
                Output::Write(bytes) => {
                    let failed = self.writer.as_mut().is_some_and(|w| w.write_all(&bytes).is_err());
                    if failed {
                        tracing::warn!("write failed: closing the link");
                        self.close_link(false);
                    }
                }
                Output::Close => self.close_link(false),
            }
        }
        while let Some(e) = self.host.poll_event() {
            self.publish(&MachineEvent::Host(e));
        }
        *self.snapshot.lock().unwrap_or_else(PoisonError::into_inner) = self.host.snapshot();
    }

    fn publish(&self, e: &MachineEvent) {
        log_event(e);
        let mut subs = self.subscribers.lock().unwrap_or_else(PoisonError::into_inner);
        subs.retain(|s| s.send(e.clone()).is_ok());
    }
}

/// Logs what the machine did: link and state changes and problems at `info` and
/// `warn`, every console row (commands sent, answers, board messages) at `debug`.
fn log_event(e: &MachineEvent) {
    use freekerf_protocol::ConsoleRow;
    match e {
        MachineEvent::Connected { transport } => tracing::info!("connected: {transport}"),
        MachineEvent::Disconnected { user } => tracing::info!(user, "disconnected"),
        MachineEvent::Host(Event::StatusChanged { old, new }) => {
            tracing::info!("status {old:?} -> {new:?}");
        }
        MachineEvent::Host(Event::Issue(issue)) => tracing::warn!("issue: {issue:?}"),
        MachineEvent::Host(Event::Console(ConsoleRow::Command { text, .. })) => {
            tracing::debug!("> {text}");
        }
        MachineEvent::Host(Event::Console(ConsoleRow::Result { result, decoded, .. })) => {
            tracing::debug!("< {result}{}", suffix(decoded.as_deref()));
        }
        MachineEvent::Host(Event::Console(ConsoleRow::Message {
            kind, text, decoded, ..
        })) => tracing::debug!("< {text}{} ({kind:?})", suffix(decoded.as_deref())),
        MachineEvent::Host(
            Event::OverridesChanged(_)
            | Event::ConsoleCleared
            | Event::SettingsChanged
            | Event::LoopCountChanged(_),
        ) => tracing::debug!("{e:?}"),
        MachineEvent::Host(other) => tracing::info!("{other:?}"),
    }
}

fn suffix(decoded: Option<&str>) -> String {
    decoded.map(|d| format!(": {d}")).unwrap_or_default()
}

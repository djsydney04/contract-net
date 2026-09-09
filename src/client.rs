//! Serial compute worker with responsive networking and reconnect-safe results.
use crate::{
    Rules, Settlement, Task,
    benchmark::{Rates, calibrate, overall_score},
    protocol::{Award, Incoming, InvalidBid, ManagerError, Outgoing, Registration, Rejection},
    strategy::{BidContext, Strategy},
};
use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::{HashMap, VecDeque},
    future::{Future, pending},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpStream,
    task::JoinHandle,
    time::{interval_at, sleep, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

const APP_PING_INTERVAL: Duration = Duration::from_secs(20);
const IO_TIMEOUT: Duration = Duration::from_secs(15);
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub name: String,
    pub url: String,
    pub token: Option<String>,
    pub machine: String,
    pub auto_calibrate: bool,
    pub verbose: bool,
}

impl ClientConfig {
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            token: None,
            machine: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            auto_calibrate: true,
            verbose: true,
        }
    }

    fn validate(&self) -> Result<()> {
        let bytes = self.name.as_bytes();
        ensure!(
            (2..=24).contains(&bytes.len())
                && bytes[0].is_ascii_alphanumeric()
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-'),
            "team name must be 2-24 ASCII letters, digits, underscores or hyphens, starting with a letter or digit"
        );
        ensure!(
            self.url.starts_with("ws://") || self.url.starts_with("wss://"),
            "URL must start with ws:// or wss://"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Pending,
    Awarded,
    Running,
    Complete,
}

struct Commitment {
    task: Task,
    compute_seconds: f64,
    started: Option<Instant>,
    stage: Stage,
}

struct WorkDone {
    task_id: u64,
    attempt: u64,
    message: Outgoing,
}
enum SessionEnd {
    Disconnected,
    DuplicateName,
    Rejected(String),
}

pub struct Contractor<S: Strategy> {
    pub config: ClientConfig,
    pub rules: Rules,
    pub rates: Rates,
    pub history: Vec<Settlement>,
    strategy: Arc<S>,
    commitments: HashMap<u64, Commitment>,
    queue: VecDeque<u64>,
    active: Option<JoinHandle<WorkDone>>,
    outbox: VecDeque<Outgoing>,
}

impl<S: Strategy> Contractor<S> {
    pub fn new(config: ClientConfig, strategy: S) -> Self {
        Self {
            config,
            rules: Rules::default(),
            rates: Rates::new(),
            history: Vec::new(),
            strategy: Arc::new(strategy),
            commitments: HashMap::new(),
            queue: VecDeque::new(),
            active: None,
            outbox: VecDeque::new(),
        }
    }

    pub fn queue_seconds(&self) -> f64 {
        self.commitments
            .values()
            .map(|c| match c.stage {
                Stage::Complete => 0.0,
                _ => (c.compute_seconds
                    - c.started.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0))
                .max(0.0),
            })
            .sum()
    }

    pub fn profit(&self) -> f64 {
        self.history.iter().map(|s| s.profit).sum()
    }

    fn context(&self) -> BidContext<'_> {
        BidContext {
            rules: &self.rules,
            rates: &self.rates,
            history: &self.history,
            queue_seconds: self.queue_seconds(),
        }
    }

    /// Reconnect until duplicate-name eviction, registration rejection, or caller cancellation.
    pub async fn run(&mut self) -> Result<()> {
        self.config.validate()?;
        if self.config.auto_calibrate {
            self.log(&format!("calibrating {}…", self.config.machine));
            let verbose = self.config.verbose;
            self.rates = tokio::task::spawn_blocking(move || calibrate(verbose)).await??;
            self.log(&format!(
                "benchmark score {:.0}",
                overall_score(&self.rates)
            ));
        }
        let mut backoff = 1;
        loop {
            let url = self.config.url.clone();
            let connection = self
                .wait_with_work(timeout(IO_TIMEOUT, connect_async(&url)))
                .await;
            match connection {
                Ok(Ok((socket, _))) => {
                    backoff = 1;
                    let result = self.session(socket).await;
                    // An old bid is no longer a commitment after disconnect. Awards survive.
                    self.commitments.retain(|_, c| c.stage != Stage::Pending);
                    match result {
                        Ok(SessionEnd::DuplicateName) => {
                            self.log("another process registered this team name; stopping");
                            return Ok(());
                        }
                        Ok(SessionEnd::Rejected(message)) => {
                            bail!("registration rejected: {message}")
                        }
                        Ok(SessionEnd::Disconnected) => self.log("connection closed"),
                        Err(err) => self.log(&format!("connection lost: {err}")),
                    }
                }
                Ok(Err(err)) => self.log(&format!("connection failed: {err}")),
                Err(_) => self.log("connection timed out"),
            }
            self.log(&format!("retrying in {backoff}s"));
            self.wait_with_work(sleep(Duration::from_secs(backoff)))
                .await;
            backoff = (backoff * 2).min(15);
        }
    }

    async fn wait_with_work<F: Future>(&mut self, future: F) -> F::Output {
        tokio::pin!(future);
        loop {
            tokio::select! {
                value = &mut future => return value,
                result = receive_work(&mut self.active) => self.finish_work(result),
            }
        }
    }

    async fn session(&mut self, mut socket: Socket) -> Result<SessionEnd> {
        let score = overall_score(&self.rates);
        send(
            &mut socket,
            &Outgoing::Register {
                name: self.config.name.clone(),
                token: self.config.token.clone(),
                machine: self.config.machine.clone(),
                benchmark: (!self.rates.is_empty()).then_some((score * 100.0).round() / 100.0),
            },
        )
        .await?;
        let mut registered = false;
        let mut last_pong = tokio::time::Instant::now();
        let registration_deadline = sleep(IO_TIMEOUT);
        tokio::pin!(registration_deadline);
        let mut ping = interval_at(
            tokio::time::Instant::now() + APP_PING_INTERVAL,
            APP_PING_INTERVAL,
        );
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = &mut registration_deadline, if !registered => bail!("registration timed out"),
                frame = socket.next() => {
                    match frame {
                        None => return Ok(SessionEnd::Disconnected),
                        Some(Err(err)) => return Err(err.into()),
                        Some(Ok(Message::Close(close))) => {
                            if close.is_some_and(|c| u16::from(c.code) == 4001) {
                                return Ok(SessionEnd::DuplicateName);
                            }
                            return Ok(SessionEnd::Disconnected);
                        }
                        Some(Ok(Message::Text(raw))) => {
                            if raw == "pong" {
                                last_pong = tokio::time::Instant::now();
                                continue;
                            }
                            let message = match Incoming::parse(&raw) {
                                Ok(message) => message,
                                Err(err) => { self.log(&format!("cannot decode manager message: {err:#}")); continue; }
                            };
                            if let Incoming::Error(ManagerError { code, message }) = &message
                                && matches!(code.as_str(), "bad_token" | "bad_name") {
                                    return Ok(SessionEnd::Rejected(format!("{code}: {message}")));
                                }
                            if matches!(message, Incoming::Registered(_)) { registered = true; }
                            if registered {
                                for response in self.dispatch(message) {
                                    // Bids are intentionally not replayed after a lost connection.
                                    send(&mut socket, &response).await?;
                                }
                            }
                        }
                        Some(Ok(Message::Ping(_))) => { timeout(IO_TIMEOUT, socket.flush()).await??; }
                        _ => {}
                    }
                }
                result = receive_work(&mut self.active) => self.finish_work(result),
                _ = ping.tick() => {
                    ensure!(last_pong.elapsed() < APP_PING_INTERVAL * 3, "manager keepalive timed out");
                    timeout(IO_TIMEOUT, socket.send(Message::Text("ping".into()))).await??;
                }
            }
            if registered {
                // Only remove results after a successful write; registration always comes first.
                while let Some(message) = self.outbox.front() {
                    send(&mut socket, message).await?;
                    self.outbox.pop_front();
                }
            }
        }
    }

    fn dispatch(&mut self, message: Incoming) -> Vec<Outgoing> {
        match message {
            Incoming::Registered(Registration {
                name,
                rules,
                open_cfps,
            }) => {
                self.rules = rules;
                self.log(&format!(
                    "registered as {name} (policy: {})",
                    self.rules.award_policy
                ));
                self.hook(|| self.strategy.on_registered(&self.context()));
                open_cfps
                    .into_iter()
                    .filter_map(|task| self.cfp(task))
                    .collect()
            }
            Incoming::Cfp(task) => self.cfp(task).into_iter().collect(),
            Incoming::AcceptProposal(Award { task_id }) => {
                if let Some(c) = self.commitments.get_mut(&task_id)
                    && c.stage == Stage::Pending
                {
                    c.stage = Stage::Awarded;
                    self.queue.push_back(task_id);
                    self.log(&format!("won #{task_id}"));
                    self.start_next();
                }
                Vec::new()
            }
            Incoming::RejectProposal(Rejection {
                task_id,
                winner,
                winning_price,
            }) => {
                self.remove_pending(task_id);
                self.hook(|| {
                    self.strategy
                        .on_reject(task_id, winner.as_deref(), winning_price)
                });
                Vec::new()
            }
            Incoming::BidInvalid(InvalidBid { task_id, reason }) => {
                self.remove_pending(task_id);
                self.log(&format!("bid on #{task_id} was rejected: {reason}"));
                self.hook(|| self.strategy.on_bid_invalid(task_id, &reason));
                Vec::new()
            }
            Incoming::Settled(mut settlement) => {
                if let Some(c) = self.commitments.remove(&settlement.task_id) {
                    settlement.task_type = c.task.task_type;
                }
                self.queue.retain(|&id| id != settlement.task_id);
                self.outbox
                    .retain(|msg| msg.task_id() != Some(settlement.task_id));
                self.history.push(settlement.clone());
                self.log(&format!(
                    "#{} {}: profit ${:+.2} | total ${:+.2}",
                    settlement.task_id,
                    settlement.verdict,
                    settlement.profit,
                    self.profit()
                ));
                self.hook(|| self.strategy.on_settled(&settlement));
                Vec::new()
            }
            Incoming::Error(ManagerError { code, message }) => {
                self.log(&format!("manager error [{code}]: {message}"));
                Vec::new()
            }
            Incoming::Unknown => Vec::new(),
        }
    }

    fn cfp(&mut self, task: Task) -> Option<Outgoing> {
        let id = task.task_id;
        if self
            .commitments
            .get(&id)
            .is_some_and(|c| c.stage != Stage::Pending)
        {
            return None;
        }
        self.commitments.remove(&id); // Replacement bids must not count themselves in the queue.
        let context = self.context();
        let compute = context.estimate(&task);
        let bid = catch_unwind(AssertUnwindSafe(|| self.strategy.on_cfp(&task, &context)))
            .ok()
            .flatten();
        if let Some(bid) = bid
            && bid.price.is_finite()
            && bid.price >= 0.0
            && bid.price <= task.budget
            && bid.est_seconds.is_finite()
            && bid.est_seconds >= 0.0
            && bid.est_seconds <= task.deadline_s
        {
            let duration = if compute.is_finite() {
                compute
            } else {
                (bid.est_seconds - context.queue_seconds).max(0.0)
            };
            self.commitments.insert(
                id,
                Commitment {
                    task,
                    compute_seconds: duration,
                    started: None,
                    stage: Stage::Pending,
                },
            );
            return Some(Outgoing::Propose {
                task_id: id,
                price: bid.price,
                est_seconds: bid.est_seconds,
            });
        }
        Some(Outgoing::Refuse { task_id: id })
    }

    fn remove_pending(&mut self, id: u64) {
        if self
            .commitments
            .get(&id)
            .is_some_and(|c| c.stage == Stage::Pending)
        {
            self.commitments.remove(&id);
        }
    }

    fn start_next(&mut self) {
        if self.active.is_some() {
            return;
        }
        while let Some(task_id) = self.queue.pop_front() {
            let Some(c) = self.commitments.get_mut(&task_id) else {
                continue;
            };
            c.stage = Stage::Running;
            c.started = Some(Instant::now());
            let task = c.task.clone();
            let strategy = Arc::clone(&self.strategy);
            self.active = Some(tokio::task::spawn_blocking(move || {
                let start = Instant::now();
                let result = catch_unwind(AssertUnwindSafe(|| strategy.execute(&task)));
                let message = match result {
                    Ok(Ok(answer)) => Outgoing::Inform {
                        task_id,
                        result: answer.to_string(),
                        runtime: start.elapsed().as_secs_f64(),
                    },
                    Ok(Err(err)) => Outgoing::Failure {
                        task_id,
                        reason: format!("{err:#}"),
                    },
                    Err(_) => Outgoing::Failure {
                        task_id,
                        reason: "executor panicked".into(),
                    },
                };
                WorkDone {
                    task_id,
                    attempt: task.attempt,
                    message,
                }
            }));
            break;
        }
    }

    fn finish_work(&mut self, result: std::result::Result<WorkDone, tokio::task::JoinError>) {
        self.active = None;
        match result {
            Ok(done) => {
                if let Some(c) = self.commitments.get_mut(&done.task_id)
                    && c.task.attempt == done.attempt
                {
                    c.stage = Stage::Complete;
                    self.outbox.push_back(done.message);
                }
            }
            Err(err) => self.log(&format!("worker stopped: {err}")),
        }
        self.start_next();
    }

    fn hook(&self, hook: impl FnOnce()) {
        if catch_unwind(AssertUnwindSafe(hook)).is_err() {
            self.log("strategy hook panicked; continuing");
        }
    }

    fn log(&self, message: &str) {
        if self.config.verbose {
            println!(
                "[{}] {}: {message}",
                chrono::Local::now().format("%H:%M:%S"),
                self.config.name
            );
        }
    }
}

async fn receive_work(
    active: &mut Option<JoinHandle<WorkDone>>,
) -> std::result::Result<WorkDone, tokio::task::JoinError> {
    match active.as_mut() {
        Some(handle) => handle.await,
        None => pending().await,
    }
}

async fn send(socket: &mut Socket, message: &Outgoing) -> Result<()> {
    timeout(
        IO_TIMEOUT,
        socket.send(Message::Text(serde_json::to_string(message)?.into())),
    )
    .await
    .context("WebSocket send timed out")??;
    Ok(())
}

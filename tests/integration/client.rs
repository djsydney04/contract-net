use anyhow::{Result, bail};
use contractnet::{
    Bid, BidContext, ClientConfig, Contractor, Rules, Settlement, Strategy, Task, benchmark::Rates,
};
use futures_util::{SinkExt, StreamExt};
use num_bigint::BigUint;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Notify,
    time::timeout,
};
use tokio_tungstenite::{
    WebSocketStream, accept_async,
    tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

type Server = WebSocketStream<TcpStream>;
const WAIT: Duration = Duration::from_secs(8);

#[derive(Default)]
struct Observations {
    executions: Mutex<Vec<u64>>,
    registered: AtomicUsize,
    rejected: AtomicUsize,
    invalid: AtomicUsize,
    settled: AtomicUsize,
    started: Notify,
}

struct TestStrategy {
    observations: Arc<Observations>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl Strategy for TestStrategy {
    fn on_cfp(&self, task: &Task, context: &BidContext<'_>) -> Option<Bid> {
        if task.task_type == "unknown" {
            return None;
        }
        let seconds = context.queue_seconds + context.estimate(task);
        (seconds <= task.deadline_s).then_some(Bid {
            price: context.rules.cost_rate * 0.5,
            est_seconds: seconds,
        })
    }

    fn execute(&self, task: &Task) -> Result<BigUint> {
        self.observations
            .executions
            .lock()
            .unwrap()
            .push(task.task_id);
        if task.task_id == 1 {
            self.observations.started.notify_one();
            self.release.lock().unwrap().recv_timeout(WAIT)?;
        }
        if task.task_id == 3 {
            bail!("intentional executor failure");
        }
        Ok("1785318802179667385".parse()?)
    }

    fn on_registered(&self, _: &BidContext<'_>) {
        self.observations.registered.fetch_add(1, Ordering::SeqCst);
    }
    fn on_reject(&self, _: u64, _: Option<&str>, _: Option<f64>) {
        self.observations.rejected.fetch_add(1, Ordering::SeqCst);
    }
    fn on_bid_invalid(&self, _: u64, _: &str) {
        self.observations.invalid.fetch_add(1, Ordering::SeqCst);
    }
    fn on_settled(&self, _: &Settlement) {
        self.observations.settled.fetch_add(1, Ordering::SeqCst);
    }
}

fn client(
    port: u16,
) -> (
    Contractor<TestStrategy>,
    Arc<Observations>,
    mpsc::Sender<()>,
) {
    let observations = Arc::new(Observations::default());
    let (release, receiver) = mpsc::channel();
    let mut config = ClientConfig::new("Rust_07", format!("ws://127.0.0.1:{port}/agent?room=test"));
    config.token = Some("test-token".into());
    config.verbose = false;
    config.auto_calibrate = false;
    let mut client = Contractor::new(
        config,
        TestStrategy {
            observations: observations.clone(),
            release: Mutex::new(receiver),
        },
    );
    client.rates = Rates::from([("monte_carlo_pi".into(), 1000.0)]);
    (client, observations, release)
}

fn cfp(id: u64) -> Value {
    json!({"type":"CFP","task_id":id,"task_type":"monte_carlo_pi","params":{"seed":1,"samples":1000},
        "budget":100,"deadline_s":30,"attempt":1})
}

async fn send(server: &mut Server, value: Value) {
    timeout(WAIT, server.send(Message::Text(value.to_string().into())))
        .await
        .unwrap()
        .unwrap();
}

async fn read(server: &mut Server) -> Value {
    timeout(WAIT, async {
        loop {
            match server.next().await.unwrap().unwrap() {
                Message::Text(text) if text == "ping" => {
                    server.send(Message::Text("pong".into())).await.unwrap()
                }
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(_) => server.flush().await.unwrap(),
                other => panic!("unexpected frame {other:?}"),
            }
        }
    })
    .await
    .expect("client must remain responsive")
}

async fn accept(listener: &TcpListener) -> Server {
    let (stream, _) = timeout(WAIT, listener.accept()).await.unwrap().unwrap();
    let mut server = accept_async(stream).await.unwrap();
    let registration = read(&mut server).await;
    assert_eq!(registration["type"], "REGISTER");
    assert_eq!(registration["name"], "Rust_07");
    assert_eq!(registration["token"], "test-token");
    server
}

async fn evict(server: &mut Server) {
    server
        .close(Some(CloseFrame {
            code: CloseCode::from(4001),
            reason: "duplicate name".into(),
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn full_protocol_keeps_networking_responsive_and_executes_awards_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, observations, release) = client(listener.local_addr().unwrap().port());
    let runner = tokio::spawn(async move {
        let result = client.run().await;
        (client, result)
    });
    let mut server = accept(&listener).await;
    send(&mut server, json!({"type":"REGISTERED","name":"Rust_07","rules":{"cost_rate":4,"future_rule":1},"open_cfps":[cfp(1)]})).await;
    let bid = read(&mut server).await;
    assert_eq!(bid["type"], "PROPOSE");
    assert_eq!(bid["price"], 2.0);
    assert_eq!(bid["est_seconds"], 1.0);
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":1})).await;
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":1})).await;
    timeout(WAIT, observations.started.notified())
        .await
        .unwrap();
    server.send(Message::Text("not json".into())).await.unwrap();
    server.send(Message::Text("pong".into())).await.unwrap();
    send(&mut server, json!({"type":"FUTURE_MESSAGE"})).await;
    send(&mut server, cfp(2)).await;
    let bid = read(&mut server).await;
    assert_eq!(bid["task_id"], 2);
    assert!(bid["est_seconds"].as_f64().unwrap() <= 2.0);
    send(
        &mut server,
        json!({"type":"REJECT_PROPOSAL","task_id":2,"winner":"Other","winning_price":1}),
    )
    .await;
    send(&mut server, cfp(4)).await;
    assert_eq!(read(&mut server).await["type"], "PROPOSE");
    send(
        &mut server,
        json!({"type":"BID_INVALID","task_id":4,"reason":"test rejection"}),
    )
    .await;
    let mut unknown = cfp(5);
    unknown["task_type"] = "unknown".into();
    send(&mut server, unknown).await;
    assert_eq!(read(&mut server).await["type"], "REFUSE");
    release.send(()).unwrap();
    let result = read(&mut server).await;
    assert_eq!(result["type"], "INFORM");
    assert_eq!(result["result"].as_str(), Some("1785318802179667385"));
    assert!(result["runtime"].as_f64().unwrap() >= 0.0);
    send(
        &mut server,
        json!({"type":"SETTLED","task_id":1,"verdict":"correct","revenue":2,"cost":1,
        "penalty":0,"profit":1,"runtime":null,"est_seconds":null}),
    )
    .await;
    send(&mut server, cfp(3)).await;
    assert_eq!(read(&mut server).await["type"], "PROPOSE");
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":3})).await;
    let result = read(&mut server).await;
    assert_eq!(result["type"], "FAILURE");
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("intentional executor failure")
    );
    send(
        &mut server,
        json!({"type":"SETTLED","task_id":3,"verdict":"failure","revenue":0,"cost":1,
        "penalty":2,"profit":-3}),
    )
    .await;
    evict(&mut server).await;
    let (client, result) = timeout(WAIT, runner).await.unwrap().unwrap();
    result.unwrap();
    assert_eq!(client.profit(), -2.0);
    assert_eq!(client.history.len(), 2);
    assert_eq!(client.history[0].task_type, "monte_carlo_pi");
    assert_eq!(client.queue_seconds(), 0.0);
    assert_eq!(*observations.executions.lock().unwrap(), vec![1, 3]);
    assert_eq!(observations.rejected.load(Ordering::SeqCst), 1);
    assert_eq!(observations.invalid.load(Ordering::SeqCst), 1);
    assert_eq!(observations.settled.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn reconnect_delivers_completed_work_after_registration_without_replaying_bids() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, observations, release) = client(listener.local_addr().unwrap().port());
    let runner = tokio::spawn(async move {
        let result = client.run().await;
        (client, result)
    });
    let mut server = accept(&listener).await;
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07","open_cfps":[cfp(1),cfp(2)]}),
    )
    .await;
    assert_eq!(read(&mut server).await["task_id"], 1);
    assert_eq!(read(&mut server).await["task_id"], 2);
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":1})).await;
    timeout(WAIT, observations.started.notified())
        .await
        .unwrap();
    // Close handshake ensures the client observes the disconnect before finishing work.
    server.close(None).await.unwrap();
    let _ = timeout(WAIT, server.next()).await;
    drop(server);
    release.send(()).unwrap();
    let mut server = accept(&listener).await;
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07","open_cfps":[]}),
    )
    .await;
    let result = read(&mut server).await;
    assert_eq!(result["type"], "INFORM");
    assert_eq!(result["task_id"], 1);
    send(&mut server, cfp(6)).await;
    let bid = read(&mut server).await;
    assert_eq!(bid["task_id"], 6);
    assert_eq!(
        bid["est_seconds"], 1.0,
        "stale bids and completed work must not inflate the queue"
    );
    evict(&mut server).await;
    let (_, result) = timeout(WAIT, runner).await.unwrap().unwrap();
    result.unwrap();
    assert_eq!(*observations.executions.lock().unwrap(), vec![1]);
    assert_eq!(observations.registered.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn invalid_credentials_stop_instead_of_reconnecting_forever() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, _, _) = client(listener.local_addr().unwrap().port());
    let runner = tokio::spawn(async move { client.run().await });
    let mut server = accept(&listener).await;
    send(
        &mut server,
        json!({"type":"ERROR","code":"bad_token","message":"invalid token"}),
    )
    .await;
    let error = timeout(WAIT, runner).await.unwrap().unwrap().unwrap_err();
    assert!(error.to_string().contains("registration rejected"));
}

#[test]
fn missing_rule_fields_keep_original_defaults() {
    let rules: Rules = serde_json::from_value(json!({"cost_rate":4})).unwrap();
    assert_eq!(rules.cost_rate, 4.0);
    assert_eq!(rules.time_weight, 2.0);
    assert_eq!(rules.concurrency, 1);
}

#[tokio::test]
async fn tls_connection_failures_retry_without_panicking() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, _, _) = client(listener.local_addr().unwrap().port());
    client.config.url = client.config.url.replacen("ws://", "wss://", 1);
    let runner = tokio::spawn(async move { client.run().await });
    // A missing Rustls crypto provider panics before any TLS handshake. A
    // properly configured client treats a dropped TLS handshake as retryable.
    for _ in 0..2 {
        let (stream, _) = timeout(WAIT, listener.accept()).await.unwrap().unwrap();
        drop(stream);
    }
    runner.abort();
    assert!(runner.await.unwrap_err().is_cancelled());
}

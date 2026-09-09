use crate::fixtures::reference_cases;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn production_binary_calibrates_bids_and_delivers_every_task_type() {
    timeout(Duration::from_secs(30), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("ws://{}/agent", listener.local_addr().unwrap());
        let mut process = Process(Command::new(env!("CARGO_BIN_EXE_contractor-net"))
            .args(["--name", "Rust_CLI", "--url", &address, "--token", "test-token"])
            .stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap());
        let (stream, _) = listener.accept().await.unwrap();
        let mut server = accept_async(stream).await.unwrap();
        let registration = server.next().await.unwrap().unwrap().into_text().unwrap();
        let registration: Value = serde_json::from_str(&registration).unwrap();
        assert_eq!(registration["type"], "REGISTER");
        assert_eq!(registration["name"], "Rust_CLI");
        assert!(registration["benchmark"].as_f64().unwrap() > 0.0);
        server.send(Message::Text(json!({"type":"REGISTERED","name":"Rust_CLI",
            "rules":{"award_policy":"best_value","time_weight":2,"cost_rate":1,
                "penalty_rate":0.5,"late_credit":0,"concurrency":1},"open_cfps":[]
        }).to_string().into())).await.unwrap();
        // Bad payloads produce an actionable error and leave the connection usable.
        server.send(Message::Text(json!({"type":"CFP","task_id":9999,"task_type":"prime_count",
            "budget":"invalid","deadline_s":1}).to_string().into())).await.unwrap();
        let cases = reference_cases();
        // Golden tasks plus moduli above 64 and 128 bits, transported alongside
        // decimal budgets and deadlines without losing any integer precision.
        for (index, case) in cases.iter().take(7).chain(cases.last()).enumerate() {
            let task_id = index as u64;
            let cfp = json!({"type":"CFP","task_id":task_id,"task_type":case.task_type,"params":case.params,
                "budget":999.5,"deadline_s":999.25});
            server.send(Message::Text(cfp.to_string().into())).await.unwrap();
            let proposal = server.next().await.unwrap().unwrap().into_text().unwrap();
            let proposal: Value = serde_json::from_str(&proposal).unwrap();
            assert_eq!(proposal["type"], "PROPOSE", "{}", case.task_type);
            assert_eq!(proposal["task_id"], task_id);
            server.send(Message::Text(json!({"type":"ACCEPT_PROPOSAL","task_id":task_id}).to_string().into())).await.unwrap();
            let result = server.next().await.unwrap().unwrap().into_text().unwrap();
            let result: Value = serde_json::from_str(&result).unwrap();
            assert_eq!(result["type"], "INFORM", "{result}");
            assert_eq!(result["task_id"], task_id);
            assert_eq!(result["result"].as_str(), Some(case.expected.as_str()), "{}", case.task_type);
            server.send(Message::Text(json!({"type":"SETTLED","task_id":task_id,"verdict":"correct",
                "revenue":1,"cost":0.1,"penalty":0,"profit":0.9,"runtime":result["runtime"],
                "est_seconds":proposal["est_seconds"]}).to_string().into())).await.unwrap();
        }
        server.close(Some(CloseFrame { code: CloseCode::from(4001), reason: "test completed".into() })).await.unwrap();
        loop {
            if let Some(status) = process.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
        let mut logs = String::new();
        process.0.stdout.take().unwrap().read_to_string(&mut logs).unwrap();
        assert!(logs.contains("registered as Rust_CLI"), "{logs}");
        assert!(logs.contains("cannot decode manager message: invalid CFP message: invalid type"), "{logs}");
        assert_eq!(logs.lines().filter(|line| line.contains(" correct: profit ")).count(), 8, "{logs}");
    }).await.expect("the complete CLI exchange must finish within 30 seconds");
}

#[test]
fn verifier_cli_checks_fixtures_without_creating_a_log() {
    let output = Command::new(env!("CARGO_BIN_EXE_verify"))
        .args(["--all", "--no-log"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("All 218 checks passed."));
}

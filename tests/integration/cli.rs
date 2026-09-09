use crate::fixtures::reference_cases;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
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
            .args(["--name", "Rust_CLI", "--url", &address, "--token", "test-token", "--quiet"])
            .stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap());
        let (stream, _) = listener.accept().await.unwrap();
        let mut server = accept_async(stream).await.unwrap();
        let registration = server.next().await.unwrap().unwrap().into_text().unwrap();
        let registration: Value = serde_json::from_str(&registration).unwrap();
        assert_eq!(registration["type"], "REGISTER");
        assert_eq!(registration["name"], "Rust_CLI");
        assert!(registration["benchmark"].as_f64().unwrap() > 0.0);
        server.send(Message::Text(json!({"type":"REGISTERED","name":"Rust_CLI"}).to_string().into())).await.unwrap();
        let cases = reference_cases();
        // Golden task types plus a large modulus transported as a JSON integer.
        for (index, case) in cases.iter().take(5).chain(cases.last()).enumerate() {
            let task_id = index as u64;
            let cfp = json!({"type":"CFP","task_id":task_id,"task_type":case.task_type,"params":case.params,
                "budget":999,"deadline_s":999});
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

//! Overlapping auctions against a local manager, with a controlled worker.
use super::*;

#[tokio::test]
async fn default_bidder_prices_parallel_queue_reserves_and_checks_deadlines() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (template, _, _) = client(listener.local_addr().unwrap().port());
    let mut client = Contractor::new(template.config, contractnet::MyContractor);
    client.rates = template.rates;
    let runner = tokio::spawn(async move { client.run().await });
    let mut server = accept(&listener).await;
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07","rules":{"concurrency":3}}),
    )
    .await;
    let mut tight = cfp(5);
    tight["deadline_s"] = 4.0.into();
    send(&mut server, tight).await;
    assert_eq!(
        read(&mut server).await["type"],
        "REFUSE",
        "p95 queue reserve must fit even when the median quote would fit"
    );
    for id in [1, 2, 4] {
        send(&mut server, cfp(id)).await;
        let bid = read(&mut server).await;
        assert_eq!(bid["type"], "PROPOSE");
        let compute_reserve = 1.2 * 1.1;
        let quoted = 2.0 * compute_reserve + 1.05 + 0.055;
        assert!((bid["est_seconds"].as_f64().unwrap() - quoted).abs() < 1e-9);
        let cost_floor = (3.0 * compute_reserve + 0.055) * 1.2 + 0.01;
        assert!(bid["price"].as_f64().unwrap() >= cost_floor);
    }
    send(&mut server, cfp(6)).await;
    assert_eq!(read(&mut server).await["type"], "REFUSE");
    evict(&mut server).await;
    timeout(WAIT, runner).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn parallel_auctions_reserve_capacity_and_execute_in_award_order() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, observations, release) = client(listener.local_addr().unwrap().port());
    let runner = tokio::spawn(async move {
        let result = client.run().await;
        (client, result)
    });
    let mut server = accept(&listener).await;
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07",
        "rules":{"concurrency":3},"open_cfps":[cfp(1),cfp(2),cfp(4)]}),
    )
    .await;
    for id in [1, 2, 4] {
        let bid = read(&mut server).await;
        assert_eq!(bid["type"], "PROPOSE");
        assert_eq!(bid["task_id"], id);
        assert_eq!(
            bid["est_seconds"], 3.0,
            "each bid must tolerate being awarded last"
        );
    }
    send(&mut server, cfp(5)).await;
    assert_eq!(
        read(&mut server).await["type"],
        "REFUSE",
        "earlier bids have used their queue allowance"
    );
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":1})).await;
    timeout(WAIT, observations.started.notified())
        .await
        .unwrap();
    // Awards differ from proposal order; duplicates must not enqueue twice.
    for id in [4, 4, 2] {
        send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":id})).await;
    }
    let mut tight = cfp(6);
    tight["deadline_s"] = 2.5.into();
    send(&mut server, tight).await;
    assert_eq!(read(&mut server).await["type"], "REFUSE");
    assert_eq!(
        *observations.executions.lock().unwrap(),
        vec![1],
        "queued work must not run concurrently"
    );
    release.send(()).unwrap();
    for id in [1, 4, 2] {
        let result = read(&mut server).await;
        assert_eq!(result["type"], "INFORM");
        assert_eq!(result["task_id"], id);
    }
    evict(&mut server).await;
    let (client, result) = timeout(WAIT, runner).await.unwrap().unwrap();
    result.unwrap();
    assert_eq!(*observations.executions.lock().unwrap(), vec![1, 4, 2]);
    assert_eq!(client.queue_seconds(), 0.0);
}

#[tokio::test]
async fn heterogeneous_jobs_cannot_delay_an_earlier_bid_beyond_its_quote() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, _, _) = client(listener.local_addr().unwrap().port());
    let runner = tokio::spawn(async move { client.run().await });
    let mut server = accept(&listener).await;
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07","rules":{"concurrency":2}}),
    )
    .await;
    send(&mut server, cfp(1)).await;
    assert_eq!(read(&mut server).await["est_seconds"], 2.0);
    let mut large = cfp(2);
    large["params"]["samples"] = 4000.into();
    send(&mut server, large).await;
    assert_eq!(read(&mut server).await["type"], "REFUSE");
    send(&mut server, cfp(2)).await;
    assert_eq!(read(&mut server).await["est_seconds"], 2.0);
    // A duplicate CFP replaces itself rather than consuming another slot.
    send(&mut server, cfp(1)).await;
    assert_eq!(read(&mut server).await["est_seconds"], 2.0);
    send(&mut server, json!({"type":"REJECT_PROPOSAL","task_id":2})).await;
    send(&mut server, cfp(4)).await;
    assert_eq!(
        read(&mut server).await["type"],
        "PROPOSE",
        "rejected bids release capacity"
    );
    evict(&mut server).await;
    timeout(WAIT, runner).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn failed_replacement_keeps_the_original_bid_reserved_and_executable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, observations, release) = client(listener.local_addr().unwrap().port());
    let runner = tokio::spawn(async move { client.run().await });
    let mut server = accept(&listener).await;
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07","rules":{"concurrency":2}}),
    )
    .await;
    send(&mut server, cfp(1)).await;
    assert_eq!(read(&mut server).await["type"], "PROPOSE");
    // Force a failed re-evaluation without changing the task's identity.
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07",
        "rules":{"concurrency":100},"open_cfps":[cfp(1)]}),
    )
    .await;
    send(
        &mut server,
        json!({"type":"REGISTERED","name":"Rust_07","rules":{"concurrency":2}}),
    )
    .await;
    send(&mut server, cfp(2)).await;
    let bid = read(&mut server).await;
    assert_eq!(
        bid["task_id"], 2,
        "failed replacement must not send a misleading REFUSE"
    );
    send(&mut server, cfp(4)).await;
    assert_eq!(
        read(&mut server).await["type"],
        "REFUSE",
        "original bid is still a commitment"
    );
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":1})).await;
    timeout(WAIT, observations.started.notified())
        .await
        .unwrap();
    release.send(()).unwrap();
    assert_eq!(read(&mut server).await["task_id"], 1);
    evict(&mut server).await;
    timeout(WAIT, runner).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn overrun_refuses_new_bids_until_the_worker_finishes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, observations, release) = client(listener.local_addr().unwrap().port());
    client.rates.insert("monte_carlo_pi".into(), 1_000_000.0);
    let runner = tokio::spawn(async move { client.run().await });
    let mut server = accept(&listener).await;
    send(&mut server, json!({"type":"REGISTERED","name":"Rust_07"})).await;
    send(&mut server, cfp(1)).await;
    assert_eq!(read(&mut server).await["type"], "PROPOSE");
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":1})).await;
    timeout(WAIT, observations.started.notified())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    send(&mut server, cfp(2)).await;
    assert_eq!(
        read(&mut server).await["type"],
        "REFUSE",
        "an overdue worker is not an empty queue"
    );
    release.send(()).unwrap();
    assert_eq!(read(&mut server).await["type"], "INFORM");
    send(&mut server, cfp(2)).await;
    assert_eq!(read(&mut server).await["type"], "PROPOSE");
    evict(&mut server).await;
    timeout(WAIT, runner).await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn settlement_does_not_free_a_worker_that_is_still_executing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut client, observations, release) = client(listener.local_addr().unwrap().port());
    client.rates.insert("monte_carlo_pi".into(), 10.0);
    let runner = tokio::spawn(async move {
        let result = client.run().await;
        (client, result)
    });
    let mut server = accept(&listener).await;
    send(&mut server, json!({"type":"REGISTERED","name":"Rust_07"})).await;
    let mut long = cfp(1);
    long["deadline_s"] = 200.into();
    send(&mut server, long).await;
    assert_eq!(read(&mut server).await["type"], "PROPOSE");
    send(&mut server, json!({"type":"ACCEPT_PROPOSAL","task_id":1})).await;
    timeout(WAIT, observations.started.notified())
        .await
        .unwrap();
    send(
        &mut server,
        json!({"type":"SETTLED","task_id":1,"verdict":"timeout",
        "revenue":0,"cost":1,"penalty":1,"profit":-2}),
    )
    .await;
    let mut short = cfp(2);
    short["params"]["samples"] = 1.into();
    short["deadline_s"] = 1.into();
    send(&mut server, short).await;
    assert_eq!(
        read(&mut server).await["type"],
        "REFUSE",
        "timed-out CPU work still occupies the worker"
    );
    release.send(()).unwrap();
    evict(&mut server).await;
    let (_, result) = timeout(WAIT, runner).await.unwrap().unwrap();
    result.unwrap();
}

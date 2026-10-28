//! Phase-3 control-plane enrollment: end-to-end against a real `loom-cp`.
//!
//! Spins the axum control plane on an ephemeral port with a temp SQLite db,
//! then exercises org CRUD, the free-tier seat cap, two-host enrollment, and
//! the OrgClient (connect + membership/license download).
//!
//! Note: a valid license flip cannot be end-to-end tested here because the
//! embedded verification *private* key is offline (never in the repo). License
//! states/grace are unit-tested in `loom-core::license`; this suite covers the
//! download endpoint and the rejection of an invalid stored key.

use loom_control_plane::{AppState, build_app};
use loom_core::org::{OrgClient, read_membership, read_token, store_membership};
use serde_json::{json, Value};
use std::mem;
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn spawn_cp() -> u16 {
    let dir = tempdir().unwrap();
    let db = dir.path().join("cp.db");
    let state = AppState::new(
            dir.path(),
            &db,
            None,
            None,
        )
        .await
        .expect("cp state");
    let app = build_app(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("cp serve");
    });
    // Leak the tempdir so the db file outlives this helper (test process is
    // short-lived; the OS cleans /tmp on exit).
    mem::forget(dir);
    port
}

fn post_json(url: &str, body: &Value) -> ureq::Response {
    let s = serde_json::to_string(body).expect("serialize");
    match ureq::post(url)
        .set("Content-Type", "application/json")
        .send_string(&s)
    {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(e) => panic!("transport error: {e}"),
    }
}

fn body_text(resp: ureq::Response) -> String {
    resp.into_string().expect("read body")
}

#[tokio::test(flavor = "multi_thread")]
async fn org_crud_and_seat_cap() {
    let port = spawn_cp().await;
    let base = format!("http://127.0.0.1:{port}");

    let org: Value = serde_json::from_str(&body_text(post_json(
        &format!("{base}/api/v1/orgs"),
        &json!({"name": "Acme"}),
    )))
    .expect("parse org");
    let org_id = org["id"].as_str().expect("org id");
    assert_eq!(org["name"], "Acme");
    assert_eq!(org["tier"], "free");

    // 5 humans accepted
    for i in 0..5 {
        let resp = post_json(
            &format!("{base}/api/v1/org/{org_id}/roster"),
            &json!({"email": format!("u{i}@acme.com"), "role": "member"}),
        );
        assert_eq!(resp.status(), 201, "human {i}");
    }

    // 6th rejected with a distinct cap error
    let resp = post_json(
        &format!("{base}/api/v1/org/{org_id}/roster"),
        &json!({"email": "u6@acme.com", "role": "member"}),
    );
    assert_eq!(resp.status(), 403);
    let err: Value = serde_json::from_str(&body_text(resp)).expect("parse error");
    assert!(err["message"].as_str().unwrap().contains("seat cap"));
}

#[tokio::test(flavor = "multi_thread")]
async fn two_hosts_enroll_and_share_membership() {
    let port = spawn_cp().await;
    let base = format!("http://127.0.0.1:{port}");

    let org: Value = serde_json::from_str(&body_text(post_json(
        &format!("{base}/api/v1/orgs"),
        &json!({"name": "TwoHost"}),
    )))
    .expect("parse org");
    let org_id = org["id"].as_str().expect("org id");

    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    eprintln!("DEBUG test: dir1={:?} dir2={:?}", dir1.path(), dir2.path());

    let c1 = OrgClient::with_dir(&base, org_id, dir1.path().to_path_buf());
    let issued1 = c1.connect().expect("host1 connect");
    assert!(issued1.get("token").is_some());
    assert_eq!(issued1.get("org_id").and_then(|v| v.as_str()).unwrap(), org_id);

    let c2 = OrgClient::with_dir(&base, org_id, dir2.path().to_path_buf());
    let issued2 = c2.connect().expect("host2 connect");
    assert_ne!(
        issued1.get("machine_pub").and_then(|v| v.as_str()),
        issued2.get("machine_pub").and_then(|v| v.as_str())
    );

    // Both machines appear in org membership (cached per host).
    let m1 = c1.download_membership().unwrap();
    store_membership(dir1.path(), &m1).unwrap();
    let machines1 = m1["machines"].as_array().expect("machines array");
    assert_eq!(machines1.len(), 2);

    let dir2_org = dir2.path().join("org");
    let m2 = read_membership(&dir2_org).unwrap().unwrap();
    let machines2 = m2["machines"].as_array().expect("machines array");
    assert_eq!(machines2.len(), 2);

    // Token files written at 0600.
    let dir1_org = dir1.path().join("org");
    let dir2_org = dir2.path().join("org");
    assert!(read_token(&dir1_org).unwrap().is_some());
    assert!(read_token(&dir2_org).unwrap().is_some());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m1 = std::fs::metadata(dir1.path().join("org").join("token"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(m1, 0o600);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn license_download_and_invalid_key_rejected() {
    let port = spawn_cp().await;
    let base = format!("http://127.0.0.1:{port}");

    let org: Value = serde_json::from_str(&body_text(post_json(
        &format!("{base}/api/v1/orgs"),
        &json!({"name": "Lic"}),
    )))
    .expect("parse org");
    let org_id = org["id"].as_str().expect("org id");

    // Enroll a machine so we have auth for license download
    let dir = tempdir().unwrap();
    let client = OrgClient::with_dir(&base, org_id, dir.path().to_path_buf());
    client.connect().expect("host connect");

    // No license set yet -> 404 (authenticated)
    let token = client.token().unwrap().unwrap();
    let proof = client
        .dpop_proof_for("GET", &format!("/api/v1/org/{org_id}/license"))
        .unwrap();
    let resp = ureq::get(&format!("{base}/api/v1/org/{org_id}/license"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("DPoP", &proof)
        .call();
    let resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("transport error: {e}"),
    };
    assert_eq!(resp.status(), 404);

    // Garbage license key rejected (400) — embedded public key won't verify it
    let proof = client
        .dpop_proof_for("POST", &format!("/api/v1/org/{org_id}/license"))
        .unwrap();
    let s = serde_json::to_string(&json!({"license_key": "not.a.real.key"})).unwrap();
    let resp = ureq::post(&format!("{base}/api/v1/org/{org_id}/license"))
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {token}"))
        .set("DPoP", &proof)
        .send_string(&s);
    let resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("transport error: {e}"),
    };
    assert_eq!(resp.status(), 400);
}

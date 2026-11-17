//! The platform end to end, against a real Postgres with pgvector: each test
//! gets its own empty database. The language model and the embedding model
//! are fakes, so nothing leaves the machine.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use capricorn_platform::config::{Config, EventsWebhook, LiveKit};
use capricorn_platform::llm::{Llm, LlmError, Request as LlmRequest};
use capricorn_platform::mail::{MailError, Mailer};
use capricorn_platform::memory::{DIMENSIONS, Embedder, MemoryError};
use capricorn_platform::state::AppState;
use capricorn_platform::{events, http, jobs};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ADMIN: &str = "0123456789abcdef0123456789abcdef";

/// Answers each kind of question with a fixed object.
struct ScriptedLlm {
    asked: Mutex<Vec<String>>,
}

#[async_trait]
impl Llm for ScriptedLlm {
    async fn answer(&self, request: LlmRequest<'_>) -> Result<Value, LlmError> {
        self.asked.lock().unwrap().push(request.answer.to_string());
        Ok(match request.answer {
            "session_brief" => json!({
                "goal": "Review adding fractions with football examples.",
                "skills": [{ "skill_id": "fractions.add", "why": "Started last week.", "start_with": "Half a pitch plus a quarter?" }],
                "approach": ["One step at a time."],
                "remember": ["Likes football."]
            }),
            "guidance" => {
                json!({ "text": "Give a smaller hint and wait.", "signals": { "frustrated": true } })
            }
            "session_review" => json!({
                "guardian_summary": "Practicó sumas de fracciones y mejoró con una pista.",
                "next_step": "Try unlike denominators.",
                "evidence": [
                    { "skill_id": "fractions.add", "correct": true, "credit": 0.5 },
                    { "skill_id": "fractions.add", "correct": true }
                ],
                "memories": [
                    { "kind": "preference", "content": "Likes football examples." },
                    { "kind": "struggle", "content": "Forgets to find a common denominator." }
                ],
                "flags": ["struggling"]
            }),
            other => panic!("unexpected question {other}"),
        })
    }
}

/// Puts each text on one axis chosen by its first word, so texts that start
/// alike are close and the rest are far.
struct WordEmbedder;

#[async_trait]
impl Embedder for WordEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError> {
        Ok(texts
            .iter()
            .map(|text| {
                let first = text.split_whitespace().next().unwrap_or("").to_lowercase();
                let axis = first.bytes().map(usize::from).sum::<usize>() % DIMENSIONS;
                let mut vector = vec![0.0; DIMENSIONS];
                vector[axis] = 1.0;
                vector
            })
            .collect())
    }
}

/// Keeps every message instead of sending it.
#[derive(Default)]
struct Outbox {
    sent: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl Mailer for Outbox {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), MailError> {
        self.sent.lock().unwrap().push((to.to_string(), subject.to_string(), body.to_string()));
        Ok(())
    }
}

struct App {
    router: Router,
    state: AppState,
    llm: Arc<ScriptedLlm>,
    mail: Arc<Outbox>,
}

fn app(pool: PgPool) -> App {
    app_with(pool, None)
}

fn app_with(pool: PgPool, events_webhook: Option<EventsWebhook>) -> App {
    let config = Config {
        database_url: String::new(),
        addr: "127.0.0.1:0".parse().unwrap(),
        admin_token: ADMIN.to_string(),
        livekit: Some(LiveKit {
            public_url: "wss://live.example.org".into(),
            api_key: "key".into(),
            api_secret: "secret".into(),
        }),
        agent_name: "capricorn-tutor".into(),
        session_minutes: 20,
        transcript_hours: 24,
        supervisor: None,
        embeddings: None,
        mail: None,
        events_webhook,
        timezone_offset_minutes: -300,
    };
    let mail = Arc::new(Outbox::default());
    let llm = Arc::new(ScriptedLlm { asked: Mutex::new(Vec::new()) });
    let state = AppState {
        pool,
        config: Arc::new(config),
        llm: Some(llm.clone()),
        embedder: Some(Arc::new(WordEmbedder)),
        mailer: Some(mail.clone()),
        watching: Arc::new(Mutex::new(HashSet::new())),
        claim_attempts: Arc::new(Mutex::new(HashMap::new())),
    };
    App { router: http::router(state.clone()), state, llm, mail }
}

impl App {
    async fn call(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let request = match body {
            Some(body) => {
                request.header("Content-Type", "application/json").body(Body::from(body.to_string()))
            }
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn admin(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call(method, path, Some(ADMIN), body).await
    }

    /// A family with one child; returns the child's id.
    async fn child(&self) -> String {
        let (status, family) = self
            .admin(
                Method::POST,
                "/v1/families",
                Some(json!({ "name": "Quispe", "guardian": { "email": "Ana@Example.org", "name": "Ana" } })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{family}");
        let (status, child) = self
            .admin(
                Method::POST,
                &format!("/v1/families/{}/children", family["family_id"].as_str().unwrap()),
                Some(json!({ "nickname": "Mateo", "birth_year": 2016, "grade": "P4", "interests": ["football"] })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{child}");
        child["id"].as_str().unwrap().to_string()
    }

    async fn consent(&self, child: &str, purpose: &str, granted: bool) {
        let (status, body) = self
            .admin(
                Method::POST,
                &format!("/v1/children/{child}/consents"),
                Some(json!({ "purpose": purpose, "granted": granted, "policy_version": "2026-11", "source": "test" })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    /// Starts a session and returns its id and the agent's token, read from
    /// the dispatch metadata inside the room token like the agent does.
    async fn session(&self, child: &str) -> (String, String) {
        let (status, session) = self
            .admin(Method::POST, "/v1/sessions", Some(json!({ "child_id": child, "audience": "operator" })))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{session}");
        let claims = session["token"].as_str().unwrap().split('.').nth(1).unwrap().to_string();
        let claims: Value =
            serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(claims).unwrap())
                .unwrap();
        let dispatch: Value =
            serde_json::from_str(claims["roomConfig"]["agents"][0]["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(dispatch["audience"], "operator");
        assert_eq!(claims["video"]["room"], session["room"]);
        (
            session["session_id"].as_str().unwrap().to_string(),
            dispatch["platform_token"].as_str().unwrap().to_string(),
        )
    }

    /// Signs a guardian in with the code from their email; returns their token.
    async fn sign_in(&self, email: &str, name: &str) -> String {
        let (status, _) =
            self.call(Method::POST, "/v1/auth/start", None, Some(json!({ "email": email }))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let code = self.mail.sent.lock().unwrap().last().unwrap().1[..6].to_string();
        let (status, body) = self
            .call(
                Method::POST,
                "/v1/auth/verify",
                None,
                Some(json!({ "email": email, "code": code, "name": name })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["token"].as_str().unwrap().to_string()
    }

    /// A device's check-in, the way the firmware sends it.
    async fn check_in(&self, serial: &str, version: &str) -> Value {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/ota/")
            .header("Serial-Number", serial)
            .header("Device-Id", "aa:bb:cc:dd:ee:ff")
            .header("Content-Type", "application/json")
            .body(Body::from(json!({ "application": { "version": version } }).to_string()))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap()).unwrap()
    }

    /// Delivers every pending event.
    async fn deliver_events(&self) {
        while events::deliver_once(&self.state).await.unwrap() {}
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.state.pool).await.unwrap()
    }
}

/// Waits for something a background task writes.
async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if check().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("never happened: {what}");
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn every_route_needs_its_credentials(pool: PgPool) {
    let app = app(pool);

    assert_eq!(app.call(Method::GET, "/healthz", None, None).await.0, StatusCode::OK);
    for (method, path) in [
        (Method::POST, "/v1/families"),
        (Method::GET, "/v1/skills"),
        (Method::POST, "/v1/sessions"),
        (Method::GET, "/v1/agent/session"),
    ] {
        let (status, body) = app.call(method.clone(), path, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(body["error"]["code"], "unauthorized");
        assert_eq!(app.call(method, path, Some("wrong"), None).await.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    // The admin token is not an agent token.
    assert_eq!(
        app.call(Method::GET, "/v1/agent/session", Some(ADMIN), None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn answers_build_the_learner_model_and_calibrate_difficulty(pool: PgPool) {
    let app = app(pool);
    let child = app.child().await;
    app.admin(
        Method::PUT,
        "/v1/skills",
        Some(json!({ "skills": [
            { "id": "add", "subject": "math", "name": "Addition" },
            { "id": "mult", "subject": "math", "name": "Multiplication", "prerequisites": ["add"] }
        ]})),
    )
    .await;

    let (_, before) = app.admin(Method::GET, &format!("/v1/children/{child}/skills"), None).await;
    assert_eq!(before["skills"], json!([]));
    assert_eq!(before["next"][0]["skill_id"], "add");
    assert_eq!(before["next"].as_array().unwrap().len(), 1, "mult waits for add");

    let attempts: Vec<Value> = (0..8)
        .map(|n| json!({ "skill_id": "add", "question_id": format!("q{}", n % 2), "correct": true }))
        .collect();
    let (status, body) = app
        .admin(
            Method::POST,
            &format!("/v1/children/{child}/attempts"),
            Some(json!({ "source": "import", "attempts": attempts })),
        )
        .await;
    assert_eq!((status, &body["recorded"]), (StatusCode::CREATED, &json!(8)));

    let (_, after) = app.admin(Method::GET, &format!("/v1/children/{child}/skills"), None).await;
    assert_eq!(after["skills"][0]["skill_id"], "add");
    assert_eq!(after["skills"][0]["attempts"], 8);
    assert!(after["skills"][0]["p_now"].as_f64().unwrap() > 0.5);
    assert!(after["ability"].as_f64().unwrap() > 0.0);
    // Questions everyone gets right become easier than average.
    let difficulty: f64 =
        sqlx::query_scalar("select value from item_difficulty where skill_id = 'add' and question_id = ''")
            .fetch_one(&app.state.pool)
            .await
            .unwrap();
    assert!(difficulty < 0.0);
    assert_eq!(app.count("select count(*) from item_difficulty where answers > 0").await, 3);

    // A correction overrides the model.
    let (status, _) = app
        .admin(
            Method::POST,
            &format!("/v1/children/{child}/corrections"),
            Some(json!({ "skill_id": "mult", "level": 0.95 })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, corrected) = app.admin(Method::GET, &format!("/v1/children/{child}/skills"), None).await;
    let mult =
        corrected["skills"].as_array().unwrap().iter().find(|skill| skill["skill_id"] == "mult").unwrap();
    assert_eq!(mult["corrected"], true);

    let (status, body) = app
        .admin(
            Method::POST,
            &format!("/v1/children/{child}/attempts"),
            Some(json!({ "source": "import", "attempts": [{ "skill_id": "Not A Slug", "correct": true }] })),
        )
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_request")));
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn no_session_without_consent_for_voice(pool: PgPool) {
    let app = app(pool);
    let child = app.child().await;
    let start = json!({ "child_id": child, "audience": "child" });

    let (status, body) = app.admin(Method::POST, "/v1/sessions", Some(start.clone())).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("forbidden")));

    app.consent(&child, "voice", true).await;
    assert_eq!(app.admin(Method::POST, "/v1/sessions", Some(start.clone())).await.0, StatusCode::CREATED);

    app.consent(&child, "voice", false).await;
    assert_eq!(app.admin(Method::POST, "/v1/sessions", Some(start)).await.0, StatusCode::FORBIDDEN);

    let (status, _) = app.admin(Method::POST, "/v1/sessions", Some(json!({ "audience": "child" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a child's session needs a child");
    let (status, _) = app.admin(Method::POST, "/v1/sessions", Some(json!({ "audience": "operator" }))).await;
    assert_eq!(status, StatusCode::CREATED, "an operator can try the tutor with no profile");
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn a_session_is_planned_watched_and_reviewed(pool: PgPool) {
    let app = app(pool);
    let child = app.child().await;
    app.consent(&child, "voice", true).await;
    app.consent(&child, "memory", true).await;
    let (session, token) = app.session(&child).await;
    let agent = Some(token.as_str());

    // Before: the tutor gets the student's profile and the supervisor's plan.
    let (status, start) = app.call(Method::GET, "/v1/agent/session", agent, None).await;
    assert_eq!(status, StatusCode::OK, "{start}");
    let instructions = start["instructions"].as_str().unwrap();
    assert!(instructions.contains("The student's name is Mateo."), "{instructions}");
    assert!(instructions.contains("Review adding fractions with football examples."), "{instructions}");
    assert_eq!(app.count("select count(*) from sessions where status = 'active'").await, 1);

    // During: reporting a turn returns at once; the guidance comes with a later call.
    let turns = json!({ "turns": [
        { "seq": 0, "role": "tutor", "content": "¿Cuánto es un medio más un cuarto?" },
        { "seq": 1, "role": "child", "content": "No sé, es muy difícil." }
    ]});
    let (status, first) = app.call(Method::POST, "/v1/agent/session/turns", agent, Some(turns.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    eventually("the supervisor wrote guidance", || async {
        app.count("select count(*) from session_guidance").await == 1
    })
    .await;
    // Sending the same turns again stores nothing new.
    let (_, second) = app.call(Method::POST, "/v1/agent/session/turns", agent, Some(turns)).await;
    assert_eq!(app.count("select count(*) from session_turns").await, 2);
    let guidance = &second["guidance"][0];
    assert!(guidance["instructions"].as_str().unwrap().contains("Give a smaller hint and wait."));
    let (_, view) = app.admin(Method::GET, &format!("/v1/sessions/{session}/supervisor"), None).await;
    assert_eq!(view["status"], "active");
    assert_eq!(view["brief"]["skills"][0]["skill_id"], "fractions.add");
    assert_eq!(view["guidance"][0]["signals"]["frustrated"], true);
    assert_eq!(view["guidance"][0]["after_seq"], 1);
    let (_, third) = app
        .call(
            Method::POST,
            "/v1/agent/session/turns",
            agent,
            Some(json!({ "turns": [{ "seq": 2, "role": "tutor", "content": "Piensa en una pizza." }], "after_guidance": guidance["id"] })),
        )
        .await;
    assert_eq!(third["guidance"], json!([]), "guidance is delivered once");

    // After: ending queues the review, and the worker runs it.
    let end =
        || app.call(Method::POST, "/v1/agent/session/end", agent, Some(json!({ "speaking_seconds": 300 })));
    assert_eq!(end().await.0, StatusCode::NO_CONTENT);
    assert_eq!(end().await.0, StatusCode::NO_CONTENT, "ending twice is fine");
    assert_eq!(app.count("select count(*) from jobs").await, 1);
    assert!(jobs::work_once(&app.state).await.unwrap());
    assert!(!jobs::work_once(&app.state).await.unwrap());
    assert_eq!(app.count("select count(*) from jobs where done_at is not null").await, 1);

    let (_, skills) = app.admin(Method::GET, &format!("/v1/children/{child}/skills"), None).await;
    assert_eq!(skills["skills"][0]["skill_id"], "fractions.add");
    assert_eq!(skills["skills"][0]["attempts"], 2);
    let (_, reviews) = app.admin(Method::GET, &format!("/v1/children/{child}/reviews"), None).await;
    assert_eq!(reviews["reviews"][0]["flags"], json!(["struggling"]));
    assert_eq!(reviews["reviews"][0]["session_id"], session.as_str());
    let (_, memories) =
        app.admin(Method::GET, &format!("/v1/children/{child}/memories?query=Likes%20what%3F"), None).await;
    assert_eq!(memories["memories"][0]["content"], "Likes football examples.", "closest in meaning first");
    assert_eq!(memories["memories"].as_array().unwrap().len(), 2);

    // The guardian did not agree to keep transcripts, so the turns are gone.
    assert_eq!(app.count("select count(*) from session_turns").await, 0);
    assert_eq!(
        app.call(Method::POST, "/v1/agent/session/turns", agent, Some(json!({ "turns": [] }))).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(*app.llm.asked.lock().unwrap(), ["session_brief", "guidance", "session_review"]);

    // The next session's plan can use what was learned; memories are not duplicated.
    let (_, token) = app.session(&child).await;
    app.call(Method::GET, "/v1/agent/session", Some(&token), None).await;
    app.call(
        Method::POST,
        "/v1/agent/session/turns",
        Some(&token),
        Some(json!({ "turns": [{ "seq": 0, "role": "child", "content": "Hola" }] })),
    )
    .await;
    app.call(Method::POST, "/v1/agent/session/end", Some(&token), Some(json!({}))).await;
    eventually("the second session's guidance settled", || async {
        app.state.watching.lock().unwrap().is_empty()
    })
    .await;
    assert!(jobs::work_once(&app.state).await.unwrap());
    assert_eq!(app.count("select count(*) from memories").await, 2);
    assert_eq!(app.count("select count(*) from attempts").await, 4);
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn a_kept_transcript_expires_and_an_abandoned_session_is_closed(pool: PgPool) {
    let app = app(pool);
    let child = app.child().await;
    app.consent(&child, "voice", true).await;
    app.consent(&child, "transcripts", true).await;
    let (_, token) = app.session(&child).await;
    let turn = json!({ "turns": [{ "seq": 0, "role": "child", "content": "Hola" }] });
    app.call(Method::POST, "/v1/agent/session/turns", Some(&token), Some(turn.clone())).await;
    app.call(Method::POST, "/v1/agent/session/end", Some(&token), Some(json!({}))).await;
    eventually("guidance settled", || async { app.state.watching.lock().unwrap().is_empty() }).await;
    assert!(jobs::work_once(&app.state).await.unwrap());
    assert_eq!(app.count("select count(*) from session_turns").await, 1, "kept, with consent");

    jobs::housekeeping(&app.state).await.unwrap();
    assert_eq!(app.count("select count(*) from session_turns").await, 1, "still inside its time");
    sqlx::query("update sessions set ended_at = now() - interval '25 hours'")
        .execute(&app.state.pool)
        .await
        .unwrap();
    jobs::housekeeping(&app.state).await.unwrap();
    assert_eq!(app.count("select count(*) from session_turns").await, 0, "past its time");

    // A session whose agent never said goodbye.
    let (_, token) = app.session(&child).await;
    app.call(Method::POST, "/v1/agent/session/turns", Some(&token), Some(turn)).await;
    eventually("guidance settled", || async { app.state.watching.lock().unwrap().is_empty() }).await;
    sqlx::query("update sessions set created_at = now() - interval '4 hours' where status <> 'ended'")
        .execute(&app.state.pool)
        .await
        .unwrap();
    jobs::housekeeping(&app.state).await.unwrap();
    assert_eq!(app.count("select count(*) from sessions where status <> 'ended'").await, 0);
    assert_eq!(app.count("select count(*) from jobs where done_at is null").await, 1);
    assert_eq!(
        app.call(Method::GET, "/v1/agent/session", Some(&token), None).await.0,
        StatusCode::UNAUTHORIZED,
        "the token expired with it"
    );
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn taking_consent_back_and_erasing_a_child_remove_their_data(pool: PgPool) {
    let app = app(pool);
    let child = app.child().await;
    for purpose in ["voice", "memory"] {
        app.consent(&child, purpose, true).await;
    }
    let (_, token) = app.session(&child).await;
    app.call(
        Method::POST,
        "/v1/agent/session/turns",
        Some(&token),
        Some(json!({ "turns": [{ "seq": 0, "role": "child", "content": "Hola" }] })),
    )
    .await;
    app.call(Method::POST, "/v1/agent/session/end", Some(&token), Some(json!({}))).await;
    eventually("guidance settled", || async { app.state.watching.lock().unwrap().is_empty() }).await;
    assert!(jobs::work_once(&app.state).await.unwrap());
    assert_eq!(app.count("select count(*) from memories").await, 2);

    // One memory can be removed on its own.
    let (_, memories) = app.admin(Method::GET, &format!("/v1/children/{child}/memories"), None).await;
    let one = memories["memories"][0]["id"].as_str().unwrap();
    assert_eq!(
        app.admin(Method::DELETE, &format!("/v1/children/{child}/memories/{one}"), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.admin(Method::DELETE, &format!("/v1/children/{child}/memories/{one}"), None).await.0,
        StatusCode::NOT_FOUND
    );

    app.consent(&child, "memory", false).await;
    assert_eq!(app.count("select count(*) from memories").await, 0);
    let (_, profile) = app.admin(Method::GET, &format!("/v1/children/{child}"), None).await;
    assert_eq!(profile["consent"], json!({ "memory": false, "transcripts": false, "voice": true }));

    assert_eq!(
        app.admin(Method::DELETE, &format!("/v1/children/{child}"), None).await.0,
        StatusCode::NO_CONTENT
    );
    for table in ["children", "sessions", "attempts", "session_reviews", "session_briefs"] {
        assert_eq!(app.count(&format!("select count(*) from {table}")).await, 0, "{table}");
    }
    assert_eq!(app.count("select count(*) from consent_records").await, 3, "the proof of consent stays");
    assert!(
        sqlx::query("delete from consent_records").execute(&app.state.pool).await.is_err(),
        "and cannot be deleted"
    );
    assert_eq!(app.admin(Method::GET, &format!("/v1/children/{child}"), None).await.0, StatusCode::NOT_FOUND);
}

type OutboxRow = (uuid::Uuid, String, Option<String>, Value, chrono::DateTime<chrono::Utc>);

/// Every event in the outbox, as it would travel, checked against the
/// contracts at the repository root. Returns the event types in order.
async fn events_follow_the_contracts(app: &App) -> Vec<String> {
    let contracts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    let schema = |path: &str| {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(contracts.join(path)).unwrap()).unwrap();
        jsonschema::validator_for(&schema).unwrap()
    };
    let rows: Vec<OutboxRow> =
        sqlx::query_as("select id, type, subject, data, time from outbox order by time, id")
            .fetch_all(&app.state.pool)
            .await
            .unwrap();
    let envelope_schema = schema("envelope.schema.json");
    rows.into_iter()
        .map(|(id, event_type, subject, data, time)| {
            let event = events::envelope(id, &event_type, subject.as_deref(), time, &data);
            assert!(envelope_schema.is_valid(&event), "envelope of {event_type}: {event}");
            assert!(
                schema(&format!("events/{event_type}.schema.json")).is_valid(&data),
                "{event_type}: {data}"
            );
            event_type
        })
        .collect()
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn a_guardian_signs_in_with_a_code_and_reaches_only_their_family(pool: PgPool) {
    let app = app(pool);
    let start = |email: &str| app.call(Method::POST, "/v1/auth/start", None, Some(json!({ "email": email })));
    let verify = |body: Value| app.call(Method::POST, "/v1/auth/verify", None, Some(body));

    // A wrong code is refused and counted; the right one still works after it.
    assert_eq!(start(" Ana@Example.org ").await.0, StatusCode::NO_CONTENT);
    let (to, subject, _) = app.mail.sent.lock().unwrap()[0].clone();
    assert_eq!(to, "ana@example.org");
    let code = subject[..6].to_string();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert_eq!(verify(json!({ "email": "ana@example.org", "code": wrong })).await.0, StatusCode::FORBIDDEN);
    // A new address needs a name; the code survives the question.
    let (status, body) = verify(json!({ "email": "ana@example.org", "code": code })).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_request")));
    let (status, account) = verify(json!({ "email": "ANA@example.org", "code": code, "name": "Ana" })).await;
    assert_eq!(status, StatusCode::OK, "{account}");
    assert_eq!(account["new_account"], true);
    let ana = account["token"].as_str().unwrap().to_string();
    // A code works once.
    assert_eq!(
        verify(json!({ "email": "ana@example.org", "code": code, "name": "Ana" })).await.0,
        StatusCode::FORBIDDEN
    );

    // Signing in again finds the same family.
    let again = app.sign_in("ana@example.org", "ignored").await;
    let (_, me) = app.call(Method::GET, "/v1/me", Some(&again), None).await;
    assert_eq!(me["family_id"], account["family_id"]);
    assert_eq!(me["guardian"]["name"], "Ana");

    // Ana adds a child and answers consent herself.
    let family = account["family_id"].as_str().unwrap();
    let (status, child) = app
        .call(
            Method::POST,
            &format!("/v1/families/{family}/children"),
            Some(&ana),
            Some(json!({ "nickname": "Mateo" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{child}");
    let child = child["id"].as_str().unwrap();
    let (status, _) = app
        .call(
            Method::POST,
            &format!("/v1/children/{child}/consents"),
            Some(&ana),
            Some(json!({ "purpose": "voice", "granted": true, "policy_version": "2026-11" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (source, by): (String, Option<uuid::Uuid>) =
        sqlx::query_as("select source, guardian_id from consent_records")
            .fetch_one(&app.state.pool)
            .await
            .unwrap();
    assert_eq!(
        (source.as_str(), by.map(|id| id.to_string())),
        ("app", account["guardian_id"].as_str().map(str::to_string))
    );

    // Another family cannot see or touch Mateo, and cannot use the operator's routes.
    let luis = app.sign_in("luis@example.org", "Luis").await;
    for (method, path, body) in [
        (Method::GET, format!("/v1/children/{child}"), None),
        (Method::GET, format!("/v1/children/{child}/skills"), None),
        (Method::GET, format!("/v1/children/{child}/memories"), None),
        (Method::DELETE, format!("/v1/children/{child}"), None),
        (Method::POST, format!("/v1/families/{family}/children"), Some(json!({ "nickname": "X" }))),
        (Method::POST, "/v1/sessions".to_string(), Some(json!({ "child_id": child, "audience": "child" }))),
    ] {
        assert_eq!(app.call(method, &path, Some(&luis), body).await.0, StatusCode::NOT_FOUND, "{path}");
    }
    let operator_only =
        app.call(Method::POST, "/v1/sessions", Some(&luis), Some(json!({ "audience": "operator" }))).await;
    assert_eq!(operator_only.0, StatusCode::FORBIDDEN);
    assert_eq!(
        app.call(Method::PUT, "/v1/skills", Some(&luis), Some(json!({ "skills": [] }))).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.call(
            Method::POST,
            "/v1/sessions",
            Some(&ana),
            Some(json!({ "child_id": child, "audience": "child" }))
        )
        .await
        .0,
        StatusCode::CREATED
    );

    // Signing out ends that session only.
    assert_eq!(
        app.call(Method::POST, "/v1/auth/signout", Some(&again), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(app.call(Method::GET, "/v1/me", Some(&again), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.call(Method::GET, "/v1/me", Some(&ana), None).await.0, StatusCode::OK);

    // Erasing the family needs the address typed again, and leaves only the proof of consent.
    let erase = |confirm: &str| {
        app.call(Method::DELETE, "/v1/me", Some(&ana), Some(json!({ "confirm_email": confirm })))
    };
    assert_eq!(erase("luis@example.org").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(erase("ana@example.org").await.0, StatusCode::NO_CONTENT);
    assert_eq!(app.call(Method::GET, "/v1/me", Some(&ana), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.count("select count(*) from children").await, 0);
    assert_eq!(app.count("select count(*) from consent_records").await, 1);
    assert_eq!(app.count("select count(*) from families").await, 1, "Luis is still there");
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn sign_in_codes_are_limited(pool: PgPool) {
    let app = app(pool);
    let start =
        || app.call(Method::POST, "/v1/auth/start", None, Some(json!({ "email": "ana@example.org" })));
    let verify = |code: &str| {
        app.call(
            Method::POST,
            "/v1/auth/verify",
            None,
            Some(json!({ "email": "ana@example.org", "code": code, "name": "Ana" })),
        )
    };
    assert_eq!(
        app.call(Method::POST, "/v1/auth/start", None, Some(json!({ "email": "nope" }))).await.0,
        StatusCode::BAD_REQUEST
    );

    // Five wrong guesses burn the code, even for the right one.
    start().await;
    let code = app.mail.sent.lock().unwrap()[0].1[..6].to_string();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        assert_eq!(verify(wrong).await.0, StatusCode::FORBIDDEN);
    }
    assert_eq!(verify(&code).await.0, StatusCode::FORBIDDEN);

    // An address gets five codes an hour; asking again answers the same and sends nothing.
    for _ in 0..6 {
        assert_eq!(start().await.0, StatusCode::NO_CONTENT);
    }
    assert_eq!(app.mail.sent.lock().unwrap().len(), 5);
    // An expired code does not work.
    let code = app.mail.sent.lock().unwrap().last().unwrap().1[..6].to_string();
    sqlx::query("update login_codes set expires_at = now() - interval '1 second'")
        .execute(&app.state.pool)
        .await
        .unwrap();
    assert_eq!(verify(&code).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.count("select count(*) from guardians").await, 0);
}

async fn claim(app: &App, token: &str, code: &str, child: Option<&str>) -> (StatusCode, Value) {
    app.call(Method::POST, "/v1/devices/claim", Some(token), Some(json!({ "code": code, "child_id": child })))
        .await
}

fn sign(key: &[u8; 32], message: &str) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).unwrap();
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn a_device_is_linked_by_its_code_and_its_key_and_then_starts_sessions(pool: PgPool) {
    let app = app(pool);
    let key = [7u8; 32];
    let activate = |challenge: String, hmac: String| {
        app.call(
            Method::POST,
            "/ota/activate",
            None,
            Some(json!({ "algorithm": "hmac-sha256", "serial_number": "CAP-0001", "challenge": challenge, "hmac": hmac })),
        )
    };

    // A device nobody registered gets the time and nothing else.
    let unknown = app.check_in("CAP-9999", "1.0.0").await;
    assert!(unknown["server_time"]["timestamp"].as_i64().unwrap() > 0);
    assert!(unknown.get("activation").is_none() && unknown.get("firmware").is_none());

    let devices = json!({ "devices": [{ "serial": "CAP-0001", "hmac_key": hex::encode(key), "model": "capricorn-v0.2" }] });
    let (status, body) = app.admin(Method::POST, "/v1/devices", Some(devices.clone())).await;
    assert_eq!((status, &body["registered"]), (StatusCode::CREATED, &json!(1)));
    let (_, body) = app.admin(Method::POST, "/v1/devices", Some(devices)).await;
    assert_eq!((&body["registered"], &body["already_known"]), (&json!(0), &json!(1)));

    // Firmware: only a newer version is offered, never an older one.
    let firmware = |version: &str| json!({ "model": "capricorn-v0.2", "version": version, "url": format!("https://example.org/{version}.bin"), "sha256": "ab".repeat(32) });
    assert_eq!(app.admin(Method::POST, "/v1/firmware", Some(firmware("1.1.0"))).await.0, StatusCode::CREATED);
    assert_eq!(
        app.admin(Method::POST, "/v1/firmware", Some(firmware("1.1.0"))).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.admin(Method::POST, "/v1/firmware", Some(firmware("latest"))).await.0,
        StatusCode::BAD_REQUEST
    );
    let first = app.check_in("CAP-0001", "1.0.0").await;
    assert_eq!(first["firmware"], json!({ "version": "1.1.0", "url": "https://example.org/1.1.0.bin" }));
    assert_eq!(app.check_in("CAP-0001", "1.2.0").await["firmware"], json!({ "version": "1.2.0", "url": "" }));
    assert_eq!(app.check_in("CAP-0001", "1.10.0").await["firmware"]["url"], "");

    // Not linked yet: it shows a code, the same one while it is valid.
    let code = first["activation"]["code"].as_str().unwrap().to_string();
    let challenge = first["activation"]["challenge"].as_str().unwrap().to_string();
    assert_eq!(app.check_in("CAP-0001", "1.2.0").await["activation"]["code"], code.as_str());

    // The device proves it holds its key; a wrong signature or another challenge does not count.
    assert_eq!(activate(challenge.clone(), sign(&[8u8; 32], &challenge)).await.0, StatusCode::FORBIDDEN);
    assert_eq!(activate("other".into(), sign(&key, "other")).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        activate(challenge.clone(), sign(&key, &challenge)).await.0,
        StatusCode::ACCEPTED,
        "waiting for a guardian"
    );

    // A guardian types the code. A wrong one is not found; so is another family's child.
    let ana = app.sign_in("ana@example.org", "Ana").await;
    let (_, me) = app.call(Method::GET, "/v1/me", Some(&ana), None).await;
    let family = me["family_id"].as_str().unwrap().to_string();
    let (_, child) = app
        .call(
            Method::POST,
            &format!("/v1/families/{family}/children"),
            Some(&ana),
            Some(json!({ "nickname": "Mateo" })),
        )
        .await;
    let child = child["id"].as_str().unwrap().to_string();
    assert_eq!(claim(&app, &ana, "ZZZZZZ", None).await.0, StatusCode::NOT_FOUND);
    let luis = app.sign_in("luis@example.org", "Luis").await;
    assert_eq!(claim(&app, &luis, &code, Some(&child)).await.0, StatusCode::NOT_FOUND);
    let (status, claimed) = claim(&app, &ana, &code.to_lowercase(), Some(&child)).await;
    assert_eq!((status, claimed["status"].as_str()), (StatusCode::OK, Some("claimed")));
    assert_eq!(activate(challenge.clone(), sign(&key, &challenge)).await.0, StatusCode::OK);
    assert!(
        app.check_in("CAP-0001", "1.2.0").await.get("activation").is_none(),
        "a linked device shows no code"
    );
    let (_, mine) = app.call(Method::GET, "/v1/devices", Some(&ana), None).await;
    assert_eq!(mine["devices"][0]["serial"], "CAP-0001");
    assert!(mine["devices"][0].get("hmac_key").is_none());
    assert_eq!(app.call(Method::GET, "/v1/devices", Some(&luis), None).await.1["devices"], json!([]));

    // Sessions: signed, fresh, once, and only with consent.
    let session = |timestamp: i64, hmac: String| {
        app.call(
            Method::POST,
            "/ota/session",
            None,
            Some(json!({ "serial_number": "CAP-0001", "timestamp": timestamp, "hmac": hmac })),
        )
    };
    let now = chrono::Utc::now().timestamp();
    let signed = |timestamp: i64| sign(&key, &format!("session:CAP-0001:{timestamp}"));
    assert_eq!(session(now, signed(now)).await.0, StatusCode::FORBIDDEN, "no consent for voice yet");
    app.call(
        Method::POST,
        &format!("/v1/children/{child}/consents"),
        Some(&ana),
        Some(json!({ "purpose": "voice", "granted": true, "policy_version": "2026-11" })),
    )
    .await;
    assert_eq!(session(now, signed(now)).await.0, StatusCode::FORBIDDEN, "that timestamp was already used");
    assert_eq!(
        session(now + 1, sign(&[8u8; 32], &format!("session:CAP-0001:{}", now + 1))).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(session(now - 3600, signed(now - 3600)).await.0, StatusCode::FORBIDDEN, "too old");
    let (status, started) = session(now + 2, signed(now + 2)).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let claims = started["token"].as_str().unwrap().split('.').nth(1).unwrap().to_string();
    let claims: Value =
        serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(claims).unwrap())
            .unwrap();
    let dispatch: Value =
        serde_json::from_str(claims["roomConfig"]["agents"][0]["metadata"].as_str().unwrap()).unwrap();
    assert_eq!(dispatch["audience"], "child");

    // The session's end is an event, and so was the claim.
    let agent = dispatch["platform_token"].as_str().unwrap();
    let (status, _) = app
        .call(
            Method::POST,
            "/v1/agent/session/end",
            Some(agent),
            Some(json!({ "end_reason": "idle", "speaking_seconds": 12 })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.call(Method::POST, "/v1/agent/session/end", Some(agent), Some(json!({ "end_reason": "bored" })))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(events_follow_the_contracts(&app).await, ["device.claimed.v1", "session.ended.v1"]);

    // Releasing the device unlinks it: it shows a new code and gets no sessions.
    let device = claimed["device_id"].as_str().unwrap();
    assert_eq!(
        app.call(Method::DELETE, &format!("/v1/devices/{device}"), Some(&luis), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.call(Method::DELETE, &format!("/v1/devices/{device}"), Some(&ana), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert!(app.check_in("CAP-0001", "1.2.0").await["activation"]["code"].is_string());
    assert_eq!(session(now + 3, signed(now + 3)).await.0, StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "capricorn_platform::MIGRATOR")]
async fn events_become_notices_for_the_family_and_reach_the_webhook(pool: PgPool) {
    // A stand-in for whoever receives the platform's events.
    let received: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}/hook", listener.local_addr().unwrap().port());
    let seen = received.clone();
    let receiver = Router::new().route(
        "/hook",
        axum::routing::post(move |headers: axum::http::HeaderMap, body: String| async move {
            let signature = headers["X-Capricorn-Signature"].to_str().unwrap().to_string();
            seen.lock().unwrap().push((signature, body));
            StatusCode::NO_CONTENT
        }),
    );
    tokio::spawn(async move { axum::serve(listener, receiver).await.unwrap() });
    let secret = "a-webhook-secret-of-32-characters!";
    let app = app_with(pool, Some(EventsWebhook { url, secret: secret.into() }));

    let ana = app.sign_in("ana@example.org", "Ana").await;
    let (_, me) = app.call(Method::GET, "/v1/me", Some(&ana), None).await;
    let family = me["family_id"].as_str().unwrap();
    let (_, child) = app
        .call(
            Method::POST,
            &format!("/v1/families/{family}/children"),
            Some(&ana),
            Some(json!({ "nickname": "Mateo" })),
        )
        .await;
    let child = child["id"].as_str().unwrap().to_string();
    app.call(
        Method::POST,
        &format!("/v1/children/{child}/consents"),
        Some(&ana),
        Some(json!({ "purpose": "voice", "granted": true, "policy_version": "2026-11" })),
    )
    .await;
    let (_, token) = app.session(&child).await;
    let agent = Some(token.as_str());
    app.mail.sent.lock().unwrap().clear();

    // A gate stops a turn: the agent reports categories, never words.
    let flag = |body: Value| app.call(Method::POST, "/v1/agent/session/flags", agent, Some(body));
    let stopped = json!({ "gate": "moderation_input", "action": "support", "categories": ["self_harm"] });
    assert_eq!(flag(stopped.clone()).await.0, StatusCode::NO_CONTENT);
    assert_eq!(flag(stopped).await.0, StatusCode::NO_CONTENT);
    assert_eq!(
        flag(json!({ "gate": "moderation_output", "action": "redirect", "categories": ["violence"] }))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for bad in [
        json!({ "gate": "curiosity", "action": "support", "categories": ["self_harm"] }),
        json!({ "gate": "moderation_input", "action": "support", "categories": [] }),
        json!({ "gate": "moderation_input", "action": "support", "categories": ["She said: I am sad"] }),
    ] {
        assert_eq!(flag(bad).await.0, StatusCode::BAD_REQUEST);
    }

    app.call(
        Method::POST,
        "/v1/agent/session/turns",
        agent,
        Some(json!({ "turns": [{ "seq": 0, "role": "child", "content": "Hola" }] })),
    )
    .await;
    app.call(Method::POST, "/v1/agent/session/end", agent, Some(json!({}))).await;
    eventually("guidance settled", || async { app.state.watching.lock().unwrap().is_empty() }).await;
    assert!(jobs::work_once(&app.state).await.unwrap());
    let types = events_follow_the_contracts(&app).await;
    assert_eq!(types, ["safety.flagged.v1", "safety.flagged.v1", "safety.flagged.v1", "session.reviewed.v1"]);

    app.deliver_events().await;
    // One note that something needs attention, however many flags, and one summary.
    let (_, notices) = app.call(Method::GET, "/v1/notices", Some(&ana), None).await;
    let notices = notices["notices"].as_array().unwrap().clone();
    let kinds: Vec<&str> = notices.iter().map(|notice| notice["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["session_summary", "attention"]);
    assert_eq!(notices[0]["body"], "Practicó sumas de fracciones y mejoró con una pista.");
    assert!(notices[1]["title"].as_str().unwrap().contains("Mateo"));
    assert!(!notices[1]["body"].as_str().unwrap().contains("self_harm"));
    // Both were emailed to the guardian, once.
    let mail = app.mail.sent.lock().unwrap().clone();
    assert_eq!(mail.len(), 2);
    assert!(mail.iter().all(|(to, _, _)| to == "ana@example.org"));

    // Delivering the same events again changes nothing.
    sqlx::query("update outbox set delivered_at = null").execute(&app.state.pool).await.unwrap();
    app.deliver_events().await;
    assert_eq!(app.count("select count(*) from notices").await, 2);
    assert_eq!(app.mail.sent.lock().unwrap().len(), 2);

    // The webhook got every event, signed over the exact body.
    let received = received.lock().unwrap().clone();
    assert_eq!(received.len(), 8);
    for (signature, body) in &received {
        assert_eq!(signature, &events::signature(secret, body.as_bytes()));
        assert_eq!(serde_json::from_str::<Value>(body).unwrap()["source"], "capricorn/platform");
    }

    // Reading a notice, and a guardian who turns email off.
    let (_, me) = app.call(Method::GET, "/v1/me", Some(&ana), None).await;
    assert_eq!(me["unread_notices"], 2);
    let id = notices[0]["id"].as_str().unwrap();
    assert_eq!(
        app.call(Method::POST, &format!("/v1/notices/{id}/read"), Some(&ana), None).await.0,
        StatusCode::NO_CONTENT
    );
    let luis = app.sign_in("luis@example.org", "Luis").await;
    assert_eq!(
        app.call(Method::POST, &format!("/v1/notices/{id}/read"), Some(&luis), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(app.call(Method::GET, "/v1/notices", Some(&luis), None).await.1["notices"], json!([]));
    let (_, me) = app.call(Method::GET, "/v1/me", Some(&ana), None).await;
    assert_eq!(me["unread_notices"], 1);
    assert_eq!(
        app.call(Method::PUT, "/v1/me", Some(&ana), Some(json!({ "notices_by_email": false }))).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(app.count("select count(*) from guardians where not notices_by_email").await, 1);
}

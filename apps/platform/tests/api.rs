//! The platform end to end, against a real Postgres with pgvector: each test
//! gets its own empty database. The language model and the embedding model
//! are fakes, so nothing leaves the machine.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use capricorn_platform::config::{Config, LiveKit};
use capricorn_platform::llm::{Llm, LlmError, Request as LlmRequest};
use capricorn_platform::memory::{DIMENSIONS, Embedder, MemoryError};
use capricorn_platform::state::AppState;
use capricorn_platform::{http, jobs};
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

struct App {
    router: Router,
    state: AppState,
    llm: Arc<ScriptedLlm>,
}

fn app(pool: PgPool) -> App {
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
    };
    let llm = Arc::new(ScriptedLlm { asked: Mutex::new(Vec::new()) });
    let state = AppState {
        pool,
        config: Arc::new(config),
        llm: Some(llm.clone()),
        embedder: Some(Arc::new(WordEmbedder)),
        watching: Arc::new(Mutex::new(HashSet::new())),
    };
    App { router: http::router(state.clone()), state, llm }
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
                Some(json!({ "nickname": "Mateo", "birth_year": 2016, "grade": "4 primaria", "interests": ["football"] })),
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

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use bson::doc;
use chrono::{DateTime, Datelike, Duration, Utc};
use mongodb::{
    options::{ClientOptions, FindOptions, IndexOptions, UpdateOptions},
    Client, Collection, IndexModel,
};
use serde::{Deserialize, Serialize};
use futures_util::TryStreamExt;
use std::{env, sync::Arc};
use tokio::time::{sleep, Duration as TokioDuration};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EmailStatus {
    Queued,
    Sending,
    Sent,
    Delivered,
    Bounced,
    SpamReported,
    Suppressed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    pub id: String,
    pub to: String,
    pub name: String,
    pub subject: String,
    pub html: String,
    pub campaign: String,
    pub status: EmailStatus,
    pub attempts: u32,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub next_attempt_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SendRequest {
    pub to: String,
    pub name: Option<String>,
    pub subject: String,
    pub html: String,
    pub campaign: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    pub messages: Vec<SendRequest>,
}

#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct BatchResponse {
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UnsubscribeRequest {
    pub email: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suppression {
    pub email: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateCounter {
    pub key: String,
    pub window: DateTime<Utc>,
    pub count: u32,
}

#[derive(Clone)]
pub struct AppState {
    pub messages: Collection<EmailMessage>,
    pub suppressions: Collection<Suppression>,
    pub rate_counters: Collection<RateCounter>,
    pub http: reqwest::Client,
    pub brevo_api_key: String,
    pub mail_from_email: String,
    pub mail_from_name: String,
    pub per_recipient_day: u32,
    pub global_per_hour: u32,
    pub warmup_max_per_hour: u32,
    pub warmup_days: u32,
    pub worker_lock: Arc<tokio::sync::Mutex<()>>,
}

pub type ApiError = (StatusCode, String);

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health_check))
        .route("/api/email/health", get(health_check))
        .route("/api/email/send", post(enqueue_single))
        .route("/api/email/batch", post(enqueue_batch))
        .route("/api/email/status/:id", get(get_status))
        .route("/api/email/unsubscribe", post(unsubscribe))
        .layer(cors)
        .with_state(state)
}

pub async fn health_check(State(state): State<AppState>) -> Json<serde_json::Value> {
    let provider_configured = !state.brevo_api_key.is_empty() && !state.mail_from_email.is_empty();
    let queued = state
        .messages
        .count_documents(doc! { "status": { "$in": ["queued", "sending"] } }, None)
        .await
        .unwrap_or(0);
    Json(serde_json::json!({
        "status": "ok",
        "provider_configured": provider_configured,
        "queued": queued,
    }))
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn is_valid_email(email: &str) -> bool {
    let email = normalize_email(email);
    let mut parts = email.splitn(2, '@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    !local.is_empty()
        && domain.contains('.')
        && domain.len() >= 4
        && !local.contains(' ')
        && !email.contains("..")
}

pub async fn enqueue_single(
    State(state): State<AppState>,
    Json(payload): Json<SendRequest>,
) -> Result<(StatusCode, Json<SendResponse>), ApiError> {
    let id = enqueue_message(&state, payload).await?;
    Ok((StatusCode::ACCEPTED, Json(SendResponse { id })))
}

pub async fn enqueue_batch(
    State(state): State<AppState>,
    Json(payload): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, ApiError> {
    if payload.messages.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            "batch is capped at 500 messages per request".into(),
        ));
    }
    let mut ids = Vec::with_capacity(payload.messages.len());
    for message in payload.messages {
        ids.push(enqueue_message(&state, message).await?);
    }
    Ok(Json(BatchResponse { ids }))
}

pub async fn enqueue_message(state: &AppState, payload: SendRequest) -> Result<String, ApiError> {
    let to = normalize_email(&payload.to);
    if !is_valid_email(&to) {
        return Err((StatusCode::BAD_REQUEST, "invalid recipient email".into()));
    }
    if payload.subject.trim().is_empty() || payload.html.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "subject and html are required".into()));
    }
    let suppressed = state
        .suppressions
        .find_one(doc! { "email": &to }, None)
        .await
        .map_err(db_error)?;
    if suppressed.is_some() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "recipient is suppressed (unsubscribed or hard bounced)".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let message = EmailMessage {
        id: id.clone(),
        to,
        name: payload.name.unwrap_or_default(),
        subject: payload.subject,
        html: payload.html,
        campaign: payload.campaign.unwrap_or_else(|| "transactional".to_string()),
        status: EmailStatus::Queued,
        attempts: 0,
        error: None,
        created_at: now,
        sent_at: None,
        next_attempt_at: now,
    };
    state
        .messages
        .insert_one(&message, None)
        .await
        .map_err(db_error)?;
    tracing::info!(id = %id, to = %message.to, "email-manager queued message");
    Ok(id)
}

pub async fn get_status(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<EmailMessage>, ApiError> {
    state
        .messages
        .find_one(doc! { "id": &id }, None)
        .await
        .map_err(db_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "message not found".to_string()))
}

pub async fn unsubscribe(
    State(state): State<AppState>,
    Json(payload): Json<UnsubscribeRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let email = normalize_email(&payload.email);
    if !is_valid_email(&email) {
        return Err((StatusCode::BAD_REQUEST, "invalid email".into()));
    }
    let suppression = Suppression {
        email,
        reason: payload.reason.unwrap_or_else(|| "user_requested".to_string()),
        created_at: Utc::now(),
    };
    state
        .suppressions
        .update_one(
            doc! { "email": &suppression.email },
            doc! { "$setOnInsert": { "reason": &suppression.reason, "created_at": suppression.created_at } },
            UpdateOptions::builder()
                .upsert(true)
                .build(),
        )
        .await
        .map_err(db_error)?;
    state
        .messages
        .update_many(
            doc! { "to": &suppression.email, "status": { "$in": ["queued", "sending", "failed"] } },
            doc! { "$set": { "status": "suppressed" } },
            None,
        )
        .await
        .map_err(db_error)?;
    tracing::info!(email = %suppression.email, "email-manager added suppression");
    Ok((StatusCode::OK, Json(serde_json::json!({ "suppressed": true }))))
}

pub fn spawn_worker(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = worker_tick(&state).await {
                tracing::warn!(error = %error, "email-manager worker tick failed");
            }
            sleep(TokioDuration::from_secs(2)).await;
        }
    });
}

pub(crate) async fn worker_tick(state: &AppState) -> anyhow::Result<()> {
    let _guard = state.worker_lock.lock().await;

    let provider_ready = !state.brevo_api_key.is_empty() && !state.mail_from_email.is_empty();
    if !provider_ready {
        tracing::warn!("email-manager provider not configured; queue paused");
        return Ok(());
    }

    let current_hour = Utc::now();
    let global_key = format!(
        "global-hour-{}-{}",
        current_hour.year(),
        current_hour.ordinal()
    );
    let global_counter = state
        .rate_counters
        .find_one(doc! { "key": &global_key }, None)
        .await?;
    let global_count = global_counter.map(|c| c.count).unwrap_or(0);

    let warmup_hour = warmup_rate(state, current_hour);
    let budget_this_hour = warmup_hour.min(state.global_per_hour);
    if global_count >= budget_this_hour {
        return Ok(());
    }
    let remaining_budget = budget_this_hour.saturating_sub(global_count);
    let to_process: u32 = remaining_budget.min(20);

    let mut cursor = state
        .messages
        .find(
            doc! {
                "status": { "$in": ["queued", "sending"] },
                "next_attempt_at": { "$lte": Utc::now() }
            },
            FindOptions::builder()
                .sort(doc! { "created_at": 1 })
                .limit(to_process as i64)
                .build(),
        )
        .await?;

    let mut messages = Vec::new();
    while let Some(message) = cursor.try_next().await? {
        messages.push(message);
    }

    for message in messages {
        if let Err(error) = send_one(state, &message).await {
            tracing::error!(id = %message.id, error = %error, "email-manager send failed");
            let attempts = message.attempts + 1;
            let backoff = TokioDuration::from_secs((30u64).saturating_mul(attempts as u64).min(1800));
            state
                .messages
                .update_one(
                    doc! { "id": &message.id },
                    doc! {
                        "$set": {
                            "status": if attempts >= 5 { "failed" } else { "queued" },
                            "attempts": attempts,
                            "error": error.to_string(),
                            "next_attempt_at": Utc::now() + Duration::seconds(backoff.as_secs() as i64)
                        }
                    },
                    None,
                )
                .await?;
        } else {
            state
                .rate_counters
                .update_one(
                    doc! { "key": &global_key },
                    doc! { "$inc": { "count": 1 }, "$setOnInsert": { "window": current_hour } },
                    UpdateOptions::builder().upsert(true).build(),
                )
                .await?;
            let recipient_key = format!("recipient-day-{}", message.to);
            state
                .rate_counters
                .update_one(
                    doc! { "key": &recipient_key },
                    doc! { "$inc": { "count": 1 }, "$setOnInsert": { "window": current_hour } },
                    UpdateOptions::builder().upsert(true).build(),
                )
                .await?;
            tracing::info!(id = %message.id, to = %message.to, "email-manager sent via Brevo");
        }
    }
    Ok(())
}

pub(crate) fn warmup_rate(state: &AppState, now: DateTime<Utc>) -> u32 {
    if state.warmup_days == 0 || state.warmup_max_per_hour == 0 {
        return state.warmup_max_per_hour;
    }
    let day_index = now.ordinal() % state.warmup_days;
    let max = state.warmup_max_per_hour;
    let step = max / state.warmup_days.max(1);
    (step * (day_index + 1)).min(max).max(1)
}

pub(crate) async fn send_one(state: &AppState, message: &EmailMessage) -> anyhow::Result<()> {
    let suppressed = state
        .suppressions
        .find_one(doc! { "email": &message.to }, None)
        .await?;
    if suppressed.is_some() {
        state
            .messages
            .update_one(
                doc! { "id": &message.id },
                doc! { "$set": { "status": "suppressed", "error": "recipient suppressed" } },
                None,
            )
            .await?;
        return Ok(());
    }

    state
        .messages
        .update_one(
            doc! { "id": &message.id },
            doc! { "$set": { "status": "sending", "attempts": message.attempts + 1 } },
            None,
        )
        .await?;

    let payload = serde_json::json!({
        "sender": { "email": state.mail_from_email, "name": state.mail_from_name },
        "to": [{ "email": message.to, "name": message.name }],
        "subject": message.subject,
        "htmlContent": format!(
            "{}<p style=\"margin-top:32px;padding-top:16px;border-top:1px solid #e5e7eb;font-size:12px;color:#9ca3af;\">You are receiving this because you signed up for updates from Eco. <a href=\"{}/unsubscribe?email={}\">Unsubscribe</a>.</p>",
            message.html,
            state.public_unsubscribe_url(),
            message.to,
        ),
        "headers": {
            "List-Unsubscribe": format!("<{}/unsubscribe?email={}>", state.public_unsubscribe_url(), message.to),
            "X-Eco-Campaign": message.campaign,
        }
    });

    let response = state
        .http
        .post("https://api.brevo.com/v3/smtp/email")
        .header("api-key", &state.brevo_api_key)
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let message_id = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("messageId").and_then(|id| id.as_str()).map(str::to_owned));
        let permanent = status == StatusCode::BAD_REQUEST
            || body.contains("invalid")
            || body.contains("bounce")
            || body.contains("suppress");
        if permanent {
            let suppression = Suppression {
                email: message.to.clone(),
                reason: format!("brevo_rejected:{}", body.chars().take(80).collect::<String>()),
                created_at: Utc::now(),
            };
            state
                .suppressions
                .update_one(
                    doc! { "email": &suppression.email },
                    doc! { "$setOnInsert": { "reason": &suppression.reason, "created_at": suppression.created_at } },
                    UpdateOptions::builder().upsert(true).build(),
                )
                .await?;
        }
        anyhow::bail!(
            "Brevo rejected email (status {status}, message_id {})",
            message_id.unwrap_or_default()
        );
    }
    let sent_at = Utc::now();
    state
        .messages
        .update_one(
            doc! { "id": &message.id },
            doc! { "$set": { "status": "sent", "sent_at": sent_at, "error": null } },
            None,
        )
        .await?;
    Ok(())
}

pub fn db_error(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %error, "email-manager MongoDB operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "Email storage is having trouble.".into())
}

impl AppState {
    pub fn public_unsubscribe_url(&self) -> String {
        env::var("EMAIL_MANAGER_PUBLIC_URL")
            .unwrap_or_else(|_| "https://eco.stuff8.com".to_string())
            .trim_end_matches('/')
            .to_string()
    }
}

pub async fn bootstrap() -> anyhow::Result<axum::Router> {
    let uri = std::env::var("MONGODB_URI")
        .unwrap_or_else(|_| "mongodb://127.0.0.1:27017/email_manager".to_string());
    let options = ClientOptions::parse(&uri).await?;
    let client = Client::with_options(options)?;
    let db = client
        .default_database()
        .unwrap_or_else(|| client.database("email_manager"));
    let messages = db.collection("messages");
    let suppressions = db.collection("suppressions");
    let rate_counters = db.collection("rate_counters");
    messages
        .create_index(
            IndexModel::builder()
                .keys(doc! { "id": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
            None,
        )
        .await?;
    messages
        .create_index(
            IndexModel::builder()
                .keys(doc! { "status": 1, "next_attempt_at": 1 })
                .build(),
            None,
        )
        .await?;
    suppressions
        .create_index(
            IndexModel::builder()
                .keys(doc! { "email": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
            None,
        )
        .await?;
    let state = AppState {
        messages,
        suppressions,
        rate_counters,
        http: reqwest::Client::new(),
        brevo_api_key: std::env::var("BREVO_API_KEY").unwrap_or_default(),
        mail_from_email: std::env::var("MAIL_FROM_EMAIL").unwrap_or_default(),
        mail_from_name: std::env::var("MAIL_FROM_NAME")
            .unwrap_or_else(|_| "Eco".to_string()),
        per_recipient_day: std::env::var("EMAIL_PER_RECIPIENT_DAY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5),
        global_per_hour: std::env::var("EMAIL_GLOBAL_PER_HOUR")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(400),
        warmup_max_per_hour: std::env::var("EMAIL_WARMUP_MAX_PER_HOUR")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(400),
        warmup_days: std::env::var("EMAIL_WARMUP_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5),
        worker_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
    spawn_worker(state.clone());
    Ok(build_router(state))
}

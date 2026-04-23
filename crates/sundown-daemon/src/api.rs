use actix_web::{HttpRequest, HttpResponse, web};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::bridge::TimekprBridge;

pub struct AppState {
    pub bridge: Arc<TimekprBridge>,
    pub token: String,
}

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    fn success(data: T) -> HttpResponse {
        HttpResponse::Ok().json(ApiResponse {
            ok: true,
            data: Some(data),
            error: None,
        })
    }
}

fn error_response(msg: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(ApiResponse::<()> {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
    })
}

fn check_auth(req: &HttpRequest, state: &web::Data<AppState>) -> Result<(), HttpResponse> {
    let header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = header.strip_prefix("Bearer ").unwrap_or("");
    if token != state.token {
        return Err(HttpResponse::Unauthorized().json(ApiResponse::<()> {
            ok: false,
            data: None,
            error: Some("invalid or missing token".to_string()),
        }));
    }
    Ok(())
}

// ─── Status ──────────────────────────────────────────────────────────────────

pub async fn get_status(req: HttpRequest, state: web::Data<AppState>) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.get_status().await {
        Ok(status) => ApiResponse::success(status),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Full config ─────────────────────────────────────────────────────────────

pub async fn get_config(req: HttpRequest, state: web::Data<AppState>) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.get_config().await {
        Ok(config) => ApiResponse::success(config),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Time adjustments ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TimeAdjustRequest {
    pub seconds: i64,
    #[serde(default)]
    pub operation: Option<String>, // "add", "subtract", "set"
}

pub async fn adjust_time(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<TimeAdjustRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }

    if body.seconds < 0 {
        return error_response("seconds must be non-negative");
    }

    let op = body.operation.as_deref().unwrap_or("add");
    let result = match op {
        "add" => state.bridge.grant_time(body.seconds).await,
        "subtract" => state.bridge.subtract_time(body.seconds).await,
        "set" => state.bridge.set_time_left(body.seconds).await,
        _ => return error_response("operation must be 'add', 'subtract', or 'set'"),
    };

    match result {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Daily limits ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DailyLimitsRequest {
    pub daily: Vec<i64>,
}

pub async fn set_limits(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<DailyLimitsRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    if body.daily.is_empty() {
        return error_response("daily limits array must not be empty");
    }
    match state.bridge.set_daily_limits(&body.daily).await {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Weekly / Monthly limits ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PeriodLimitRequest {
    pub seconds: i64,
}

pub async fn set_weekly_limit(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<PeriodLimitRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.set_weekly_limit(body.seconds).await {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn set_monthly_limit(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<PeriodLimitRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.set_monthly_limit(body.seconds).await {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Allowed days ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AllowedDaysRequest {
    pub days: Vec<u8>,
}

pub async fn set_allowed_days(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<AllowedDaysRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.set_allowed_days(&body.days).await {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Allowed hours ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AllowedHoursRequest {
    pub day: String,
    pub hours: Vec<u8>,
}

pub async fn set_allowed_hours(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<AllowedHoursRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.set_allowed_hours(&body.day, &body.hours).await {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Options ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TrackInactiveRequest {
    pub enabled: bool,
}

pub async fn set_track_inactive(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<TrackInactiveRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.set_track_inactive(body.enabled).await {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct HideTrayRequest {
    pub hidden: bool,
}

pub async fn set_hide_tray_icon(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<HideTrayRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.set_hide_tray_icon(body.hidden).await {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct LockoutTypeRequest {
    pub lockout_type: String,
    #[serde(default = "default_wake")]
    pub wake_from: String,
    #[serde(default = "default_wake_to")]
    pub wake_to: String,
}

fn default_wake() -> String {
    "0".to_string()
}
fn default_wake_to() -> String {
    "23".to_string()
}

pub async fn set_lockout_type(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<LockoutTypeRequest>,
) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }

    let valid_types = [
        "lock",
        "suspend",
        "suspendwake",
        "terminate",
        "kill",
        "shutdown",
    ];
    if !valid_types.contains(&body.lockout_type.as_str()) {
        return error_response(&format!(
            "invalid lockout type, must be one of: {}",
            valid_types.join(", ")
        ));
    }

    match state
        .bridge
        .set_lockout_type(&body.lockout_type, &body.wake_from, &body.wake_to)
        .await
    {
        Ok(()) => ApiResponse::success("ok"),
        Err(e) => error_response(&e.to_string()),
    }
}

// ─── Lock / Unlock ───────────────────────────────────────────────────────────

pub async fn lock_user(req: HttpRequest, state: web::Data<AppState>) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.lock().await {
        Ok(()) => ApiResponse::success("locked"),
        Err(e) => error_response(&e.to_string()),
    }
}

pub async fn unlock_user(req: HttpRequest, state: web::Data<AppState>) -> HttpResponse {
    if let Err(resp) = check_auth(&req, &state) {
        return resp;
    }
    match state.bridge.unlock().await {
        Ok(()) => ApiResponse::success("unlocked"),
        Err(e) => error_response(&e.to_string()),
    }
}

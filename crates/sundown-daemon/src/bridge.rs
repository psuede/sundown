use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::zvariant::{self, ObjectPath, OwnedValue};

#[derive(Debug, Clone, Serialize)]
pub struct UserStatus {
    pub user: String,
    pub time_remaining_secs: i64,
    pub time_spent_today_secs: i64,
    pub time_spent_week_secs: i64,
    pub time_spent_month_secs: i64,
    pub time_inactive_secs: i64,
    pub daily_limit_secs: i64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserConfig {
    pub user: String,
    pub allowed_days: Vec<u8>,
    pub daily_limits_secs: Vec<i64>,
    pub allowed_hours: HashMap<String, Vec<HourInterval>>,
    pub weekly_limit_secs: i64,
    pub monthly_limit_secs: i64,
    pub track_inactive: bool,
    pub hide_tray_icon: bool,
    pub lockout_type: String,
    pub wakeup_from: u8,
    pub wakeup_to: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct HourInterval {
    pub from: String,
    pub to: String,
    pub unaccountable: bool,
}

pub struct TimekprBridge {
    user: String,
    inner: BridgeInner,
}

enum BridgeInner {
    Mock(Arc<Mutex<MockState>>),
    DBus { conn: zbus::Connection },
}

struct MockState {
    time_remaining: i64,
    daily_limits: Vec<i64>,
    allowed_days: Vec<u8>,
    weekly_limit: i64,
    monthly_limit: i64,
    track_inactive: bool,
    hide_tray_icon: bool,
    lockout_type: String,
    locked: bool,
}

const DEST: &str = "com.timekpr.server";
const PATH: &str = "/com/timekpr/server";
const IFACE_LIMITS: &str = "com.timekpr.server.user.limits";
const IFACE_ADMIN: &str = "com.timekpr.server.user.admin";

impl TimekprBridge {
    pub async fn connect(user: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = zbus::Connection::system().await?;
        let proxy = zbus::fdo::DBusProxy::new(&conn).await?;
        let names = proxy.list_names().await?;
        let found = names.iter().any(|n| n.as_str() == DEST);
        if !found {
            return Err("timekpr-next D-Bus service not found. Is timekpr.service running?".into());
        }
        Ok(Self {
            user: user.to_string(),
            inner: BridgeInner::DBus { conn },
        })
    }

    pub fn mock(user: &str) -> Self {
        tracing::info!("running in mock mode — no real timekpr-next connection");
        Self {
            user: user.to_string(),
            inner: BridgeInner::Mock(Arc::new(Mutex::new(MockState {
                time_remaining: 3600,
                daily_limits: vec![7200; 7],
                allowed_days: vec![1, 2, 3, 4, 5, 6, 7],
                weekly_limit: 50400,
                monthly_limit: 216000,
                track_inactive: false,
                hide_tray_icon: false,
                lockout_type: "terminate".to_string(),
                locked: false,
            }))),
        }
    }

    fn path() -> Result<ObjectPath<'static>, Box<dyn std::error::Error>> {
        Ok(ObjectPath::try_from(PATH)?.to_owned())
    }

    async fn call_admin(
        conn: &zbus::Connection,
        method: &str,
        body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> Result<(i32, String), Box<dyn std::error::Error>> {
        let reply = conn
            .call_method(Some(DEST), Self::path()?, Some(IFACE_ADMIN), method, body)
            .await?;
        let result: (i32, String) = reply.body().deserialize()?;
        if result.0 != 0 {
            return Err(format!("timekpr error: {}", result.1).into());
        }
        Ok(result)
    }

    async fn call_admin_info(
        conn: &zbus::Connection,
        user: &str,
    ) -> Result<HashMap<String, OwnedValue>, Box<dyn std::error::Error>> {
        let reply = conn
            .call_method(
                Some(DEST),
                Self::path()?,
                Some(IFACE_ADMIN),
                "getUserInformation",
                &(user, "F"),
            )
            .await?;

        // Return signature is isa{sv}
        // Try to deserialize; if the dict fails, log the raw body
        match reply.body().deserialize::<(i32, String, HashMap<String, OwnedValue>)>() {
            Ok((code, msg, data)) => {
                if code != 0 {
                    return Err(format!("timekpr error: {msg}").into());
                }
                Ok(data)
            }
            Err(e) => {
                // Try just getting code and message to see if the call itself succeeded
                let body = reply.body();
                let sig = body.signature();
                tracing::warn!("getUserInformation deserialize failed: {e}, signature: {sig:?}");

                // Fall back: try parsing the string field which contains semicolon-delimited data
                let (code, msg): (i32, String) = reply.body().deserialize()?;
                tracing::debug!("getUserInformation fallback: code={code}, msg_len={}", msg.len());
                if !msg.is_empty() {
                    tracing::debug!("getUserInformation raw msg (first 500): {}", &msg[..msg.len().min(500)]);
                }
                // Return empty map, caller will use defaults
                Ok(HashMap::new())
            }
        }
    }

    fn extract_i64(data: &HashMap<String, OwnedValue>, key: &str) -> i64 {
        data.get(key)
            .and_then(|v| {
                let val = v.downcast_ref().ok()?;
                // Try i64 first, then i32, then string parse
                if let Ok(n) = <i64 as TryFrom<&zvariant::Value<'_>>>::try_from(val) {
                    return Some(n);
                }
                if let Ok(n) = <i32 as TryFrom<&zvariant::Value<'_>>>::try_from(val) {
                    return Some(n as i64);
                }
                if let Ok(s) = <&str as TryFrom<&zvariant::Value<'_>>>::try_from(val) {
                    return s.parse().ok();
                }
                None
            })
            .unwrap_or(0)
    }

    fn extract_bool(data: &HashMap<String, OwnedValue>, key: &str) -> bool {
        data.get(key)
            .and_then(|v| {
                let val = v.downcast_ref().ok()?;
                <bool as TryFrom<&zvariant::Value<'_>>>::try_from(val).ok()
            })
            .unwrap_or(false)
    }

    fn extract_string(data: &HashMap<String, OwnedValue>, key: &str) -> String {
        data.get(key)
            .and_then(|v| {
                let val = v.downcast_ref().ok()?;
                <&str as TryFrom<&zvariant::Value<'_>>>::try_from(val)
                    .map(|s| s.to_string())
                    .ok()
            })
            .unwrap_or_default()
    }

    fn extract_i32_array(data: &HashMap<String, OwnedValue>, key: &str) -> Vec<i64> {
        data.get(key)
            .and_then(|v| {
                let val = v.downcast_ref().ok()?;
                if let zvariant::Value::Array(arr) = val {
                    let result: Vec<i64> = arr.iter()
                        .filter_map(|item| {
                            if let Ok(n) = <i32 as TryFrom<&zvariant::Value<'_>>>::try_from(item) {
                                Some(n as i64)
                            } else if let Ok(n) = <i64 as TryFrom<&zvariant::Value<'_>>>::try_from(item) {
                                Some(n)
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !result.is_empty() { return Some(result); }
                }
                None
            })
            .unwrap_or_default()
    }

    fn extract_string_array(data: &HashMap<String, OwnedValue>, key: &str) -> Vec<String> {
        data.get(key)
            .and_then(|v| {
                let val = v.downcast_ref().ok()?;
                if let zvariant::Value::Array(arr) = val {
                    let result: Vec<String> = arr.iter()
                        .filter_map(|item| {
                            <&str as TryFrom<&zvariant::Value<'_>>>::try_from(item)
                                .map(|s| s.to_string())
                                .ok()
                        })
                        .collect();
                    if !result.is_empty() { return Some(result); }
                }
                None
            })
            .unwrap_or_default()
    }

    pub async fn get_status(&self) -> Result<UserStatus, Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let s = state.lock().await;
                let limit = s.daily_limits.first().copied().unwrap_or(7200);
                Ok(UserStatus {
                    user: self.user.clone(),
                    time_remaining_secs: if s.locked { 0 } else { s.time_remaining },
                    daily_limit_secs: limit,
                    time_spent_today_secs: limit - s.time_remaining,
                    time_spent_week_secs: 0,
                    time_spent_month_secs: 0,
                    time_inactive_secs: 0,
                    active: !s.locked,
                })
            }
            BridgeInner::DBus { conn } => {
                let data = Self::call_admin_info(conn, &self.user).await?;

                tracing::debug!("getUserInformation keys: {:?}", data.keys().collect::<Vec<_>>());

                let time_left_raw = Self::extract_i64(&data, "TIME_LEFT_DAY");
                let time_spent_day = Self::extract_i64(&data, "TIME_SPENT_DAY");
                let time_spent_week = Self::extract_i64(&data, "TIME_SPENT_WEEK");
                let time_spent_month = Self::extract_i64(&data, "TIME_SPENT_MONTH");
                let time_inactive = Self::extract_i64(&data, "TIME_INACTIVE");
                let time_spent_balance = Self::extract_i64(&data, "TIME_SPENT_BALANCE");

                let daily_limits = Self::extract_i32_array(&data, "LIMITS_PER_WEEKDAYS");
                // Get today's weekday (Monday=0 in our array, matching timekpr's ISO weekday)
                let today_idx = {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    // Unix epoch (1970-01-01) was a Thursday (day 3, 0-indexed from Monday)
                    ((now / 86400 + 3) % 7) as usize
                };
                let limit_day = daily_limits.get(today_idx).copied()
                    .unwrap_or_else(|| daily_limits.first().copied().unwrap_or(0));

                // TIME_LEFT_DAY is 0 when user has no active session.
                // Compute from balance: remaining = limit - balance (balance includes granted/removed time)
                let time_left = if time_left_raw > 0 {
                    time_left_raw
                } else if limit_day > 0 {
                    (limit_day - time_spent_balance).max(0)
                } else {
                    0
                };

                Ok(UserStatus {
                    user: self.user.clone(),
                    time_remaining_secs: time_left,
                    daily_limit_secs: limit_day,
                    time_spent_today_secs: time_spent_day,
                    time_spent_week_secs: time_spent_week,
                    time_spent_month_secs: time_spent_month,
                    time_inactive_secs: time_inactive,
                    active: time_left > 0,
                })
            }
        }
    }

    pub async fn get_config(&self) -> Result<UserConfig, Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let s = state.lock().await;
                let mut hours = HashMap::new();
                for d in &s.allowed_days {
                    hours.insert(
                        d.to_string(),
                        vec![HourInterval {
                            from: "0:00".into(),
                            to: "23:59".into(),
                            unaccountable: false,
                        }],
                    );
                }
                Ok(UserConfig {
                    user: self.user.clone(),
                    allowed_days: s.allowed_days.clone(),
                    daily_limits_secs: s.daily_limits.clone(),
                    allowed_hours: hours,
                    weekly_limit_secs: s.weekly_limit,
                    monthly_limit_secs: s.monthly_limit,
                    track_inactive: s.track_inactive,
                    hide_tray_icon: s.hide_tray_icon,
                    lockout_type: s.lockout_type.clone(),
                    wakeup_from: 0,
                    wakeup_to: 23,
                })
            }
            BridgeInner::DBus { conn } => {
                let data = Self::call_admin_info(conn, &self.user).await?;

                // ALLOWED_WEEKDAYS: array of i32 (day numbers 1-7)
                let allowed_days: Vec<u8> = Self::extract_i32_array(&data, "ALLOWED_WEEKDAYS")
                    .iter().map(|&d| d as u8).collect();

                // LIMITS_PER_WEEKDAYS: array of i32 (seconds per day)
                let daily_limits = Self::extract_i32_array(&data, "LIMITS_PER_WEEKDAYS");

                // ALLOWED_HOURS_N: array of strings like "0", "1", "!3", "4[15-45]"
                let mut allowed_hours = HashMap::new();
                for day in 1..=7u8 {
                    let key = format!("ALLOWED_HOURS_{day}");
                    let hour_strs = Self::extract_string_array(&data, &key);
                    if !hour_strs.is_empty() {
                        let hours_joined = hour_strs.join(";");
                        let intervals = parse_allowed_hours(&hours_joined);
                        allowed_hours.insert(day.to_string(), intervals);
                    }
                }

                let weekly_limit = Self::extract_i64(&data, "LIMIT_PER_WEEK");
                let monthly_limit = Self::extract_i64(&data, "LIMIT_PER_MONTH");
                let track_inactive = Self::extract_bool(&data, "TRACK_INACTIVE");
                let hide_tray_icon = Self::extract_bool(&data, "HIDE_TRAY_ICON");
                let lockout_type = Self::extract_string(&data, "LOCKOUT_TYPE");

                Ok(UserConfig {
                    user: self.user.clone(),
                    allowed_days,
                    daily_limits_secs: daily_limits,
                    allowed_hours,
                    weekly_limit_secs: weekly_limit,
                    monthly_limit_secs: monthly_limit,
                    track_inactive,
                    hide_tray_icon,
                    lockout_type,
                    wakeup_from: 0,
                    wakeup_to: 23,
                })
            }
        }
    }

    pub async fn grant_time(&self, seconds: i64) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.time_remaining += seconds;
                tracing::info!("mock: granted {}s, remaining: {}s", seconds, s.time_remaining);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setTimeLeft", &(&self.user, "+", seconds as i32)).await?;
                Ok(())
            }
        }
    }

    pub async fn subtract_time(&self, seconds: i64) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.time_remaining = (s.time_remaining - seconds).max(0);
                tracing::info!("mock: subtracted {}s, remaining: {}s", seconds, s.time_remaining);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setTimeLeft", &(&self.user, "-", seconds as i32)).await?;
                Ok(())
            }
        }
    }

    pub async fn set_time_left(&self, seconds: i64) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.time_remaining = seconds;
                tracing::info!("mock: set time to {}s", seconds);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setTimeLeft", &(&self.user, "=", seconds as i32)).await?;
                Ok(())
            }
        }
    }

    pub async fn set_daily_limits(&self, limits: &[i64]) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.daily_limits = limits.to_vec();
                tracing::info!("mock: daily limits set to {:?}", limits);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                let int_limits: Vec<i32> = limits.iter().map(|&l| l as i32).collect();
                Self::call_admin(conn, "setTimeLimitForDays", &(&self.user, &int_limits)).await?;
                Ok(())
            }
        }
    }

    pub async fn set_weekly_limit(&self, seconds: i64) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.weekly_limit = seconds;
                tracing::info!("mock: weekly limit set to {}s", seconds);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setTimeLimitForWeek", &(&self.user, seconds as i32))
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn set_monthly_limit(&self, seconds: i64) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.monthly_limit = seconds;
                tracing::info!("mock: monthly limit set to {}s", seconds);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setTimeLimitForMonth", &(&self.user, seconds as i32))
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn set_allowed_days(&self, days: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.allowed_days = days.to_vec();
                tracing::info!("mock: allowed days set to {:?}", days);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                let day_strs: Vec<String> = days.iter().map(|d| d.to_string()).collect();
                let day_refs: Vec<&str> = day_strs.iter().map(|s| s.as_str()).collect();
                Self::call_admin(conn, "setAllowedDays", &(&self.user, &day_refs)).await?;
                Ok(())
            }
        }
    }

    pub async fn set_allowed_hours(
        &self,
        day: &str,
        hours: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(_) => {
                tracing::info!("mock: allowed hours for day {} set to {:?}", day, hours);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                // Build the a{sa{si}} structure for setAllowedHours
                // Keys: STARTMIN (0-60), ENDMIN (0-60), UACC (0=accountable, 1=unaccountable)
                let mut hour_map: HashMap<String, HashMap<String, i32>> = HashMap::new();
                for &h in hours {
                    let mut inner = HashMap::new();
                    inner.insert("STARTMIN".to_string(), 0i32);
                    inner.insert("ENDMIN".to_string(), 60i32);
                    inner.insert("UACC".to_string(), 0i32);
                    hour_map.insert(h.to_string(), inner);
                }
                Self::call_admin(conn, "setAllowedHours", &(&self.user, day, &hour_map)).await?;
                Ok(())
            }
        }
    }

    pub async fn set_track_inactive(&self, track: bool) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.track_inactive = track;
                tracing::info!("mock: track_inactive = {}", track);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setTrackInactive", &(&self.user, track)).await?;
                Ok(())
            }
        }
    }

    pub async fn set_hide_tray_icon(&self, hide: bool) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.hide_tray_icon = hide;
                tracing::info!("mock: hide_tray_icon = {}", hide);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setHideTrayIcon", &(&self.user, hide)).await?;
                Ok(())
            }
        }
    }

    pub async fn set_lockout_type(
        &self,
        lockout_type: &str,
        wake_from: &str,
        wake_to: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.lockout_type = lockout_type.to_string();
                tracing::info!("mock: lockout_type = {}", lockout_type);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(
                    conn,
                    "setLockoutType",
                    &(&self.user, lockout_type, wake_from, wake_to),
                )
                .await?;
                Ok(())
            }
        }
    }

    pub async fn lock(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.locked = true;
                tracing::info!("mock: user locked");
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                Self::call_admin(conn, "setTimeLeft", &(&self.user, "=", 0i32)).await?;
                Ok(())
            }
        }
    }

    pub async fn unlock(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.inner {
            BridgeInner::Mock(state) => {
                let mut s = state.lock().await;
                s.locked = false;
                if s.time_remaining <= 0 {
                    s.time_remaining = s.daily_limits.first().copied().unwrap_or(7200);
                }
                tracing::info!("mock: user unlocked, remaining: {}s", s.time_remaining);
                Ok(())
            }
            BridgeInner::DBus { conn } => {
                // Restore daily limit
                let path = ObjectPath::try_from(PATH)?;
                let limits_reply: (i32, String) = conn
                    .call_method(
                        Some(DEST),
                        path,
                        Some(IFACE_LIMITS),
                        "requestTimeLimits",
                        &(&self.user,),
                    )
                    .await?
                    .body()
                    .deserialize()?;

                let daily_limit = if limits_reply.0 == 0 {
                    limits_reply.1.trim().parse().unwrap_or(7200i32)
                } else {
                    7200i32
                };

                Self::call_admin(conn, "setTimeLeft", &(&self.user, "=", daily_limit)).await?;
                Ok(())
            }
        }
    }
}

/// Parse timekpr hour strings like "0;1;2;!3;4[15-45];5;..." into HourInterval structs.
fn parse_allowed_hours(hours_str: &str) -> Vec<HourInterval> {
    let parts: Vec<&str> = hours_str.split(';').collect();
    let mut intervals = Vec::new();

    // Group consecutive hours into intervals
    let mut current_start: Option<(u8, bool)> = None;
    let mut current_end: u8 = 0;

    for part in &parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let unaccountable = part.starts_with('!');
        let clean = part.trim_start_matches('!');

        // Extract hour number (ignore minute ranges for now)
        let hour: u8 = clean
            .split('[')
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        match current_start {
            None => {
                current_start = Some((hour, unaccountable));
                current_end = hour;
            }
            Some((start, start_unacc)) => {
                if hour == current_end + 1 && unaccountable == start_unacc {
                    current_end = hour;
                } else {
                    // Emit interval
                    intervals.push(HourInterval {
                        from: format!("{}:00", start),
                        to: format!("{}:59", current_end),
                        unaccountable: start_unacc,
                    });
                    current_start = Some((hour, unaccountable));
                    current_end = hour;
                }
            }
        }
    }

    // Emit last interval
    if let Some((start, unacc)) = current_start {
        intervals.push(HourInterval {
            from: format!("{}:00", start),
            to: format!("{}:59", current_end),
            unaccountable: unacc,
        });
    }

    intervals
}

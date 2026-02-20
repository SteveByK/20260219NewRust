#![cfg(feature = "ssr")]

use std::{net::SocketAddr, sync::Arc};

use argon2::{
    password_hash::{PasswordHash, PasswordVerifier, SaltString},
    Argon2, PasswordHasher,
};
use async_nats::jetstream;
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::{get, post},
    Extension,
    Json, Router,
};
use axum::http::StatusCode;
use axum_prometheus::PrometheusMetricLayer;
use deadpool_redis::{Config as RedisPoolConfig, Pool as RedisPool, Runtime};
use futures_util::StreamExt;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use once_cell::sync::OnceCell;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

pub mod state {
    use super::*;

    pub static APP_STATE: OnceCell<Arc<AppState>> = OnceCell::new();

    #[derive(Clone)]
    pub struct AppState {
        pub pg: PgPool,
        pub redis: RedisPool,
        pub nats: async_nats::Client,
        pub jetstream: jetstream::Context,
        pub clickhouse: clickhouse::Client,
        pub r2: aws_sdk_s3::Client,
        pub jwt: JwtConfig,
        pub realtime_tx: broadcast::Sender<Vec<u8>>,
    }

    #[derive(Clone)]
    pub struct JwtConfig {
        pub algorithm: Algorithm,
        pub encoding: EncodingKey,
        pub decoding: DecodingKey,
    }
}

pub mod services {
    use super::*;

    pub mod auth {
        use super::*;

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct Claims {
            sub: String,
            exp: usize,
        }

        pub fn hash_password(raw: &str) -> anyhow::Result<String> {
            let salt = SaltString::generate(&mut rand::thread_rng());
            let hash = Argon2::default()
                .hash_password(raw.as_bytes(), &salt)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?
                .to_string();
            Ok(hash)
        }

        pub fn verify_password(raw: &str, hash: &str) -> bool {
            let Ok(parsed_hash) = PasswordHash::new(hash) else {
                return false;
            };
            Argon2::default()
                .verify_password(raw.as_bytes(), &parsed_hash)
                .is_ok()
        }

        pub fn make_jwt(user_id: Uuid, config: &state::JwtConfig) -> anyhow::Result<String> {
            let claims = Claims {
                sub: user_id.to_string(),
                exp: (chrono::Utc::now().timestamp() + 3600 * 24 * 7) as usize,
            };
            Ok(jsonwebtoken::encode(
                &Header::new(config.algorithm),
                &claims,
                &config.encoding,
            )?)
        }

        pub fn parse_jwt(token: &str, config: &state::JwtConfig) -> anyhow::Result<Uuid> {
            let data = jsonwebtoken::decode::<Claims>(
                token,
                &config.decoding,
                &Validation::new(config.algorithm),
            )?;
            Ok(Uuid::parse_str(&data.claims.sub)?)
        }
    }

    pub mod spatial {
        use super::*;

        #[derive(Debug, Serialize)]
        pub struct NearbyUser {
            pub user_id: String,
            pub distance_m: f64,
            pub lon: f64,
            pub lat: f64,
        }

        pub async fn nearby_users(pg: &PgPool, lon: f64, lat: f64, radius_m: i32) -> anyhow::Result<Vec<NearbyUser>> {
            let rows = sqlx::query(
                r#"
                SELECT
                    user_id::text,
                    ST_Distance(location, ST_Point($1, $2)::geography) AS distance,
                    ST_X(location::geometry) AS lon,
                    ST_Y(location::geometry) AS lat
                FROM user_locations
                WHERE ST_DWithin(location, ST_Point($1, $2)::geography, $3)
                ORDER BY distance
                LIMIT 50
                "#,
            )
            .bind(lon)
            .bind(lat)
            .bind(radius_m as f64)
            .fetch_all(pg)
            .await?;

            Ok(rows
                .into_iter()
                .map(|r| NearbyUser {
                    user_id: r.get::<String, _>("user_id"),
                    distance_m: r.get::<f64, _>("distance"),
                    lon: r.get::<f64, _>("lon"),
                    lat: r.get::<f64, _>("lat"),
                })
                .collect())
        }
    }

    pub mod realtime {
        use super::*;

        pub async fn publish_position(js: &jetstream::Context, payload: Vec<u8>) -> anyhow::Result<()> {
            js.publish("location.update", payload.into()).await?;
            Ok(())
        }

        pub async fn store_presence(redis: &RedisPool, user_key: &str, lon: f64, lat: f64) -> anyhow::Result<()> {
            let mut conn = redis.get().await?;
            let _: () = conn.set_ex(format!("presence:{user_key}"), "1", 30).await?;
            let _: usize = redis::cmd("GEOADD")
                .arg("geo:online")
                .arg(lon)
                .arg(lat)
                .arg(user_key)
                .query_async(&mut conn)
                .await?;
            Ok(())
        }

        pub async fn upsert_location(pg: &PgPool, user_id: Uuid, lon: f64, lat: f64) -> anyhow::Result<()> {
            sqlx::query(
                r#"
                INSERT INTO user_locations(user_id, location, updated_at)
                VALUES ($1, ST_SetSRID(ST_MakePoint($2, $3), 4326)::geography, now())
                ON CONFLICT (user_id)
                DO UPDATE SET location = EXCLUDED.location, updated_at = now()
                "#,
            )
            .bind(user_id)
            .bind(lon)
            .bind(lat)
            .execute(pg)
            .await?;
            Ok(())
        }

        pub async fn run_location_consumer(app: Arc<state::AppState>) -> anyhow::Result<()> {
            let mut sub = app.nats.subscribe("location.update").await?;
            while let Some(message) = sub.next().await {
                let packet: shared::RealtimePacket = match rmp_serde::from_slice(&message.payload) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                if let shared::RealtimePacket::Position(pos) = packet {
                    let _ = upsert_location(&app.pg, pos.user_id, pos.lon, pos.lat).await;
                }
            }
            Ok(())
        }

        pub async fn ingest_position(
            app: &state::AppState,
            user_id: Uuid,
            lon: f64,
            lat: f64,
        ) -> anyhow::Result<()> {
            let packet = shared::RealtimePacket::Position(shared::PositionUpdate {
                user_id,
                lon,
                lat,
                ts: chrono::Utc::now(),
            });
            let payload = rmp_serde::to_vec(&packet)?;

            store_presence(&app.redis, &user_id.to_string(), lon, lat).await?;
            publish_position(&app.jetstream, payload.clone()).await?;
            let _ = app.realtime_tx.send(payload);
            Ok(())
        }
    }

    pub mod chat {
        use super::*;

        pub async fn insert_message(pg: &PgPool, msg: &shared::ChatMessage) -> anyhow::Result<()> {
            sqlx::query(
                r#"
                INSERT INTO room_messages(room_id, from_user, message, created_at)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(&msg.room_id)
            .bind(msg.from_user)
            .bind(&msg.text)
            .bind(msg.ts)
            .execute(pg)
            .await?;
            Ok(())
        }

        pub(crate) async fn history(pg: &PgPool, room_id: &str, limit: i64) -> anyhow::Result<Vec<ChatHistoryItem>> {
            let rows = sqlx::query(
                r#"
                SELECT room_id, from_user::text AS from_user, message, created_at
                FROM room_messages
                WHERE room_id = $1
                ORDER BY created_at DESC
                LIMIT $2
                "#,
            )
            .bind(room_id)
            .bind(limit)
            .fetch_all(pg)
            .await?;

            let mut messages = rows
                .into_iter()
                .map(|row| ChatHistoryItem {
                    room_id: row.get::<String, _>("room_id"),
                    from_user: row.get::<String, _>("from_user"),
                    text: row.get::<String, _>("message"),
                    ts: row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                })
                .collect::<Vec<_>>();

            messages.reverse();
            Ok(messages)
        }

        pub async fn mark_read(pg: &PgPool, room_id: &str, user_id: Uuid) -> anyhow::Result<()> {
            sqlx::query(
                r#"
                INSERT INTO room_member_reads(room_id, user_id, last_read_at)
                VALUES ($1, $2, now())
                ON CONFLICT (room_id, user_id)
                DO UPDATE SET last_read_at = now()
                "#,
            )
            .bind(room_id)
            .bind(user_id)
            .execute(pg)
            .await?;
            Ok(())
        }

        pub async fn unread_count(pg: &PgPool, room_id: &str, user_id: Uuid) -> anyhow::Result<i64> {
            let row = sqlx::query(
                r#"
                WITH marker AS (
                  SELECT last_read_at
                  FROM room_member_reads
                  WHERE room_id = $1 AND user_id = $2
                )
                SELECT COUNT(*)::bigint AS unread_count
                FROM room_messages
                WHERE room_id = $1
                  AND from_user <> $2
                  AND created_at > COALESCE((SELECT last_read_at FROM marker), to_timestamp(0))
                "#,
            )
            .bind(room_id)
            .bind(user_id)
            .fetch_one(pg)
            .await?;

            Ok(row.get::<i64, _>("unread_count"))
        }

        pub async fn room_members(pg: &PgPool, room_id: &str) -> anyhow::Result<Vec<Uuid>> {
            let rows = sqlx::query(
                r#"
                SELECT DISTINCT from_user
                FROM room_messages
                WHERE room_id = $1
                ORDER BY from_user
                "#,
            )
            .bind(room_id)
            .fetch_all(pg)
            .await?;

            Ok(rows
                .into_iter()
                .map(|r| r.get::<Uuid, _>("from_user"))
                .collect())
        }
    }

    pub mod invite {
        use super::*;

        pub async fn create(pg: &PgPool, from_user: Uuid, to_user: Uuid, mode: &str) -> anyhow::Result<Uuid> {
            let invite_id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO invites(id, from_user, to_user, mode, status, created_at)
                VALUES ($1, $2, $3, $4, 'pending', now())
                "#,
            )
            .bind(invite_id)
            .bind(from_user)
            .bind(to_user)
            .bind(mode)
            .execute(pg)
            .await?;
            Ok(invite_id)
        }

        pub async fn respond(pg: &PgPool, invite_id: Uuid, to_user: Uuid, status: &str) -> anyhow::Result<Option<(Uuid, Uuid, String)>> {
            let row = sqlx::query(
                r#"
                UPDATE invites
                SET status = $1, responded_at = now()
                WHERE id = $2 AND to_user = $3 AND status = 'pending'
                RETURNING from_user, to_user, mode
                "#,
            )
            .bind(status)
            .bind(invite_id)
            .bind(to_user)
            .fetch_optional(pg)
            .await?;

            Ok(row.map(|r| {
                (
                    r.get::<Uuid, _>("from_user"),
                    r.get::<Uuid, _>("to_user"),
                    r.get::<String, _>("mode"),
                )
            }))
        }

        pub(crate) async fn pending_for_user(pg: &PgPool, to_user: Uuid) -> anyhow::Result<Vec<InviteItem>> {
            let rows = sqlx::query(
                r#"
                SELECT id::text AS invite_id, from_user::text AS from_user, to_user::text AS to_user, mode, status, created_at
                FROM invites
                WHERE to_user = $1 AND status = 'pending'
                ORDER BY created_at DESC
                LIMIT 100
                "#,
            )
            .bind(to_user)
            .fetch_all(pg)
            .await?;

            Ok(rows
                .into_iter()
                .map(|r| InviteItem {
                    invite_id: r.get::<String, _>("invite_id"),
                    from_user: r.get::<String, _>("from_user"),
                    to_user: r.get::<String, _>("to_user"),
                    mode: r.get::<String, _>("mode"),
                    status: r.get::<String, _>("status"),
                    ts: r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                })
                .collect())
        }
    }

    pub mod game {
        use super::*;

        pub async fn websocket_fallback_loop(
            mut ws: WebSocket,
            app: Arc<state::AppState>,
            auth_user: Uuid,
            mut rx: broadcast::Receiver<Vec<u8>>,
        ) {
            loop {
                tokio::select! {
                    incoming = ws.recv() => {
                        match incoming {
                            Some(Ok(Message::Binary(bin))) => {
                                let Ok(packet) = rmp_serde::from_slice::<shared::RealtimePacket>(&bin) else {
                                    continue;
                                };

                                if let shared::RealtimePacket::Position(mut pos) = packet {
                                    pos.user_id = auth_user;
                                    let _ = services::realtime::ingest_position(&app, auth_user, pos.lon, pos.lat).await;
                                } else if let shared::RealtimePacket::Chat(mut chat) = packet {
                                    chat.from_user = auth_user;
                                    if chat.room_id.trim().is_empty() {
                                        chat.room_id = "global".to_string();
                                    }
                                    let _ = services::chat::insert_message(&app.pg, &chat).await;
                                    if let Ok(payload) = rmp_serde::to_vec(&shared::RealtimePacket::Chat(chat)) {
                                        let _ = app.realtime_tx.send(payload);
                                    }
                                } else if let shared::RealtimePacket::Invite(mut invite) = packet {
                                    invite.from_user = auth_user;
                                    if let Ok(payload) = rmp_serde::to_vec(&shared::RealtimePacket::Invite(invite)) {
                                        let _ = app.realtime_tx.send(payload);
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => break,
                            _ => {}
                        }
                    }
                    outbound = rx.recv() => {
                        match outbound {
                            Ok(bin) => {
                                if ws.send(Message::Binary(bin.into())).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        }

        pub fn webtransport_placeholder() {
            let _ = "webtransport-enabled";
        }
    }
}

#[derive(Deserialize)]
struct RegisterBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct WsQuery {
    token: String,
}

#[derive(Deserialize)]
struct PositionBody {
    token: String,
    lon: f64,
    lat: f64,
}

#[derive(Deserialize)]
struct SendChatBody {
    token: String,
    room_id: String,
    text: String,
}

#[derive(Deserialize)]
struct ChatHistoryQuery {
    room_id: String,
}

#[derive(Deserialize)]
struct RoomStateQuery {
    token: String,
    room_id: String,
}

#[derive(Serialize)]
struct RoomMemberState {
    user_id: String,
    online: bool,
}

#[derive(Serialize)]
struct RoomStateResponse {
    room_id: String,
    unread_count: i64,
    members: Vec<RoomMemberState>,
}

#[derive(Deserialize)]
struct MarkReadBody {
    token: String,
    room_id: String,
}

#[derive(Serialize)]
pub(crate) struct ChatHistoryItem {
    room_id: String,
    from_user: String,
    text: String,
    ts: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct InviteBody {
    token: String,
    to_user: String,
    mode: String,
}

#[derive(Deserialize)]
struct InviteRespondBody {
    token: String,
    invite_id: String,
    action: String,
}

#[derive(Deserialize)]
struct InvitePendingQuery {
    token: String,
}

#[derive(Serialize)]
pub(crate) struct InviteItem {
    invite_id: String,
    from_user: String,
    to_user: String,
    mode: String,
    status: String,
    ts: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
struct RegisterResult {
    token: String,
    user_id: String,
    username: String,
}

struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn health(&self, _ctx: &Context<'_>) -> &str {
        "ok"
    }
}

type AppSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

async fn register(State(app): State<Arc<state::AppState>>, Json(body): Json<RegisterBody>) -> impl IntoResponse {
    let hashed = services::auth::hash_password(&body.password).unwrap_or_default();
    let username = body.username;
    let row = sqlx::query(
        "INSERT INTO users(username, password_hash) VALUES($1, $2) ON CONFLICT (username) DO UPDATE SET password_hash = EXCLUDED.password_hash RETURNING id::text, username"
    )
    .bind(&username)
    .bind(hashed)
    .fetch_one(&app.pg)
    .await;

    let Ok(row) = row else {
        return Json(RegisterResult {
            token: String::new(),
            user_id: String::new(),
            username,
        });
    };

    let user_id_str = row.get::<String, _>("id");
    let user_id = Uuid::parse_str(&user_id_str).unwrap_or_else(|_| Uuid::nil());
    let token = services::auth::make_jwt(user_id, &app.jwt).unwrap_or_default();

    Json(RegisterResult {
        token,
        user_id: user_id.to_string(),
        username: row.get::<String, _>("username"),
    })
}

async fn login(State(app): State<Arc<state::AppState>>, Json(body): Json<LoginBody>) -> impl IntoResponse {
    let row = sqlx::query("SELECT id::text, username, password_hash FROM users WHERE username = $1")
        .bind(&body.username)
        .fetch_optional(&app.pg)
        .await;

    let Ok(Some(row)) = row else {
        return Json(RegisterResult {
            token: String::new(),
            user_id: String::new(),
            username: body.username,
        });
    };

    let hash = row.get::<String, _>("password_hash");
    let valid = services::auth::verify_password(&body.password, &hash);
    if !valid {
        return Json(RegisterResult {
            token: String::new(),
            user_id: String::new(),
            username: row.get::<String, _>("username"),
        });
    }

    let user_id = Uuid::parse_str(&row.get::<String, _>("id")).unwrap_or_else(|_| Uuid::nil());
    let token = services::auth::make_jwt(user_id, &app.jwt).unwrap_or_default();

    Json(RegisterResult {
        token,
        user_id: user_id.to_string(),
        username: row.get::<String, _>("username"),
    })
}

async fn ingest_position_http(
    State(app): State<Arc<state::AppState>>,
    Json(body): Json<PositionBody>,
) -> impl IntoResponse {
    let Ok(user_id) = services::auth::parse_jwt(&body.token, &app.jwt) else {
        return StatusCode::UNAUTHORIZED;
    };

    match services::realtime::ingest_position(&app, user_id, body.lon, body.lat).await {
        Ok(_) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn send_chat(
    State(app): State<Arc<state::AppState>>,
    Json(body): Json<SendChatBody>,
) -> impl IntoResponse {
    let Ok(user_id) = services::auth::parse_jwt(&body.token, &app.jwt) else {
        return StatusCode::UNAUTHORIZED;
    };

    let text = body.text.trim().to_string();
    if text.is_empty() {
        return StatusCode::BAD_REQUEST;
    }

    let room_id = if body.room_id.trim().is_empty() {
        "global".to_string()
    } else {
        body.room_id
    };

    let message = shared::ChatMessage {
        room_id,
        from_user: user_id,
        text,
        ts: chrono::Utc::now(),
    };

    if services::chat::insert_message(&app.pg, &message).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    if let Ok(payload) = rmp_serde::to_vec(&shared::RealtimePacket::Chat(message)) {
        let _ = app.realtime_tx.send(payload);
    }

    StatusCode::ACCEPTED
}

async fn chat_history(
    State(app): State<Arc<state::AppState>>,
    Query(query): Query<ChatHistoryQuery>,
) -> impl IntoResponse {
    let room_id = if query.room_id.trim().is_empty() {
        "global".to_string()
    } else {
        query.room_id
    };

    match services::chat::history(&app.pg, &room_id, 100).await {
        Ok(rows) => Json(rows).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn chat_room_state(
    State(app): State<Arc<state::AppState>>,
    Query(query): Query<RoomStateQuery>,
) -> impl IntoResponse {
    let Ok(user_id) = services::auth::parse_jwt(&query.token, &app.jwt) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let room_id = if query.room_id.trim().is_empty() {
        "global".to_string()
    } else {
        query.room_id
    };

    let unread_count = match services::chat::unread_count(&app.pg, &room_id, user_id).await {
        Ok(value) => value,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let member_ids = match services::chat::room_members(&app.pg, &room_id).await {
        Ok(ids) => ids,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let mut members = Vec::with_capacity(member_ids.len());
    if let Ok(mut conn) = app.redis.get().await {
        for id in member_ids {
            let key = format!("presence:{id}");
            let online = conn.exists::<_, bool>(key).await.unwrap_or(false);
            members.push(RoomMemberState {
                user_id: id.to_string(),
                online,
            });
        }
    }

    Json(RoomStateResponse {
        room_id,
        unread_count,
        members,
    })
    .into_response()
}

async fn chat_mark_read(
    State(app): State<Arc<state::AppState>>,
    Json(body): Json<MarkReadBody>,
) -> impl IntoResponse {
    let Ok(user_id) = services::auth::parse_jwt(&body.token, &app.jwt) else {
        return StatusCode::UNAUTHORIZED;
    };

    let room_id = if body.room_id.trim().is_empty() {
        "global".to_string()
    } else {
        body.room_id
    };

    match services::chat::mark_read(&app.pg, &room_id, user_id).await {
        Ok(_) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn send_invite(
    State(app): State<Arc<state::AppState>>,
    Json(body): Json<InviteBody>,
) -> impl IntoResponse {
    let Ok(from_user) = services::auth::parse_jwt(&body.token, &app.jwt) else {
        return StatusCode::UNAUTHORIZED;
    };
    let Ok(to_user) = Uuid::parse_str(&body.to_user) else {
        return StatusCode::BAD_REQUEST;
    };

    let mode = if body.mode.trim().is_empty() {
        "duel".to_string()
    } else {
        body.mode
    };

    let Ok(invite_id) = services::invite::create(&app.pg, from_user, to_user, &mode).await else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };

    let packet = shared::RealtimePacket::Invite(shared::InviteEvent {
        invite_id,
        from_user,
        to_user,
        mode,
        status: "pending".to_string(),
        ts: chrono::Utc::now(),
    });

    if let Ok(payload) = rmp_serde::to_vec(&packet) {
        let _ = app.realtime_tx.send(payload);
    }

    StatusCode::ACCEPTED
}

async fn invite_pending(
    State(app): State<Arc<state::AppState>>,
    Query(query): Query<InvitePendingQuery>,
) -> impl IntoResponse {
    let Ok(user_id) = services::auth::parse_jwt(&query.token, &app.jwt) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    match services::invite::pending_for_user(&app.pg, user_id).await {
        Ok(rows) => Json(rows).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn invite_respond(
    State(app): State<Arc<state::AppState>>,
    Json(body): Json<InviteRespondBody>,
) -> impl IntoResponse {
    let Ok(to_user) = services::auth::parse_jwt(&body.token, &app.jwt) else {
        return StatusCode::UNAUTHORIZED;
    };
    let Ok(invite_id) = Uuid::parse_str(&body.invite_id) else {
        return StatusCode::BAD_REQUEST;
    };

    let status = match body.action.as_str() {
        "accept" => "accepted",
        "reject" => "rejected",
        _ => return StatusCode::BAD_REQUEST,
    };

    let Ok(updated) = services::invite::respond(&app.pg, invite_id, to_user, status).await else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    let Some((from_user, to_user, mode)) = updated else {
        return StatusCode::NOT_FOUND;
    };

    let packet = shared::RealtimePacket::Invite(shared::InviteEvent {
        invite_id,
        from_user,
        to_user,
        mode,
        status: status.to_string(),
        ts: chrono::Utc::now(),
    });
    if let Ok(payload) = rmp_serde::to_vec(&packet) {
        let _ = app.realtime_tx.send(payload);
    }

    StatusCode::ACCEPTED
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(app): State<Arc<state::AppState>>,
) -> impl IntoResponse {
    let Ok(user_id) = services::auth::parse_jwt(&query.token, &app.jwt) else {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    };

    ws.on_upgrade(move |socket| async move {
        let rx = app.realtime_tx.subscribe();
        services::game::websocket_fallback_loop(socket, app, user_id, rx).await;
    })
}

async fn graphql_handler(
    Extension(schema): Extension<AppSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@127.0.0.1:5432/platform".into());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into());
    let clickhouse_url = std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".into());

    let pg = PgPool::connect(&database_url).await?;
    let redis = RedisPoolConfig::from_url(redis_url).create_pool(Some(Runtime::Tokio1))?;
    let nats = async_nats::connect(nats_url).await?;
    let jetstream = jetstream::new(nats.clone());
    let _ = jetstream
        .create_stream(jetstream::stream::Config {
            name: "location_events".to_string(),
            subjects: vec!["location.update".to_string()],
            ..Default::default()
        })
        .await;

    let clickhouse = clickhouse::Client::default().with_url(clickhouse_url).with_database("default");

    let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let r2 = aws_sdk_s3::Client::new(&aws_config);

    let private_key_pem = std::env::var("JWT_PRIVATE_KEY_PEM").unwrap_or_default();
    let public_key_pem = std::env::var("JWT_PUBLIC_KEY_PEM").unwrap_or_default();
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".to_string());

    let jwt = if !private_key_pem.is_empty() && !public_key_pem.is_empty() {
        state::JwtConfig {
            algorithm: Algorithm::RS256,
            encoding: EncodingKey::from_rsa_pem(private_key_pem.as_bytes())?,
            decoding: DecodingKey::from_rsa_pem(public_key_pem.as_bytes())?,
        }
    } else {
        state::JwtConfig {
            algorithm: Algorithm::HS256,
            encoding: EncodingKey::from_secret(jwt_secret.as_bytes()),
            decoding: DecodingKey::from_secret(jwt_secret.as_bytes()),
        }
    };

    let (realtime_tx, _) = broadcast::channel(4096);
    let app_state = Arc::new(state::AppState {
        pg,
        redis,
        nats,
        jetstream,
        clickhouse,
        r2,
        jwt,
        realtime_tx,
    });

    let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription).finish();

    let consumer_state = app_state.clone();
    tokio::spawn(async move {
        if let Err(err) = services::realtime::run_location_consumer(consumer_state).await {
            tracing::error!(?err, "location consumer exited");
        }
    });

    let _ = state::APP_STATE.set(app_state.clone());

    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/position", post(ingest_position_http))
        .route("/api/chat/send", post(send_chat))
        .route("/api/chat/history", get(chat_history))
        .route("/api/chat/room-state", get(chat_room_state))
        .route("/api/chat/mark-read", post(chat_mark_read))
        .route("/api/invite/send", post(send_invite))
        .route("/api/invite/pending", get(invite_pending))
        .route("/api/invite/respond", post(invite_respond))
        .route("/graphql", post(graphql_handler))
        .route("/ws", get(ws_handler))
        .route("/metrics", get(|| async move { metric_handle.render() }))
        .layer(prometheus_layer)
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(Extension(schema))
        .with_state(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!(%addr, "platform server started");

    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

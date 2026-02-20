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

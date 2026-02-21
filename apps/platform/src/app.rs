use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::components::*;
use leptos_router::StaticSegment;
use serde::{Deserialize, Serialize};

#[cfg(feature = "hydrate")]
use wasm_bindgen::{closure::Closure, JsCast};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyUserDto {
    pub user_id: String,
    pub distance_m: f64,
    pub lon: f64,
    pub lat: f64,
}

#[cfg(feature = "hydrate")]
#[derive(Debug, Clone, Serialize)]
struct AuthBody {
    username: String,
    password: String,
}

#[cfg(feature = "hydrate")]
#[derive(Debug, Clone, Deserialize)]
struct AuthResult {
    token: String,
    user_id: String,
    username: String,
}

#[cfg(feature = "hydrate")]
#[derive(Debug, Clone, Deserialize)]
struct ApiErrorBody {
    error: String,
}

#[cfg(feature = "hydrate")]
async fn request_auth(endpoint: &str, payload: &AuthBody, action: &str) -> Result<AuthResult, (u16, String)> {
    let body = serde_json::to_string(payload)
        .map_err(|_| (500, format!("{}请求序列化失败", action)))?;

    let req = gloo_net::http::Request::post(endpoint)
        .header("content-type", "application/json")
        .body(body)
        .map_err(|_| (500, format!("{}请求构建失败", action)))?;

    let resp = req
        .send()
        .await
        .map_err(|_| (500, format!("{}请求失败", action)))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if (200..300).contains(&status) {
        let parsed = serde_json::from_str::<AuthResult>(&text)
            .map_err(|_| (500, format!("{}返回解析失败", action)))?;
        return Ok(parsed);
    }

    let msg = serde_json::from_str::<ApiErrorBody>(&text)
        .map(|v| v.error)
        .unwrap_or_else(|_| format!("{}失败（HTTP {}）", action, status));

    Err((status, msg))
}

#[cfg(feature = "hydrate")]
#[derive(Debug, Clone, Deserialize)]
struct ChatHistoryItem {
    room_id: String,
    from_user: String,
    text: String,
    ts: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Deserialize)]
struct RoomMemberState {
    user_id: String,
    online: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RoomStateResponse {
    room_id: String,
    unread_count: i64,
    members: Vec<RoomMemberState>,
}

#[derive(Debug, Clone, Deserialize)]
struct InviteItem {
    invite_id: String,
    from_user: String,
    to_user: String,
    mode: String,
    status: String,
    ts: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct Session {
    token: String,
    user_id: String,
    username: String,
}

#[cfg(feature = "hydrate")]
#[derive(Debug, Clone, Deserialize)]
struct PublicMapConfig {
    style_url: String,
    center_lon: f64,
    center_lat: f64,
    zoom: f64,
}

const CHAT_HISTORY_PAGE_SIZE: i64 = 20;

#[cfg(feature = "hydrate")]
async fn load_pending_invites(token: &str) -> Result<Vec<InviteItem>, String> {
    let pending_url = format!("/api/invite/pending?token={}", urlencoding::encode(token));
    let resp = gloo_net::http::Request::get(&pending_url)
        .send()
        .await
        .map_err(|_| "加载待处理邀请失败".to_string())?;

    resp.json::<Vec<InviteItem>>()
        .await
        .map_err(|_| "解析待处理邀请失败".to_string())
}

#[cfg(feature = "hydrate")]
async fn load_history_page(room_id: &str, page: i64) -> Result<Vec<String>, String> {
    let page = page.max(1);
    let limit = (page * CHAT_HISTORY_PAGE_SIZE).clamp(CHAT_HISTORY_PAGE_SIZE, 500);
    let url = format!(
        "/api/chat/history?room_id={}&limit={}",
        urlencoding::encode(room_id),
        limit
    );

    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|_| "历史消息加载失败".to_string())?;

    let rows = resp
        .json::<Vec<ChatHistoryItem>>()
        .await
        .map_err(|_| "历史消息解析失败".to_string())?;

    Ok(rows
        .into_iter()
        .map(|r| {
            format!(
                "[{}][{}] {}: {}",
                r.room_id,
                r.ts.format("%H:%M:%S"),
                r.from_user.chars().take(8).collect::<String>(),
                r.text
            )
        })
        .collect())
}

#[cfg(feature = "hydrate")]
fn ws_url(token: &str) -> Option<String> {
    let window = web_sys::window()?;
    let location = window.location();
    let host = location.host().ok()?;
    let protocol = location.protocol().ok()?;
    let ws_proto = if protocol == "https:" { "wss" } else { "ws" };
    Some(format!("{ws_proto}://{host}/ws?token={token}"))
}

#[cfg(feature = "hydrate")]
fn build_geojson(users: &[NearbyUserDto], me: Option<&str>) -> String {
    let features = users
        .iter()
        .map(|u| {
            serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [u.lon, u.lat]
                },
                "properties": {
                    "user_id": u.user_id,
                    "label": if me.is_some_and(|v| v == u.user_id) {
                        format!("我 ({:.0}m)", u.distance_m)
                    } else {
                        format!("{} ({:.0}m)", &u.user_id[..u.user_id.len().min(8)], u.distance_m)
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "type": "FeatureCollection",
        "features": features
    })
    .to_string()
}

#[cfg(feature = "hydrate")]
fn connect_realtime(
    token: String,
    user_id: String,
    my_position: RwSignal<(f64, f64)>,
    ws_connected: RwSignal<bool>,
    refresh_tick: RwSignal<u64>,
    status: RwSignal<String>,
    chat_messages: RwSignal<Vec<String>>,
    invite_events: RwSignal<Vec<String>>,
    pending_invites: RwSignal<Vec<InviteItem>>,
) {
    let Some(url) = ws_url(&token) else {
        status.set("WebSocket 地址生成失败".to_string());
        return;
    };

    let Ok(ws) = web_sys::WebSocket::new(&url) else {
        status.set("WebSocket 初始化失败".to_string());
        return;
    };

    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    let on_open_connected = ws_connected;
    let on_open_status = status;
    let on_open = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        on_open_connected.set(true);
        on_open_status.set("实时通道已连接".to_string());
    }) as Box<dyn FnMut(_)>);
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let on_close_connected = ws_connected;
    let on_close_status = status;
    let on_close = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        on_close_connected.set(false);
        on_close_status.set("实时通道已断开".to_string());
    }) as Box<dyn FnMut(_)>);
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    let on_msg_tick = refresh_tick;
    let on_msg_chat = chat_messages;
    let on_msg_invite_events = invite_events;
    let on_msg_pending_invites = pending_invites;
    let my_uid = user_id.clone();

    let on_message = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        if let Ok(buf) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
            let bytes = js_sys::Uint8Array::new(&buf).to_vec();
            if let Ok(packet) = rmp_serde::from_slice::<shared::RealtimePacket>(&bytes) {
                match packet {
                    shared::RealtimePacket::Chat(chat) => {
                        on_msg_chat.update(|list| {
                            list.push(format!(
                                "[{}] {}: {}",
                                chat.room_id,
                                chat.from_user.to_string().chars().take(8).collect::<String>(),
                                chat.text
                            ));
                            if list.len() > 200 {
                                let keep_from = list.len().saturating_sub(200);
                                *list = list[keep_from..].to_vec();
                            }
                        });
                    }
                    shared::RealtimePacket::Invite(inv) => {
                        let from_id = inv.from_user.to_string();
                        let to_id = inv.to_user.to_string();
                        let summary = format!(
                            "邀请[{}] {} -> {} [{}|{}]",
                            inv.invite_id,
                            from_id.chars().take(8).collect::<String>(),
                            to_id.chars().take(8).collect::<String>(),
                            inv.mode,
                            inv.status
                        );

                        on_msg_invite_events.update(|list| {
                            list.push(summary);
                            if list.len() > 120 {
                                let keep_from = list.len().saturating_sub(120);
                                *list = list[keep_from..].to_vec();
                            }
                        });

                        on_msg_pending_invites.update(|list| {
                            if inv.status == "pending" && to_id == my_uid {
                                let incoming = InviteItem {
                                    invite_id: inv.invite_id.to_string(),
                                    from_user: from_id,
                                    to_user: to_id,
                                    mode: inv.mode,
                                    status: inv.status,
                                    ts: inv.ts,
                                };
                                if !list.iter().any(|it| it.invite_id == incoming.invite_id) {
                                    list.push(incoming);
                                }
                            } else {
                                list.retain(|it| it.invite_id != inv.invite_id.to_string());
                            }
                        });
                    }
                    _ => {}
                }
            }
        }

        on_msg_tick.update(|v| *v += 1);
    }) as Box<dyn FnMut(_)>);
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    let Ok(parsed_user_id) = uuid::Uuid::parse_str(&user_id) else {
        status.set("用户ID解析失败".to_string());
        return;
    };

    let ws_for_tick = ws.clone();
    let tick_pos = my_position;
    let tick_refresh = refresh_tick;
    let tick = Closure::wrap(Box::new(move || {
        let (base_lon, base_lat) = tick_pos.get();
        let lon = base_lon + (js_sys::Math::random() - 0.5) * 0.0015;
        let lat = base_lat + (js_sys::Math::random() - 0.5) * 0.0015;
        tick_pos.set((lon, lat));

        let packet = shared::RealtimePacket::Position(shared::PositionUpdate {
            user_id: parsed_user_id,
            lon,
            lat,
            ts: chrono::Utc::now(),
        });

        if let Ok(bin) = rmp_serde::to_vec(&packet) {
            let _ = ws_for_tick.send_with_u8_array(&bin);
            tick_refresh.update(|v| *v += 1);
        }
    }) as Box<dyn FnMut()>);

    if let Some(window) = web_sys::window() {
        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
            tick.as_ref().unchecked_ref(),
            2500,
        );
    }
    tick.forget();
}

#[server(name = QueryNearby, prefix = "/api")]
pub async fn query_nearby(lon: f64, lat: f64, radius_m: i32) -> Result<Vec<NearbyUserDto>, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let app_state = crate::server::state::APP_STATE
            .get()
            .ok_or_else(|| ServerFnError::new("server state not initialized"))?;
        let users = crate::server::services::spatial::nearby_users(&app_state.pg, lon, lat, radius_m)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        return Ok(users
            .into_iter()
            .map(|u| NearbyUserDto {
                user_id: u.user_id,
                distance_m: u.distance_m,
                lon: u.lon,
                lat: u.lat,
            })
            .collect());
    }

    #[cfg(not(feature = "ssr"))]
    {
        let _ = (lon, lat, radius_m);
        Ok(vec![])
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());

    let room_id = RwSignal::new("global".to_string());
    let chat_input = RwSignal::new(String::new());

    let session = RwSignal::new(None::<Session>);
    let my_position = RwSignal::new((116.397, 39.908));
    let refresh_tick = RwSignal::new(0_u64);
    let ws_connected = RwSignal::new(false);
    let status = RwSignal::new("请先登录以开启实时联调".to_string());
    let selected_user = RwSignal::new(String::new());

    let chat_messages = RwSignal::new(Vec::<String>::new());
    let invite_events = RwSignal::new(Vec::<String>::new());
    let pending_invites = RwSignal::new(Vec::<InviteItem>::new());
    let history_page = RwSignal::new(1_i64);
    #[cfg(feature = "hydrate")]
    let invite_poll_started = RwSignal::new(false);

    #[cfg(feature = "hydrate")]
    Effect::new(|_| {
        leptos::task::spawn_local(async move {
            let config = match gloo_net::http::Request::get("/api/public-map-config").send().await {
                Ok(resp) => resp.json::<PublicMapConfig>().await.ok(),
                Err(_) => None,
            }
            .unwrap_or(PublicMapConfig {
                style_url: "https://demotiles.maplibre.org/style.json".to_string(),
                center_lon: 116.397,
                center_lat: 39.908,
                zoom: 11.0,
            });

            crate::map::mount_map(
                &config.style_url,
                config.center_lon,
                config.center_lat,
                config.zoom,
            );
            crate::map::set_center(config.center_lon, config.center_lat);
        });
    });

    let nearby = LocalResource::new(move || {
        let tick = refresh_tick.get();
        let active = session.get().is_some();

        async move {
            if !active {
                return Some(Vec::<NearbyUserDto>::new());
            }
            let _ = tick;
            let (lon, lat) = my_position.get();
            query_nearby(lon, lat, 5_000).await.ok()
        }
    });

    let room_state: LocalResource<Option<RoomStateResponse>> = LocalResource::new(move || {
        let tick = refresh_tick.get();
        let room = room_id.get();
        let session_value = session.get();

        async move {
            #[cfg(feature = "hydrate")]
            {
                if let Some(s) = session_value {
                    let url = format!(
                        "/api/chat/room-state?token={}&room_id={}",
                        urlencoding::encode(&s.token),
                        urlencoding::encode(&room)
                    );
                    let response = match gloo_net::http::Request::get(&url).send().await {
                        Ok(resp) => resp,
                        Err(_) => return None,
                    };
                    return response.json::<RoomStateResponse>().await.ok();
                }
                let _ = tick;
                None
            }

            #[cfg(not(feature = "hydrate"))]
            {
                let _ = (tick, room, session_value);
                None
            }
        }
    });

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(items) = nearby.get().and_then(|wrapped| wrapped.take()) {
            let my_user_id = session.get().map(|s| s.user_id).unwrap_or_default();
            let geojson = build_geojson(&items, Some(&my_user_id));
            crate::map::update_online_users_geojson(&geojson);

            if let Some(first) = items.first() {
                crate::map::set_center(first.lon, first.lat);
            }
        }
    });

    let on_login = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let username_value = username.get();
            let password_value = password.get();
            if username_value.trim().is_empty() || password_value.trim().is_empty() {
                status.set("请输入用户名和密码".to_string());
                return;
            }

            let status_setter = status;
            let session_setter = session;
            let pos = my_position;
            let ws_state = ws_connected;
            let tick = refresh_tick;
            let chat_state = chat_messages;
            let invite_state = invite_events;
            let pending_state = pending_invites;
            let poll_started = invite_poll_started;

            leptos::task::spawn_local(async move {
                status_setter.set("登录中...".to_string());
                let payload = AuthBody {
                    username: username_value,
                    password: password_value,
                };
                let final_auth = match request_auth("/api/login", &payload, "登录").await {
                    Ok(auth) => auth,
                    Err((404, _)) => {
                        status_setter.set("用户不存在，自动注册中...".to_string());
                        match request_auth("/api/register", &payload, "注册").await {
                            Ok(auth) => auth,
                            Err((_, msg)) => {
                                status_setter.set(format!("注册失败：{}", msg));
                                return;
                            }
                        }
                    }
                    Err((_, msg)) => {
                        status_setter.set(format!("登录失败：{}", msg));
                        return;
                    }
                };

                if final_auth.token.is_empty() {
                    status_setter.set("登录失败：凭据无效".to_string());
                    return;
                }

                let token = final_auth.token.clone();
                let user_id = final_auth.user_id.clone();
                let username = final_auth.username.clone();

                session_setter.set(Some(Session {
                    token: token.clone(),
                    user_id: user_id.clone(),
                    username: username.clone(),
                }));

                if let Ok(rows) = load_pending_invites(&token).await {
                    pending_state.set(rows);
                }

                if !poll_started.get_untracked() {
                    poll_started.set(true);
                    let token_for_poll = token.clone();
                    let pending_for_poll = pending_state;
                    let status_for_poll = status_setter;
                    let poll = Closure::wrap(Box::new(move || {
                        let token_value = token_for_poll.clone();
                        let pending_value = pending_for_poll;
                        let status_value = status_for_poll;
                        leptos::task::spawn_local(async move {
                            match load_pending_invites(&token_value).await {
                                Ok(rows) => pending_value.set(rows),
                                Err(err) => status_value.set(err),
                            }
                        });
                    }) as Box<dyn FnMut()>);

                    if let Some(window) = web_sys::window() {
                        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                            poll.as_ref().unchecked_ref(),
                            12_000,
                        );
                    }
                    poll.forget();
                }

                status_setter.set(format!("已登录：{}", username));
                tick.update(|v| *v += 1);

                connect_realtime(
                    token,
                    user_id,
                    pos,
                    ws_state,
                    tick,
                    status_setter,
                    chat_state,
                    invite_state,
                    pending_state,
                );
            });
        }
    };

    let on_send_chat = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let Some(s) = session.get() else {
                status.set("请先登录".to_string());
                return;
            };

            let text = chat_input.get();
            if text.trim().is_empty() {
                return;
            }

            let payload = serde_json::json!({
                "token": s.token,
                "room_id": room_id.get(),
                "text": text,
            });

            let status_setter = status;
            let chat_input_setter = chat_input;

            leptos::task::spawn_local(async move {
                let req = gloo_net::http::Request::post("/api/chat/send")
                    .header("content-type", "application/json")
                    .body(payload.to_string());

                match req {
                    Ok(r) => {
                        if r.send().await.is_ok() {
                            chat_input_setter.set(String::new());
                        } else {
                            status_setter.set("消息发送失败".to_string());
                        }
                    }
                    Err(_) => status_setter.set("消息请求构建失败".to_string()),
                }
            });
        }
    };

    let on_load_history = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let room = room_id.get();
            let chat_state = chat_messages;
            let status_setter = status;
            let page = history_page.get();
            leptos::task::spawn_local(async move {
                match load_history_page(&room, page).await {
                    Ok(rows) => chat_state.set(rows),
                    Err(err) => status_setter.set(err),
                }
            });
        }
    };

    let on_load_older_history = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let room = room_id.get();
            let chat_state = chat_messages;
            let status_setter = status;
            let page_signal = history_page;
            page_signal.update(|v| *v += 1);
            let page = page_signal.get();

            leptos::task::spawn_local(async move {
                match load_history_page(&room, page).await {
                    Ok(rows) => chat_state.set(rows),
                    Err(err) => status_setter.set(err),
                }
            });
        }
    };

    let on_load_newer_history = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let room = room_id.get();
            let chat_state = chat_messages;
            let status_setter = status;
            let page_signal = history_page;
            page_signal.update(|v| {
                if *v > 1 {
                    *v -= 1;
                }
            });
            let page = page_signal.get();

            leptos::task::spawn_local(async move {
                match load_history_page(&room, page).await {
                    Ok(rows) => chat_state.set(rows),
                    Err(err) => status_setter.set(err),
                }
            });
        }
    };

    let on_mark_read = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let Some(s) = session.get() else {
                status.set("请先登录".to_string());
                return;
            };

            let payload = serde_json::json!({
                "token": s.token,
                "room_id": room_id.get(),
            });

            let status_setter = status;
            let tick = refresh_tick;

            leptos::task::spawn_local(async move {
                let req = gloo_net::http::Request::post("/api/chat/mark-read")
                    .header("content-type", "application/json")
                    .body(payload.to_string());
                match req {
                    Ok(r) => {
                        if r.send().await.is_ok() {
                            status_setter.set("已标记为已读".to_string());
                            tick.update(|v| *v += 1);
                        } else {
                            status_setter.set("标记已读失败".to_string());
                        }
                    }
                    Err(_) => status_setter.set("已读请求构建失败".to_string()),
                }
            });
        }
    };

    let on_send_invite = move |_| {
        #[cfg(feature = "hydrate")]
        {
            let Some(s) = session.get() else {
                status.set("请先登录".to_string());
                return;
            };

            let to_user = selected_user.get();
            if to_user.trim().is_empty() {
                status.set("请先选择在线用户".to_string());
                return;
            }

            let payload = serde_json::json!({
                "token": s.token,
                "to_user": to_user,
                "mode": "duel",
            });

            let status_setter = status;
            leptos::task::spawn_local(async move {
                let req = gloo_net::http::Request::post("/api/invite/send")
                    .header("content-type", "application/json")
                    .body(payload.to_string());

                match req {
                    Ok(r) => {
                        if r.send().await.is_ok() {
                            status_setter.set("邀请已发送".to_string());
                        } else {
                            status_setter.set("邀请发送失败".to_string());
                        }
                    }
                    Err(_) => status_setter.set("邀请请求构建失败".to_string()),
                }
            });
        }
    };

    let on_respond_invite = move |_invite_id: String, _action: &'static str| {
        #[cfg(feature = "hydrate")]
        {
            let Some(s) = session.get() else {
                status.set("请先登录".to_string());
                return;
            };

            let payload = serde_json::json!({
                "token": s.token,
                "invite_id": _invite_id,
                "action": _action,
            });
            let invite_id_key = payload["invite_id"].as_str().unwrap_or_default().to_string();

            let status_setter = status;
            let pending_state = pending_invites;

            leptos::task::spawn_local(async move {
                let req = gloo_net::http::Request::post("/api/invite/respond")
                    .header("content-type", "application/json")
                    .body(payload.to_string());

                match req {
                    Ok(r) => {
                        if r.send().await.is_ok() {
                            pending_state.update(|list| {
                                list.retain(|it| it.invite_id != invite_id_key);
                            });
                            status_setter.set(if _action == "accept" {
                                "邀请已接受".to_string()
                            } else {
                                "邀请已拒绝".to_string()
                            });
                        } else {
                            status_setter.set("邀请响应失败".to_string());
                        }
                    }
                    Err(_) => status_setter.set("邀请响应请求构建失败".to_string()),
                }
            });
        }
    };

    let online_count = move || {
        nearby
            .get()
            .and_then(|wrapped| wrapped.take())
            .unwrap_or_default()
            .len()
    };

    let unread_count = move || {
        #[cfg(feature = "hydrate")]
        {
            match room_state.get() {
                Some(wrapped) => wrapped.take().map(|s| s.unread_count).unwrap_or(0),
                None => 0,
            }
        }

        #[cfg(not(feature = "hydrate"))]
        {
            let _ = &room_state;
            0
        }
    };

    view! {
        <div class="h-screen w-screen grid grid-rows-[auto_1fr] bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center justify-between bg-slate-900/80 backdrop-blur">
                <div>
                    <h1 class="text-lg font-semibold">"社交地图 + 实时通讯 + 高性能网页游戏"</h1>
                    <p class="text-xs text-slate-400">"Leptos + MapLibre + WebSocket + Redis + PostGIS + Chat + Invite Workflow"</p>
                </div>
                <div class="text-sm flex items-center gap-4">
                    <span class="px-2 py-1 rounded border border-slate-700">{move || format!("在线: {}", online_count())}</span>
                    <span class="px-2 py-1 rounded border border-slate-700">{move || format!("未读: {}", unread_count())}</span>
                    <span class=move || {
                        if ws_connected.get() {
                            "px-2 py-1 rounded bg-emerald-500/20 text-emerald-300"
                        } else {
                            "px-2 py-1 rounded bg-amber-500/20 text-amber-300"
                        }
                    }>
                        {move || if ws_connected.get() { "实时已连接" } else { "实时未连接" }}
                    </span>
                </div>
            </header>

            <main class="grid grid-cols-[2fr_1fr] min-h-0">
                <section class="relative min-h-0">
                    <div id="map" class="absolute inset-0"></div>
                </section>

                <aside class="border-l border-slate-800 bg-slate-900/70 min-h-0 overflow-auto p-4 space-y-4">
                    <section class="rounded-lg border border-slate-700 p-3 space-y-3">
                        <h2 class="font-medium">"账号登录"</h2>
                        <div class="space-y-2">
                            <input class="w-full rounded bg-slate-950 border border-slate-700 px-3 py-2 text-sm" placeholder="用户名" prop:value=move || username.get() on:input=move |ev| username.set(event_target_value(&ev)) />
                            <input class="w-full rounded bg-slate-950 border border-slate-700 px-3 py-2 text-sm" placeholder="密码" r#type="password" prop:value=move || password.get() on:input=move |ev| password.set(event_target_value(&ev)) />
                            <button class="w-full rounded bg-sky-500 hover:bg-sky-400 text-slate-950 font-medium py-2" on:click=on_login>"登录（不存在则自动注册）"</button>
                        </div>
                        <p class="text-xs text-slate-400">{move || status.get()}</p>
                        <Show when=move || session.get().is_some()>
                            <div class="text-xs text-slate-300 rounded border border-slate-700 p-2 space-y-1">
                                <p>{move || format!("用户: {}", session.get().map(|s| s.username).unwrap_or_default())}</p>
                                <p>{move || format!("ID: {}", session.get().map(|s| s.user_id).unwrap_or_default())}</p>
                            </div>
                        </Show>
                    </section>

                    <section class="rounded-lg border border-slate-700 p-3 space-y-3">
                        <h2 class="font-medium">"房间状态"</h2>
                        <div class="flex gap-2">
                            <input class="flex-1 rounded bg-slate-950 border border-slate-700 px-2 py-1 text-xs" placeholder="房间ID（默认global）" prop:value=move || room_id.get() on:input=move |ev| room_id.set(event_target_value(&ev)) />
                            <button class="rounded bg-slate-700 hover:bg-slate-600 px-2 py-1 text-xs" on:click=on_load_history>"加载历史"</button>
                            <button class="rounded bg-slate-700 hover:bg-slate-600 px-2 py-1 text-xs" on:click=on_load_older_history>"更早"</button>
                            <button class="rounded bg-slate-700 hover:bg-slate-600 px-2 py-1 text-xs" on:click=on_load_newer_history>"较新"</button>
                            <button class="rounded bg-cyan-600 hover:bg-cyan-500 px-2 py-1 text-xs" on:click=on_mark_read>"标记已读"</button>
                        </div>
                        <p class="text-[11px] text-slate-500">{move || format!("历史页: {} (每页{}条)", history_page.get(), CHAT_HISTORY_PAGE_SIZE)}</p>
                        <div class="max-h-24 overflow-auto rounded border border-slate-800 p-2 text-xs text-slate-300 space-y-1">
                            {move || {
                                #[cfg(feature = "hydrate")]
                                {
                                    let state_opt = match room_state.get() {
                                        Some(wrapped) => wrapped.take(),
                                        None => None,
                                    };
                                    if let Some(state) = state_opt {
                                        state.members.into_iter().map(|m| {
                                            let short = m.user_id.chars().take(8).collect::<String>();
                                            view! { <p>{format!("{} - {}", short, if m.online { "online" } else { "offline" })}</p> }
                                        }).collect_view().into_any()
                                    } else {
                                        view! { <p class="text-slate-500">"暂无成员状态"</p> }.into_any()
                                    }
                                }

                                #[cfg(not(feature = "hydrate"))]
                                {
                                    let _ = &room_state;
                                    view! { <p class="text-slate-500">"SSR 模式不展示实时成员"</p> }.into_any()
                                }
                            }}
                        </div>
                    </section>

                    <section class="rounded-lg border border-slate-700 p-3 space-y-3">
                        <h2 class="font-medium">"在线用户与邀请"</h2>
                        <Suspense fallback=move || view! { <p class="text-sm text-slate-400">"加载中..."</p> }>
                            {move || {
                                nearby.get().map(|items| {
                                    let rows = items.as_ref().cloned().unwrap_or_default();
                                    if rows.is_empty() {
                                        view! { <p class="text-sm text-slate-400">"暂无在线用户"</p> }.into_any()
                                    } else {
                                        view! {
                                            <ul class="space-y-2 text-sm">
                                                {rows.into_iter().map(|row| {
                                                    let short_id = row.user_id.chars().take(8).collect::<String>();
                                                    let uid = row.user_id.clone();
                                                    view! {
                                                        <li class="rounded border border-slate-700 p-2 bg-slate-950/70">
                                                            <div class="flex items-center justify-between gap-2">
                                                                <button class="font-mono text-xs text-sky-300 hover:text-sky-200" on:click={
                                                                    let selected_user = selected_user;
                                                                    let uid = uid.clone();
                                                                    move |_| selected_user.set(uid.clone())
                                                                }>{short_id}</button>
                                                                <span class="text-xs text-slate-400">{format!("{:.0}m", row.distance_m)}</span>
                                                            </div>
                                                            <p class="text-xs text-slate-500">{format!("{:.6}, {:.6}", row.lon, row.lat)}</p>
                                                        </li>
                                                    }
                                                }).collect_view()}
                                            </ul>
                                        }
                                        .into_any()
                                    }
                                })
                            }}
                        </Suspense>
                        <div class="flex gap-2">
                            <input class="flex-1 rounded bg-slate-950 border border-slate-700 px-2 py-1 text-xs" placeholder="目标用户ID" prop:value=move || selected_user.get() on:input=move |ev| selected_user.set(event_target_value(&ev)) />
                            <button class="rounded bg-violet-500 hover:bg-violet-400 text-slate-950 font-medium px-3 py-1 text-xs" on:click=on_send_invite>"发邀请"</button>
                        </div>
                        <div class="max-h-32 overflow-auto rounded border border-slate-800 p-2 text-xs text-slate-300 space-y-1">
                            {move || pending_invites.get().into_iter().map(|inv| {
                                let invite_id_accept = inv.invite_id.clone();
                                let invite_id_reject = inv.invite_id.clone();
                                view! {
                                    <div class="border border-slate-700 rounded p-2 space-y-1">
                                        <p>{format!("来自 {} 的 {} 邀请", inv.from_user.chars().take(8).collect::<String>(), inv.mode)}</p>
                                        <p class="text-slate-500">{format!("{} | {}", inv.status, inv.ts.format("%H:%M:%S"))}</p>
                                        <div class="flex gap-2">
                                            <button class="rounded bg-emerald-500 hover:bg-emerald-400 text-slate-950 px-2 py-1" on:click={
                                                let on_respond_invite = on_respond_invite;
                                                move |_| on_respond_invite(invite_id_accept.clone(), "accept")
                                            }>"接受"</button>
                                            <button class="rounded bg-rose-500 hover:bg-rose-400 text-slate-950 px-2 py-1" on:click={
                                                let on_respond_invite = on_respond_invite;
                                                move |_| on_respond_invite(invite_id_reject.clone(), "reject")
                                            }>"拒绝"</button>
                                        </div>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                        <div class="max-h-24 overflow-auto rounded border border-slate-800 p-2 text-xs text-slate-300 space-y-1">
                            {move || invite_events.get().into_iter().rev().map(|line| view!{ <p>{line}</p>}).collect_view()}
                        </div>
                    </section>

                    <section class="rounded-lg border border-slate-700 p-3 space-y-3">
                        <h2 class="font-medium">"聊天室"</h2>
                        <div class="flex gap-2">
                            <input class="flex-1 rounded bg-slate-950 border border-slate-700 px-2 py-1 text-xs" placeholder="输入消息" prop:value=move || chat_input.get() on:input=move |ev| chat_input.set(event_target_value(&ev)) />
                            <button class="rounded bg-emerald-500 hover:bg-emerald-400 text-slate-950 font-medium px-3 py-1 text-xs" on:click=on_send_chat>"发送"</button>
                        </div>
                        <div class="max-h-56 overflow-auto rounded border border-slate-800 p-2 text-xs text-slate-300 space-y-1">
                            {move || chat_messages.get().into_iter().rev().map(|line| view!{ <p>{line}</p>}).collect_view()}
                        </div>
                    </section>
                </aside>
            </main>
        </div>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet href="/style/output.css" />
        <Stylesheet href="https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.css" />
        <Script src="https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.js"></Script>
        <Title text="Social Map Platform" />
        <Router>
            <Routes fallback=|| "Not Found">
                <Route path=StaticSegment("") view=HomePage />
            </Routes>
        </Router>
    }
}

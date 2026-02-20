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

#[derive(Debug, Clone)]
struct Session {
    token: String,
    user_id: String,
    username: String,
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
    let on_message = Closure::wrap(Box::new(move |_event: web_sys::MessageEvent| {
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

    let session = RwSignal::new(None::<Session>);
    let my_position = RwSignal::new((116.397, 39.908));
    let refresh_tick = RwSignal::new(0_u64);
    let ws_connected = RwSignal::new(false);
    let status = RwSignal::new("请先登录以开启实时联调".to_string());

    #[cfg(feature = "hydrate")]
    Effect::new(|_| {
        crate::map::mount_map();
        crate::map::set_center(116.397, 39.908);
    });

    let nearby = LocalResource::new(move || {
        let (_, _) = my_position.get();
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

            leptos::task::spawn_local(async move {
                status_setter.set("登录中...".to_string());
                let payload = AuthBody {
                    username: username_value,
                    password: password_value,
                };
                let Ok(body) = serde_json::to_string(&payload) else {
                    status_setter.set("登录请求序列化失败".to_string());
                    return;
                };

                let request = gloo_net::http::Request::post("/api/login")
                    .header("content-type", "application/json")
                    .body(body.clone());

                let auth = match request {
                    Ok(req) => match req.send().await {
                        Ok(resp) => resp.json::<AuthResult>().await.ok(),
                        Err(_) => None,
                    },
                    Err(_) => None,
                };

                let resolved = if let Some(a) = auth {
                    if !a.token.is_empty() {
                        Some(a)
                    } else {
                        None
                    }
                } else {
                    None
                };

                let final_auth = if let Some(ok_auth) = resolved {
                    ok_auth
                } else {
                    let reg_req = gloo_net::http::Request::post("/api/register")
                        .header("content-type", "application/json")
                        .body(body);
                    match reg_req {
                        Ok(req) => match req.send().await {
                            Ok(resp) => match resp.json::<AuthResult>().await {
                                Ok(a) => a,
                                Err(_) => {
                                    status_setter.set("注册返回解析失败".to_string());
                                    return;
                                }
                            },
                            Err(_) => {
                                status_setter.set("注册失败".to_string());
                                return;
                            }
                        },
                        Err(_) => {
                            status_setter.set("注册请求构建失败".to_string());
                            return;
                        }
                    }
                };

                if final_auth.token.is_empty() {
                    status_setter.set("登录失败：凭据无效".to_string());
                    return;
                }

                session_setter.set(Some(Session {
                    token: final_auth.token.clone(),
                    user_id: final_auth.user_id.clone(),
                    username: final_auth.username.clone(),
                }));

                status_setter.set(format!("已登录：{}", final_auth.username));
                tick.update(|v| *v += 1);

                connect_realtime(
                    final_auth.token,
                    final_auth.user_id,
                    pos,
                    ws_state,
                    tick,
                    status_setter,
                );
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

    view! {
        <div class="h-screen w-screen grid grid-rows-[auto_1fr] bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center justify-between bg-slate-900/80 backdrop-blur">
                <div>
                    <h1 class="text-lg font-semibold">"社交地图 + 实时通讯 + 高性能网页游戏"</h1>
                    <p class="text-xs text-slate-400">"Leptos + MapLibre + WebSocket + Redis + PostGIS"</p>
                </div>
                <div class="text-sm flex items-center gap-4">
                    <span class="px-2 py-1 rounded border border-slate-700">{move || format!("在线: {}", online_count())}</span>
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
                            <input
                                class="w-full rounded bg-slate-950 border border-slate-700 px-3 py-2 text-sm"
                                placeholder="用户名"
                                prop:value=move || username.get()
                                on:input=move |ev| username.set(event_target_value(&ev))
                            />
                            <input
                                class="w-full rounded bg-slate-950 border border-slate-700 px-3 py-2 text-sm"
                                placeholder="密码"
                                r#type="password"
                                prop:value=move || password.get()
                                on:input=move |ev| password.set(event_target_value(&ev))
                            />
                            <button
                                class="w-full rounded bg-sky-500 hover:bg-sky-400 text-slate-950 font-medium py-2"
                                on:click=on_login
                            >
                                "登录（不存在则自动注册）"
                            </button>
                        </div>
                        <p class="text-xs text-slate-400">{move || status.get()}</p>
                        <Show when=move || session.get().is_some()>
                            <div class="text-xs text-slate-300 rounded border border-slate-700 p-2 space-y-1">
                                <p>{move || format!("用户: {}", session.get().map(|s| s.username).unwrap_or_default())}</p>
                                <p>{move || format!("ID: {}", session.get().map(|s| s.user_id).unwrap_or_default())}</p>
                                <p>{move || format!("Token长度: {}", session.get().map(|s| s.token.len()).unwrap_or_default())}</p>
                            </div>
                        </Show>
                    </section>

                    <section class="rounded-lg border border-slate-700 p-3 space-y-3">
                        <h2 class="font-medium">"实时在线用户（5km）"</h2>
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
                                                    view! {
                                                        <li class="rounded border border-slate-700 p-2 bg-slate-950/70">
                                                            <div class="flex items-center justify-between gap-2">
                                                                <span class="font-mono text-xs text-sky-300">{short_id}</span>
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
        <Title text="Social Map Platform" />
        <Router>
            <Routes fallback=|| "Not Found">
                <Route path=StaticSegment("") view=HomePage />
            </Routes>
        </Router>
    }
}

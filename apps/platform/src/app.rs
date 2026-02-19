use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::components::*;
use leptos_router::StaticSegment;

#[cfg(feature = "hydrate")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "hydrate")]
use wasm_bindgen::{closure::Closure, JsCast};

#[cfg(feature = "hydrate")]
#[derive(Debug, Clone, Serialize)]
struct RegisterBody {
    username: String,
    password: String,
}

#[cfg(feature = "hydrate")]
#[derive(Debug, Clone, Deserialize)]
struct AuthResult {
    token: String,
    user_id: String,
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
fn start_realtime_loop() {
    leptos::task::spawn_local(async move {
        let username = format!("demo-{}", uuid::Uuid::new_v4().simple());
        let payload = RegisterBody {
            username,
            password: "demo-password".to_string(),
        };

        let Ok(body) = serde_json::to_string(&payload) else {
            return;
        };
        let req = gloo_net::http::Request::post("/api/register")
            .header("content-type", "application/json")
            .body(body)
        ;
        let Ok(req) = req else {
            return;
        };
        let Ok(resp) = req.send().await else {
            return;
        };

        let Ok(auth) = resp.json::<AuthResult>().await else {
            return;
        };

        if auth.token.is_empty() || auth.user_id.is_empty() {
            return;
        }

        let Some(url) = ws_url(&auth.token) else {
            return;
        };
        let Ok(ws) = web_sys::WebSocket::new(&url) else {
            return;
        };
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let Ok(user_id) = uuid::Uuid::parse_str(&auth.user_id) else {
            return;
        };

        let ws_for_tick = ws.clone();
        let tick = Closure::wrap(Box::new(move || {
            let lon = 116.397 + (js_sys::Math::random() - 0.5) * 0.01;
            let lat = 39.908 + (js_sys::Math::random() - 0.5) * 0.01;
            let packet = shared::RealtimePacket::Position(shared::PositionUpdate {
                user_id,
                lon,
                lat,
                ts: chrono::Utc::now(),
            });

            if let Ok(bin) = rmp_serde::to_vec(&packet) {
                let _ = ws_for_tick.send_with_u8_array(&bin);
            }
        }) as Box<dyn FnMut()>);

        if let Some(window) = web_sys::window() {
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                3000,
            );
        }
        tick.forget();
    });
}

#[server(name = QueryNearby, prefix = "/api")]
pub async fn query_nearby(lon: f64, lat: f64, radius_m: i32) -> Result<Vec<String>, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let app_state = crate::server::state::APP_STATE
            .get()
            .ok_or_else(|| ServerFnError::new("server state not initialized"))?;
        let users = crate::server::services::spatial::nearby_users(&app_state.pg, lon, lat, radius_m)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        return Ok(users.into_iter().map(|u| format!("{}:{}m", u.user_id, u.distance_m)).collect());
    }

    #[cfg(not(feature = "ssr"))]
    {
        let _ = (lon, lat, radius_m);
        Ok(vec![])
    }
}

#[component]
pub fn HomePage() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    Effect::new(|_| {
        crate::map::mount_map();
        start_realtime_loop();
    });

    let nearby = LocalResource::new(|| async move { query_nearby(116.397, 39.908, 5_000).await.ok() });

    view! {
        <div class="h-screen w-screen grid grid-rows-[auto_1fr]">
            <header class="px-6 py-3 border-b border-slate-800 flex items-center justify-between">
                <h1 class="text-lg font-semibold">"社交地图 + 实时通讯 + 游戏平台"</h1>
                <span class="text-sm text-slate-400">"Leptos 0.7 / Axum / PostGIS / Redis / NATS"</span>
            </header>
            <main class="grid grid-cols-[2fr_1fr]">
                <section class="relative">
                    <div id="map" class="absolute inset-0"></div>
                </section>
                <aside class="border-l border-slate-800 p-4 space-y-3 overflow-auto">
                    <h2 class="font-medium">"附近在线用户（Server Function + PostGIS）"</h2>
                    <Suspense fallback=move || view! { <p>"加载中..."</p> }>
                        {move || {
                            nearby
                                .get()
                                .map(|items| {
                                    view! {
                                        <ul class="space-y-2 text-sm">
                                            {items.as_ref().cloned().unwrap_or_default().into_iter().map(|row| view! { <li class="rounded border border-slate-700 p-2">{row}</li> }).collect_view()}
                                        </ul>
                                    }
                                })
                        }}
                    </Suspense>
                    <p class="text-xs text-slate-400">
                        "地图由 MapLibre GL JS 渲染，Wasm 通过 wasm-bindgen/js-sys/web-sys 控制。"
                    </p>
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

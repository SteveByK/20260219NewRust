pub mod app;
#[cfg(feature = "hydrate")]
pub mod map;
#[cfg(feature = "ssr")]
pub mod server;

pub use app::App;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
	leptos::mount::mount_to_body(App);
}

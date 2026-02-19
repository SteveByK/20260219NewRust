use wasm_bindgen::prelude::*;
use web_sys::window;

#[wasm_bindgen(inline_js = r#"
export function initMap(targetId) {
  const styleUrl = 'https://demotiles.maplibre.org/style.json';
  if (!window.maplibregl) {
    console.warn('MapLibre GL JS not loaded. Add it via CDN in production.');
    return null;
  }
  const map = new window.maplibregl.Map({
    container: targetId,
    style: styleUrl,
    center: [116.397, 39.908],
    zoom: 10
  });
  return map;
}
"#)]
extern "C" {
    fn initMap(target_id: &str) -> JsValue;
}

pub fn mount_map() {
    if window().is_none() {
        return;
    }
    let _ = initMap("map");
}

use wasm_bindgen::prelude::*;
use web_sys::window;

#[wasm_bindgen(inline_js = r#"
let appMap = null;
const SOURCE_ID = 'online-users';

export function initMap(targetId) {
  const styleUrl = 'https://demotiles.maplibre.org/style.json';
  if (!window.maplibregl) {
    console.warn('MapLibre GL JS not loaded. Add it via CDN in production.');
    return null;
  }
  if (appMap) {
    return appMap;
  }

  const map = new window.maplibregl.Map({
    container: targetId,
    style: styleUrl,
    center: [116.397, 39.908],
    zoom: 11,
    attributionControl: false
  });

  map.on('load', () => {
    if (!map.getSource(SOURCE_ID)) {
      map.addSource(SOURCE_ID, {
        type: 'geojson',
        data: {
          type: 'FeatureCollection',
          features: []
        }
      });
    }

    if (!map.getLayer('online-users-circle')) {
      map.addLayer({
        id: 'online-users-circle',
        type: 'circle',
        source: SOURCE_ID,
        paint: {
          'circle-radius': 8,
          'circle-color': '#38bdf8',
          'circle-stroke-color': '#0f172a',
          'circle-stroke-width': 2
        }
      });
    }

    if (!map.getLayer('online-users-label')) {
      map.addLayer({
        id: 'online-users-label',
        type: 'symbol',
        source: SOURCE_ID,
        layout: {
          'text-field': ['get', 'label'],
          'text-size': 11,
          'text-offset': [0, 1.5],
          'text-anchor': 'top'
        },
        paint: {
          'text-color': '#e2e8f0'
        }
      });
    }
  });

  appMap = map;
  return appMap;
}

export function updateOnlineUsersGeoJson(featureCollectionJson) {
  if (!appMap) {
    return;
  }
  const source = appMap.getSource(SOURCE_ID);
  if (!source) {
    return;
  }
  source.setData(JSON.parse(featureCollectionJson));
}

export function setMapCenter(lon, lat) {
  if (!appMap) {
    return;
  }
  appMap.easeTo({ center: [lon, lat], duration: 400 });
}
"#)]
extern "C" {
    fn initMap(target_id: &str) -> JsValue;
    fn updateOnlineUsersGeoJson(feature_collection_json: &str);
    fn setMapCenter(lon: f64, lat: f64);
}

pub fn mount_map() {
    if window().is_none() {
        return;
    }
    let _ = initMap("map");
}

pub fn update_online_users_geojson(feature_collection_json: &str) {
    if window().is_none() {
        return;
    }
    updateOnlineUsersGeoJson(feature_collection_json);
}

pub fn set_center(lon: f64, lat: f64) {
    if window().is_none() {
        return;
    }
    setMapCenter(lon, lat);
}

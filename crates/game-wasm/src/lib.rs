use bevy::prelude::*;

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run_game() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2dBundle::default());
}

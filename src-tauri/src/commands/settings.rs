use crate::settings::AppSettings;
use tauri_plugin_store::StoreExt;

#[tauri::command]
pub async fn get_settings(
    app: tauri::AppHandle,
) -> Result<AppSettings, String> {
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to open store: {}", e))?;

    let settings = if let Some(val) = store.get("settings") {
        serde_json::from_value(val).unwrap_or_default()
    } else {
        AppSettings::default()
    };

    Ok(settings)
}

#[tauri::command]
pub async fn set_settings(
    app: tauri::AppHandle,
    settings: AppSettings,
) -> Result<(), String> {
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to open store: {}", e))?;

    let val = serde_json::to_value(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    store.set("settings", val);

    Ok(())
}

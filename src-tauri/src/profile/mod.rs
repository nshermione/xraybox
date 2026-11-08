pub mod country;
pub mod ping;
pub mod share;
pub mod store;
pub mod subscription;
pub mod vless;

use tauri::AppHandle;

use crate::engine::error::EngineError;

pub use store::{load_manual_profile, save_manual_profile};
pub use subscription::{FetchInterval, ProfileNode, SubscriptionSettings};
pub use vless::VlessReality;

pub fn sync_smart_selected_id(
    app: &AppHandle,
    settings: &mut SubscriptionSettings,
    nodes: &[ProfileNode],
    tunnel_running: bool,
) -> Result<(), EngineError> {
    if tunnel_running {
        return Ok(());
    }
    let engine = crate::engine::settings::load_engine_settings(app)?;
    if !engine.smart_connect {
        return Ok(());
    }
    let Some(best) = ping::pick_best_node(nodes) else {
        return Ok(());
    };
    if settings.selected_profile_id.as_deref() == Some(best.id.as_str()) {
        return Ok(());
    }
    settings.selected_profile_id = Some(best.id.clone());
    subscription::save_subscription(app, settings)
}

pub fn resolve_connect_profile(app: &AppHandle) -> Result<VlessReality, EngineError> {
    let engine = crate::engine::settings::load_engine_settings(app)?;
    let mut settings = subscription::load_subscription(app)?;
    let mut nodes = subscription::load_nodes(app)?;

    if engine.smart_connect && !nodes.is_empty() {
        if nodes.iter().all(|node| node.ping_ms.is_empty()) {
            ping::ping_nodes(&mut nodes);
            subscription::save_nodes(app, &nodes)?;
        }
        if let Some(best) = ping::pick_best_node(&nodes) {
            settings.selected_profile_id = Some(best.id.clone());
            subscription::save_subscription(app, &settings)?;
            return Ok(best.profile.clone());
        }
        settings.selected_profile_id = Some(nodes[0].id.clone());
        subscription::save_subscription(app, &settings)?;
        return Ok(nodes[0].profile.clone());
    }

    if let Some(id) = &settings.selected_profile_id {
        if let Some(node) = nodes.iter().find(|node| node.id == *id) {
            return Ok(node.profile.clone());
        }
    }

    if let Some(first) = nodes.first() {
        return Ok(first.profile.clone());
    }

    load_manual_profile(app)?.ok_or(EngineError::NoProfile)
}

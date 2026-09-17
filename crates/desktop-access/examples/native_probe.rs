use serde_json::json;
use vox_desktop_access::{list_native_windows, NativeDesktop};

fn main() {
    match list_native_windows() {
        Ok(windows) => println!(
            "windows={}",
            json!({
                "count": windows.len(),
                "items": windows.into_iter().map(|window| json!({
                    "app_name": window.app_name,
                    "app_pid": window.app_pid,
                    "window_id": window.window_id,
                    "role": window.role,
                    "states": window.states,
                    "bounds": window.bounds,
                    "title_present": !window.name.is_empty(),
                })).collect::<Vec<_>>(),
            })
        ),
        Err(error) => println!("windows={}", json!({ "error": error })),
    }

    match NativeDesktop::default().snapshots() {
        Ok(snapshots) => {
            let first_window_ids = snapshots
                .iter()
                .map(|snapshot| (snapshot.app_id.clone(), snapshot.window_id.clone()))
                .collect::<std::collections::BTreeSet<_>>();
            let repeated_window_ids = NativeDesktop::default().snapshots().ok().map(|snapshots| {
                snapshots
                    .into_iter()
                    .map(|snapshot| (snapshot.app_id, snapshot.window_id))
                    .collect::<std::collections::BTreeSet<_>>()
            });
            let repeated_matches = repeated_window_ids
                .as_ref()
                .map(|ids| first_window_ids.intersection(ids).count());
            println!(
                "snapshots={}",
                json!({
                    "count": snapshots.len(),
                    "repeat_window_id_matches": repeated_matches,
                    "items": snapshots.into_iter().map(|snapshot| json!({
                    "app_id": snapshot.app_id,
                    "window_id": snapshot.window_id,
                    "node_count": snapshot.nodes.len(),
                    "interactive_nodes": snapshot.nodes.iter()
                        .filter(|node| !node.actions.is_empty())
                        .count(),
                    "truncated": snapshot.truncated,
                    })).collect::<Vec<_>>(),
                })
            );
        }
        Err(error) => println!("snapshots={}", json!({ "error": error })),
    }
}

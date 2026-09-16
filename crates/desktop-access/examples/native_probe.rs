use vox_desktop_access::{list_native_windows, NativeDesktop};

fn main() {
    let windows = list_native_windows();
    println!(
        "windows={}",
        serde_json::to_string(&windows)
            .unwrap_or_else(|error| format!("serialization-error:{error}"))
    );
    let snapshots = NativeDesktop::default().snapshots();
    println!(
        "snapshots={}",
        serde_json::to_string(&snapshots)
            .unwrap_or_else(|error| format!("serialization-error:{error}"))
    );
}

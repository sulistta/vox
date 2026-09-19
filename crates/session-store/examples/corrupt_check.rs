fn main() {
    let path = "/tmp/vox-corrupt-probe.sqlite";
    std::fs::write(path, b"not sqlite").unwrap();
    match vox_session_store::SessionStore::open(path) {
        Ok(_) => println!("OPENED (bad)"),
        Err(e) => println!("ERR: {e:?}"),
    }
}

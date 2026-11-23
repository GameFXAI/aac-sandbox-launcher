// The version info, icon and manifest (setup.rc) go into AAReplaySetup.exe.
fn main() {
    println!("cargo:rerun-if-changed=setup.rc");
    println!("cargo:rerun-if-changed=setup.manifest");
    println!("cargo:rerun-if-changed=aareplay.ico");
    embed_resource::compile("setup.rc", embed_resource::NONE).manifest_required().unwrap();
}

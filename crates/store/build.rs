// refinery::embed_migrations! embeds SQL at compile time. Cargo does not
// watch that directory by default, so a new migration alone used to leave
// braian-store stale until FORCE_DEPLOY / cargo clean.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}

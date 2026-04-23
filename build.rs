fn main() {
    #[cfg(windows)]
    embed_resource::compile("hupdater.rc");
}

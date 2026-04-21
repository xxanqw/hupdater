fn main() {
    #[cfg(windows)]
    embed_resource::compile("hupdater.rc");

    slint_build::compile("ui/app.slint").expect("Slint compilation failed");
}

use tera::Context;

fn default_context() -> Context {
    // add hostname, arch, os, etc
    let mut context = Context::new();

    if let Ok(hostname) = hostname::get() {
        context.insert("hostname", &hostname);
    }

    // Arch
    #[cfg(target_arch = "x86_64")]
    context.insert("arch", "x86_64");

    #[cfg(target_arch = "aarch64")]
    context.insert("arch", "aarch64");

    // OS
    #[cfg(target_os = "linux")]
    context.insert("os", "linux");

    #[cfg(target_os = "macos")]
    context.insert("os", "macos");

    #[cfg(target_os = "windows")]
    context.insert("os", "windows");

    // Misc info
    context.insert("nproc", &num_cpus::get());
    context.insert("wsl", &wsl::is_wsl());

    context
}

pub fn render(template: &str) -> Result<String, tera::Error> {
    let mut tera = tera::Tera::default();
    let context = default_context();

    tera.render_str(template, &context)
}

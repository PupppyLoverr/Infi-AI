static POSSIBLE_BACKENDS: &[&str] = &[
    #[cfg(feature = "winit")]
    "--winit : Run cosmos-compositor as a nested Wayland/X11 client using winit.",
    #[cfg(feature = "udev")]
    "--tty-udev : Run cosmos-compositor on a tty using udev (needs a seat via logind or seatd).",
];

fn main() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt()
            .compact()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().compact().init();
    }

    profiling::register_thread!("Main Thread");

    let arg = ::std::env::args().nth(1);
    match arg.as_ref().map(|s| &s[..]) {
        #[cfg(feature = "winit")]
        Some("--winit") => {
            tracing::info!("Starting cosmos-compositor with winit backend");
            cosmos_compositor::winit::run_winit();
        }
        #[cfg(feature = "udev")]
        Some("--tty-udev") => {
            tracing::info!("Starting cosmos-compositor on a tty using udev");
            cosmos_compositor::udev::run_udev();
        }
        Some(other) => {
            tracing::error!("Unknown backend: {}", other);
        }
        None => {
            #[allow(clippy::disallowed_macros)]
            {
                println!("USAGE: cosmos-compositor --backend");
                println!();
                println!("Possible backends are:");
                for backend in POSSIBLE_BACKENDS {
                    println!("{}", backend);
                }
            }
        }
    }
}

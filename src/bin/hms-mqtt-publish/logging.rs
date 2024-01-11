use env_logger::{Builder, Env};

pub fn init_logger() {
    Builder::from_env(Env::default().default_filter_or("info")).init();
}

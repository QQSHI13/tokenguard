//! Desktop-notification abstraction so the core state can notify the user
//! when a GUI runtime is available, while remaining usable in headless mode.

/// A small notifier interface for limit and proxy state events.
pub trait Notifier: Send + Sync + std::any::Any {
    fn limit_warning(&self, name: &str, used: f64, cap: f64);
    fn limit_blocked(&self, name: &str, used: f64, cap: f64);
    fn limit_paused(&self, name: &str, used: f64, cap: f64);
    fn proxy_paused(&self);
    fn proxy_resumed(&self);
}

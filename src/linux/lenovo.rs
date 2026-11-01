//! Lenovo performance mode (Fn+Q) is read through Windows WMI; nothing to listen to on Linux.

/// Does nothing on Linux.
pub fn watch(_on_mode: impl Fn(i32) + Send + 'static) {}

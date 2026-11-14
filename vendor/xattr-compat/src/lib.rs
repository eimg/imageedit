//! GPUI 0.2.2 -> gpui_http_client -> zed-async-tar requires xattr 0.2.
//! That release references libc::ENOATTR, which modern libc no longer exports
//! on Linux. The archive code only uses `xattr::set`; the maintained crate keeps
//! that API and symlink behavior, and uses the correct platform error constants.
//! Re-export it rather than pinning libc or copying obsolete unsafe FFI code.
//! Remove this bridge when GPUI's archive dependency is updated.
pub use xattr_current::*;

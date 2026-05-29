# Assets

Drop `icon.ico` here (any 256×256 multi-resolution .ico works best — Windows
will pick the appropriate size at runtime). The build script
([`../build.rs`](../build.rs)) picks it up automatically on the next
`cargo build --release` and embeds it into `cmd_zoo.exe`.

Until you add one, the exe falls back to Windows' default application icon
and `cargo build` emits a `cargo:warning` reminding you.

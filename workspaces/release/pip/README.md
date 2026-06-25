# Incan Toolchain Python Installer

Incan is a statically typed, Pythonic programming language for writing clear high-level application code that compiles to native Rust. Learn more at [incan.io](https://incan.io).

This package is a thin installer and command shim for the Incan toolchain. It installs verified toolchain archives from the shared Incan release manifest instead of building the compiler from Python packaging.

```bash
pipx install incan
incan --version
```

The command shims install the toolchain into a package-local cache on first use. Set `INCAN_PIP_TOOLCHAIN_HOME`, `INCAN_PIP_BIN_DIR`, or `INCAN_TOOLCHAIN_MANIFEST` when you need a custom cache location or manifest.

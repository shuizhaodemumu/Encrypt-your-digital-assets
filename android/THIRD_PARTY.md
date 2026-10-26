# Third-party components

The MIT license covers this project's original code only. Third-party software keeps its own license.

- LibVLC Android 3.7.7 (LGPL 2.1, as declared by the artifact POM): official Maven artifact `org.videolan.android:libvlc-all:3.7.7`. Upstream: https://code.videolan.org/videolan/libvlcjni and https://www.videolan.org/vlc/libvlc.html . No LibVLC source modifications are made by this project. Consult upstream for its license texts, bundled modules and corresponding source.
- AndroidX and Kotlin libraries: upstream Android / JetBrains components. Original notices present in the reused JAR/AAR files are collected below where available.
- Rust dependencies: exact versions in `core/Cargo.lock`. Included upstream license files and package license expressions are collected in `third-party-licenses`.
- Windows and Ubuntu artwork is reused from this repository. Rust/Android build scripts currently reference the original development machine toolchain; adjust paths to build elsewhere.

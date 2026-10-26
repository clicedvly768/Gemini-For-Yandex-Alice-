project('Gemini-for-alice', 'rust',
  version: '0.1.0',
  meson_version: '>= 0.60.0',
  default_options: ['warning_level=1']
)

cargo = find_program('cargo', required: true)

# 1. Native x86_64 release build
run_target('build-x86_64',
  command: [cargo, 'build', '--release']
)

# 2. ARM64 Musl target for Yandex Station (Amlogic A113X, Linux kernel 4.9)
run_target('build-arm64-musl',
  command: [cargo, 'zigbuild', '--target', 'aarch64-unknown-linux-musl', '--release']
)

# 3. ARM32 Musl target (armv7)
run_target('build-armv7-musl',
  command: [cargo, 'zigbuild', '--target', 'armv7-unknown-linux-musleabihf', '--release']
)

# 4. x86_64 static Musl target
run_target('build-x86_64-musl',
  command: [cargo, 'zigbuild', '--target', 'x86_64-unknown-linux-musl', '--release']
)

# 5. Build all targets at once
run_target('build-all',
  command: [
    'sh', '-c',
    'cargo build --release && cargo zigbuild --target aarch64-unknown-linux-musl --release && cargo zigbuild --target armv7-unknown-linux-musleabihf --release && cargo zigbuild --target x86_64-unknown-linux-musl --release'
  ]
)

# 6. Run local server
run_target('run',
  command: [cargo, 'run']
)

# 7. Push to Yandex Station via ADB
run_target('push-station',
  command: [
    'sh', '-c',
    'adb push target/aarch64-unknown-linux-musl/release/Gemini-for-alice /tmp/gemini-alice && adb push .env /tmp/.env && adb shell "chmod +x /tmp/gemini-alice"'
  ]
)
